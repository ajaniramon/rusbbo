//! Evaluation, with Pebble 3.1's semantics (scopes, truthiness, number types, escaping).

use std::collections::HashMap;
use std::sync::Arc;

use crate::Engine;
use crate::escape;
use crate::syntax::{BinOp, Expr, Node};
use crate::value::{Map, Value};

type R<T> = Result<T, String>;

pub(crate) struct Renderer<'e> {
    pub engine: &'e Engine,
    pub scopes: Vec<HashMap<String, Value>>,
    /// The template being rendered (for relative includes), and the include depth.
    pub path: String,
    pub depth: usize,
    /// `autoescape` is lexical in Pebble: per template, reset in included ones.
    pub escape: Option<String>,
}

impl Renderer<'_> {
    pub fn nodes(&mut self, nodes: &[Node], out: &mut String) -> R<()> {
        for node in nodes {
            self.node(node, out)?;
        }
        Ok(())
    }

    fn node(&mut self, node: &Node, out: &mut String) -> R<()> {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Print(e) => {
                let v = self.eval(e)?;
                match (&self.escape, &v, e) {
                    (_, Value::Safe(s), _) => out.push_str(s),
                    (Some(_), _, Expr::StrLit(_)) => v.write_java(out, false),
                    (Some(strategy), _, _) => {
                        let s = v.render();
                        out.push_str(&escape_with(strategy, &s)?);
                    }
                    (None, _, _) => v.write_java(out, false),
                }
            }
            Node::If {
                branches,
                otherwise,
            } => {
                for (cond, body) in branches {
                    let c = self.eval(cond)?;
                    if truthy(&c)? {
                        return self.nodes(body, out);
                    }
                }
                self.nodes(otherwise, out)?;
            }
            Node::For {
                var,
                iter,
                body,
                otherwise,
            } => {
                let items = match self.eval(iter)? {
                    Value::Null => return Ok(()),
                    Value::List(l) => l.as_ref().clone(),
                    Value::Map(m) => m.iter().map(|(k, v)| entry(k, v)).collect(),
                    Value::Str(s) | Value::Safe(s) => {
                        s.chars().map(|c| Value::str(c.to_string())).collect()
                    }
                    other => return Err(format!("cannot iterate over {other:?}")),
                };
                if items.is_empty() {
                    return self.nodes(otherwise, out);
                }
                self.scopes.push(HashMap::new());
                let len = items.len();
                let mut result = Ok(());
                for (i, item) in items.into_iter().enumerate() {
                    let scope = self.scopes.last_mut().unwrap();
                    scope.insert(var.clone(), item);
                    let lp = Map::new()
                        .with("index", i)
                        .with("revindex", len - i - 1)
                        .with("length", len)
                        .with("first", i == 0)
                        .with("last", i == len - 1);
                    scope.insert("loop".into(), lp.into());
                    result = self.nodes(body, out);
                    if result.is_err() {
                        break;
                    }
                }
                self.scopes.pop();
                result?;
            }
            Node::Set { name, value } => {
                let v = self.eval(value)?;
                self.set(name, v);
            }
            Node::Include { path, with } => {
                let p = self.eval(path)?.render();
                let resolved = resolve(&self.path, &p);
                let tpl = self.engine.load(&resolved)?;
                let mut scope = HashMap::new();
                if let Some(w) = with {
                    match self.eval(w)? {
                        Value::Map(m) => {
                            for (k, v) in m.iter() {
                                scope.insert(k.clone(), v.clone());
                            }
                        }
                        Value::Null => {}
                        other => return Err(format!("include with needs a map, got {other:?}")),
                    }
                }
                if self.depth > 64 {
                    return Err("include depth over 64".into());
                }
                let saved_path = std::mem::replace(&mut self.path, resolved);
                let saved_escape = self.escape.take();
                self.depth += 1;
                self.scopes.push(scope);
                let r = self.nodes(&tpl, out);
                self.scopes.pop();
                self.depth -= 1;
                self.escape = saved_escape;
                self.path = saved_path;
                r?;
            }
            Node::Autoescape { strategy, body } => {
                let saved = std::mem::replace(&mut self.escape, strategy.clone());
                let r = self.nodes(body, out);
                self.escape = saved;
                r?;
            }
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// Pebble's `ScopeChain.set`: the innermost scope that has the variable, else the current one.
    fn set(&mut self, name: &str, v: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = v;
                return;
            }
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), v);
    }

    pub fn eval(&mut self, e: &Expr) -> R<Value> {
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::StrLit(s) => Value::Str(s.clone()),
            Expr::Interp(parts) => {
                let mut s = String::new();
                for p in parts {
                    self.eval(p)?.write_java(&mut s, false);
                }
                Value::from(s)
            }
            Expr::Var(n) => force(self.lookup(n).cloned().unwrap_or(Value::Null))?,
            Expr::Attr(target, name) => {
                let t = self.eval(target)?;
                attr(&t, name)?
            }
            Expr::Index(target, idx) => {
                let t = self.eval(target)?;
                let i = self.eval(idx)?;
                match (&t, &i) {
                    (Value::List(l), Value::Int(n)) => {
                        if *n >= 0 {
                            l.get(*n as usize).cloned().unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    }
                    (Value::Map(_), _) => attr(&t, &i.render())?,
                    _ => Value::Null,
                }
            }
            Expr::Call(target, name, args) => {
                let t = self.eval(target)?;
                let args = args.iter().map(|a| self.eval(a)).collect::<R<Vec<_>>>()?;
                call(&t, name, &args)?
            }
            Expr::Function(name, args) => {
                let args = args.iter().map(|a| self.eval(a)).collect::<R<Vec<_>>>()?;
                match name.as_str() {
                    "range" => {
                        let a = args
                            .first()
                            .and_then(Value::as_i64)
                            .ok_or("range needs numbers")?;
                        let b = args
                            .get(1)
                            .and_then(Value::as_i64)
                            .ok_or("range needs numbers")?;
                        let step = args.get(2).and_then(Value::as_i64).unwrap_or(1);
                        range(a, b, step)?
                    }
                    "max" | "min" => {
                        let mut best: Option<Value> = None;
                        for a in args {
                            let better = match &best {
                                None => true,
                                Some(b) => {
                                    let (x, y) = (
                                        a.as_f64().ok_or("max/min need numbers")?,
                                        b.as_f64().unwrap(),
                                    );
                                    if name == "max" { x > y } else { x < y }
                                }
                            };
                            if better {
                                best = Some(a);
                            }
                        }
                        best.unwrap_or(Value::Null)
                    }
                    other => return Err(format!("unknown function {other}")),
                }
            }
            Expr::Filter(target, name, args) => {
                let v = self.eval(target)?;
                let args = args.iter().map(|a| self.eval(a)).collect::<R<Vec<_>>>()?;
                filter(v, name, &args)?
            }
            Expr::Test {
                value,
                name,
                negated,
                args,
            } => {
                let r = if name == "present" {
                    let v = self.eval(value)?;
                    let key = v.as_str().ok_or("present test only accepts a string")?;
                    self.lookup(key).is_some()
                } else {
                    let v = self.eval(value)?;
                    let _ = args;
                    match name.as_str() {
                        "null" => v.is_null(),
                        "empty" => match &v {
                            Value::Null => true,
                            Value::Str(s) | Value::Safe(s) => s.is_empty(),
                            Value::List(l) => l.is_empty(),
                            Value::Map(m) => m.is_empty(),
                            _ => false,
                        },
                        "even" => v.as_i64().ok_or("even needs a number")? % 2 == 0,
                        "odd" => v.as_i64().ok_or("odd needs a number")? % 2 != 0,
                        "iterable" => matches!(v, Value::List(_) | Value::Map(_)),
                        "map" => matches!(v, Value::Map(_)),
                        other => return Err(format!("unknown test {other}")),
                    }
                };
                Value::Bool(r != *negated)
            }
            Expr::Not(inner) => {
                let v = self.eval(inner)?;
                Value::Bool(!truthy(&v)?)
            }
            Expr::Neg(inner) => match self.eval(inner)? {
                Value::Int(i) => Value::Int(-i),
                Value::Float(f) => Value::Float(-f),
                other => return Err(format!("cannot negate {other:?}")),
            },
            Expr::Binary(op, a, b) => match op {
                BinOp::And => {
                    let a = self.eval(a)?;
                    Value::Bool(truthy(&a)? && truthy(&self.eval(b)?)?)
                }
                BinOp::Or => {
                    let a = self.eval(a)?;
                    Value::Bool(truthy(&a)? || truthy(&self.eval(b)?)?)
                }
                _ => {
                    let a = self.eval(a)?;
                    let b = self.eval(b)?;
                    binary(*op, &a, &b)?
                }
            },
            Expr::Ternary(c, a, b) => {
                let c = self.eval(c)?;
                if truthy(&c)? {
                    self.eval(a)?
                } else {
                    self.eval(b)?
                }
            }
            Expr::List(items) => {
                Value::list(items.iter().map(|i| self.eval(i)).collect::<R<Vec<_>>>()?)
            }
            Expr::MapLit(items) => {
                let mut m = Map::new();
                for (k, v) in items {
                    let k = self.eval(k)?.render();
                    let v = self.eval(v)?;
                    m.insert(k, v);
                }
                m.into()
            }
        })
    }
}

fn entry(k: &str, v: &Value) -> Value {
    Map::new().with("key", k).with("value", v.clone()).into()
}

fn force(v: Value) -> R<Value> {
    match v {
        Value::Func(f) => f(&[]),
        v => Ok(v),
    }
}

/// `includes` paths: `./` and `../` are relative to the including template.
fn resolve(current: &str, path: &str) -> String {
    if !(path.starts_with("./") || path.starts_with("../")) {
        return path.to_string();
    }
    let mut parts: Vec<&str> = current.split('/').collect();
    parts.pop();
    for seg in path.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn range(a: i64, b: i64, step: i64) -> R<Value> {
    if step == 0 {
        return Err("range step 0".into());
    }
    let mut out = Vec::new();
    let mut i = a;
    while (step > 0 && i <= b) || (step < 0 && i >= b) {
        out.push(Value::Int(i));
        if out.len() > 100_000 {
            return Err("range too long".into());
        }
        i += step;
    }
    Ok(Value::list(out))
}

/// Pebble's conditional conversion: booleans, null, numbers (non-zero) and strings (non-empty).
pub fn truthy(v: &Value) -> R<bool> {
    Ok(match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) | Value::Safe(s) => !s.is_empty(),
        _ => return Err("Wrong operand(s) type in conditional expression".into()),
    })
}

/// Pebble's `OperatorUtils.equals`.
pub fn equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        _ if a.is_number() && b.is_number() => a.as_f64() == b.as_f64(),
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => x == y,
            _ => match (a, b) {
                (Value::List(x), Value::List(y)) => {
                    x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| equals(p, q))
                }
                (Value::Map(x), Value::Map(y)) => Arc::ptr_eq(x, y),
                _ => false,
            },
        },
    }
}

fn binary(op: BinOp, a: &Value, b: &Value) -> R<Value> {
    use BinOp::*;
    Ok(match op {
        Eq => Value::Bool(equals(a, b)),
        Ne => Value::Bool(!equals(a, b)),
        // Comparing with null is false in Pebble.
        Lt | Gt | Le | Ge if a.is_null() || b.is_null() => Value::Bool(false),
        Lt | Gt | Le | Ge => {
            let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) else {
                return Err("Could not perform comparison".into());
            };
            Value::Bool(match op {
                Lt => x < y,
                Gt => x > y,
                Le => x <= y,
                _ => x >= y,
            })
        }
        Concat => {
            let mut s = String::new();
            a.write_java(&mut s, false);
            b.write_java(&mut s, false);
            Value::from(s)
        }
        Contains => Value::Bool(match a {
            Value::List(l) => l.iter().any(|x| equals(x, b)),
            Value::Str(s) | Value::Safe(s) => s.contains(&*b.render()),
            Value::Map(m) => m.contains_key(&b.render()),
            Value::Null => false,
            _ => return Err("contains needs a collection or string".into()),
        }),
        Range => {
            let (Some(x), Some(y)) = (a.as_i64(), b.as_i64()) else {
                return Err("range needs numbers".into());
            };
            range(x, y, 1)?
        }
        Add if !(a.is_number() && b.is_number()) => {
            if a.as_str().is_none() && b.as_str().is_none() {
                return Err("Could not perform addition".into());
            }
            let mut s = String::new();
            a.write_java(&mut s, true);
            b.write_java(&mut s, true);
            Value::from(s)
        }
        Add | Sub | Mul | Div | Mod => match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(match op {
                Add => x.wrapping_add(*y),
                Sub => x.wrapping_sub(*y),
                Mul => x.wrapping_mul(*y),
                Div => x.checked_div(*y).ok_or("/ by zero")?,
                _ => x.checked_rem(*y).ok_or("/ by zero")?,
            }),
            _ => {
                let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) else {
                    return Err("Could not perform arithmetic".into());
                };
                Value::Float(match op {
                    Add => x + y,
                    Sub => x - y,
                    Mul => x * y,
                    Div => x / y,
                    _ => x % y,
                })
            }
        },
        And | Or => unreachable!(),
    })
}

/// Bean property names Pebble tries for a method call: `getName()` and `isActive()` read the
/// `name`/`active` keys.
fn property_of(method: &str) -> Option<String> {
    for prefix in ["get", "is", "has"] {
        if let Some(rest) = method.strip_prefix(prefix) {
            let mut chars = rest.chars();
            if let Some(first) = chars.next().filter(|c| c.is_ascii_uppercase()) {
                return Some(first.to_ascii_lowercase().to_string() + chars.as_str());
            }
        }
    }
    None
}

/// A bean member the way Pebble finds one (`MemberCacheUtils`, method names compared ignoring
/// case): the key itself, then `get`/`is`/`has` + name, ignoring case.
fn member<'m>(m: &'m Map, name: &str) -> Option<&'m Value> {
    if let Some(v) = m.get(name) {
        return Some(v);
    }
    if let Some(v) = property_of(name).and_then(|p| m.get(&p)) {
        return Some(v);
    }
    let lower = name.to_ascii_lowercase();
    let stripped = ["get", "is", "has"]
        .iter()
        .find_map(|p| lower.strip_prefix(p).filter(|r| !r.is_empty()));
    m.iter()
        .find(|(k, _)| {
            let k = k.to_ascii_lowercase();
            k == lower || stripped.is_some_and(|s| k == s)
        })
        .map(|(_, v)| v)
}

/// `target.name`: a map key, else Pebble's bean getters on our maps.
fn attr(target: &Value, name: &str) -> R<Value> {
    match target {
        // `bean.isOnline` finds `isOnline()`, `bean.shortstory` finds `getShortStory()`.
        Value::Map(m) => match member(m, name) {
            Some(v) => force(v.clone()),
            None => Ok(Value::Null),
        },
        Value::List(l) => match name {
            // A Java List has size() and isEmpty(), no length.
            "size" => Ok(Value::from(l.len())),
            "empty" => Ok(Value::Bool(l.is_empty())),
            _ => Ok(name
                .parse::<usize>()
                .ok()
                .and_then(|i| l.get(i).cloned())
                .unwrap_or(Value::Null)),
        },
        Value::Str(s) | Value::Safe(s) => match name {
            "empty" => Ok(Value::Bool(s.is_empty())),
            "length" => Ok(Value::from(s.encode_utf16().count())),
            _ => Ok(Value::Null),
        },
        _ => Ok(Value::Null),
    }
}

/// `target.name(args)`.
fn call(target: &Value, name: &str, args: &[Value]) -> R<Value> {
    let arg_str = |i: usize| args.get(i).map(Value::render).unwrap_or_default();
    match target {
        Value::Map(m) => {
            match member(m, name) {
                Some(Value::Func(f)) => return f(args),
                Some(v) if args.is_empty() => return Ok(v.clone()),
                _ => {}
            }
            match name {
                "size" => Ok(Value::from(m.len())),
                "isEmpty" => Ok(Value::Bool(m.is_empty())),
                "get" => attr(target, &arg_str(0)),
                "containsKey" => Ok(Value::Bool(m.contains_key(&arg_str(0)))),
                "entrySet" => Ok(Value::list(
                    m.iter().map(|(k, v)| entry(k, v)).collect::<Vec<_>>(),
                )),
                "keySet" => Ok(Value::list(
                    m.iter().map(|(k, _)| Value::str(k)).collect::<Vec<_>>(),
                )),
                "values" => Ok(Value::list(
                    m.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
                )),
                "toString" => Ok(Value::from(target.render())),
                _ => Ok(Value::Null),
            }
        }
        Value::List(l) => Ok(match name {
            "size" => Value::from(l.len()),
            "isEmpty" => Value::Bool(l.is_empty()),
            "get" => {
                let i = args
                    .first()
                    .and_then(Value::as_i64)
                    .ok_or("get needs an index")?;
                l.get(i as usize).cloned().ok_or("index out of bounds")?
            }
            "contains" => Value::Bool(args.first().is_some_and(|a| l.iter().any(|x| equals(x, a)))),
            _ => Value::Null,
        }),
        Value::Str(s) | Value::Safe(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(match name {
                "length" => Value::from(s.encode_utf16().count()),
                "isEmpty" => Value::Bool(s.is_empty()),
                "toLowerCase" => Value::from(s.to_lowercase()),
                "toUpperCase" => Value::from(s.to_uppercase()),
                "trim" => Value::str(s.trim_matches(|c: char| c <= ' ')),
                "contains" => Value::Bool(s.contains(&*arg_str(0))),
                "startsWith" => Value::Bool(s.starts_with(&*arg_str(0))),
                "endsWith" => Value::Bool(s.ends_with(&*arg_str(0))),
                "equals" => Value::Bool(args.first().is_some_and(|a| a.as_str() == Some(s))),
                "equalsIgnoreCase" => Value::Bool(s.to_lowercase() == arg_str(0).to_lowercase()),
                "indexOf" => Value::Int(
                    s.find(&*arg_str(0))
                        .map_or(-1, |i| s[..i].chars().count() as i64),
                ),
                "replace" => Value::from(s.replace(&*arg_str(0), &arg_str(1))),
                "toString" => Value::Str(s.clone()),
                "substring" => {
                    let a = args
                        .first()
                        .and_then(Value::as_i64)
                        .ok_or("substring needs an index")? as usize;
                    let b = args
                        .get(1)
                        .and_then(Value::as_i64)
                        .map_or(chars.len(), |b| b as usize);
                    if a > b || b > chars.len() {
                        return Err("String index out of range".into());
                    }
                    Value::from(chars[a..b].iter().collect::<String>())
                }
                "charAt" => {
                    let a = args
                        .first()
                        .and_then(Value::as_i64)
                        .ok_or("charAt needs an index")? as usize;
                    Value::from(chars.get(a).ok_or("String index out of range")?.to_string())
                }
                _ => Value::Null,
            })
        }
        Value::Int(_) | Value::Float(_) => Ok(match name {
            "intValue" | "longValue" => Value::Int(target.as_i64().unwrap()),
            "doubleValue" => Value::Float(target.as_f64().unwrap()),
            "toString" => Value::from(target.render()),
            _ => Value::Null,
        }),
        Value::Func(f) if args.is_empty() => call(&f(&[])?, name, args),
        _ => Ok(Value::Null),
    }
}

fn escape_with(strategy: &str, s: &str) -> R<String> {
    Ok(match strategy {
        "html" => escape::html(s),
        "js" => escape::js(s),
        other => return Err(format!("unknown escaping strategy {other}")),
    })
}

fn filter(v: Value, name: &str, args: &[Value]) -> R<Value> {
    Ok(match name {
        "escape" | "e" => match v {
            Value::Safe(_) => v,
            Value::Null => Value::Null,
            v => {
                let strategy = args
                    .first()
                    .map(Value::render)
                    .unwrap_or_else(|| "html".into());
                Value::Safe(Arc::from(escape_with(&strategy, &v.render())?))
            }
        },
        "raw" => match v {
            Value::Null => Value::Null,
            Value::Str(s) => Value::Safe(s),
            v => v,
        },
        "length" => Value::from(match &v {
            Value::Null => 0,
            Value::Str(s) | Value::Safe(s) => s.encode_utf16().count(),
            Value::List(l) => l.len(),
            Value::Map(m) => m.len(),
            _ => return Err("length of a non-collection".into()),
        }),
        "default" => {
            let empty = match &v {
                Value::Null => true,
                Value::Str(s) | Value::Safe(s) => s.trim().is_empty(),
                Value::List(l) => l.is_empty(),
                Value::Map(m) => m.is_empty(),
                _ => false,
            };
            if empty {
                args.first().cloned().unwrap_or(Value::Null)
            } else {
                v
            }
        }
        "numberformat" => {
            let pattern = args.first().map(Value::render).unwrap_or_default();
            match v {
                Value::Int(i) => Value::from(escape::number_format(i as f64, true, &pattern)),
                Value::Float(f) => Value::from(escape::number_format(f, false, &pattern)),
                Value::Null => Value::Null,
                other => return Err(format!("numberformat of {other:?}")),
            }
        }
        "replace" => {
            let mut s = v.render();
            if let Some(Value::Map(pairs)) = args.first() {
                for (from, to) in pairs.iter() {
                    s = s.replace(from.as_str(), &to.render());
                }
            }
            Value::from(s)
        }
        "upper" => Value::from(v.render().to_uppercase()),
        "lower" => Value::from(v.render().to_lowercase()),
        "trim" => Value::from(v.render().trim().to_string()),
        "capitalize" => {
            let s = v.render();
            let mut c = s.chars();
            match c.next() {
                Some(f) => Value::from(f.to_uppercase().collect::<String>() + c.as_str()),
                None => Value::from(s),
            }
        }
        "abs" => match v {
            Value::Int(i) => Value::Int(i.abs()),
            Value::Float(f) => Value::Float(f.abs()),
            other => return Err(format!("abs of {other:?}")),
        },
        "join" => {
            let sep = args.first().map(Value::render).unwrap_or_default();
            match v {
                Value::List(l) => {
                    Value::from(l.iter().map(Value::render).collect::<Vec<_>>().join(&sep))
                }
                Value::Null => Value::Null,
                other => other,
            }
        }
        "first" => match v {
            Value::List(l) => l.first().cloned().unwrap_or(Value::Null),
            Value::Str(s) | Value::Safe(s) => {
                Value::from(s.chars().next().map(String::from).unwrap_or_default())
            }
            _ => Value::Null,
        },
        "last" => match v {
            Value::List(l) => l.last().cloned().unwrap_or(Value::Null),
            Value::Str(s) | Value::Safe(s) => {
                Value::from(s.chars().last().map(String::from).unwrap_or_default())
            }
            _ => Value::Null,
        },
        other => return Err(format!("unknown filter {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_includes() {
        assert_eq!(
            resolve("groups/page.tpl", "../base/header.tpl"),
            "base/header.tpl"
        );
        assert_eq!(resolve("a/b/c.tpl", "./d.tpl"), "a/b/d.tpl");
        assert_eq!(resolve("a/b/c.tpl", "base/x.tpl"), "base/x.tpl");
    }

    #[test]
    fn bean_getters() {
        assert_eq!(property_of("getName").as_deref(), Some("name"));
        assert_eq!(property_of("isHasAlert").as_deref(), Some("hasAlert"));
        assert_eq!(property_of("generateClickLink"), None);
        assert_eq!(property_of("getID").as_deref(), Some("iD"));
    }
}
