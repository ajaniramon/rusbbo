//! Lexer and parser: template source to [`Node`]s.

use std::sync::Arc;

use crate::value::Value;

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    Print(Expr),
    If {
        branches: Vec<(Expr, Vec<Node>)>,
        otherwise: Vec<Node>,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Node>,
        otherwise: Vec<Node>,
    },
    Set {
        name: String,
        value: Expr,
    },
    Include {
        path: Expr,
        with: Option<Expr>,
    },
    /// `None`: escaping off; `Some(strategy)`.
    Autoescape {
        strategy: Option<String>,
        body: Vec<Node>,
    },
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Value),
    /// A string literal: never escaped by `autoescape`.
    StrLit(Arc<str>),
    Interp(Vec<Expr>),
    Var(String),
    Attr(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, String, Vec<Expr>),
    Function(String, Vec<Expr>),
    Filter(Box<Expr>, String, Vec<Expr>),
    Test {
        value: Box<Expr>,
        name: String,
        negated: bool,
        args: Vec<Expr>,
    },
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    List(Vec<Expr>),
    MapLit(Vec<(Expr, Expr)>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Or,
    And,
    Contains,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
    Range,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Int(i64),
    Float(f64),
    /// Single-quoted (escapes resolved).
    Str(String),
    /// Double-quoted: raw text, may contain `#{...}`.
    DStr(String),
    Op(&'static str),
    Punct(char),
}

/// A top-level chunk of template source.
enum Chunk {
    Text(String),
    Print(Vec<Tok>, usize),
    Tag(Vec<Tok>, usize),
}

pub fn parse(src: &str) -> Result<Vec<Node>, String> {
    let chunks = chunk(src)?;
    let mut p = Parser { chunks, pos: 0 };
    let (nodes, end) = p.nodes(&[])?;
    if let Some(end) = end {
        return Err(format!("unexpected tag {end}"));
    }
    Ok(nodes)
}

fn line_of(src: &str, pos: usize) -> usize {
    src[..pos].bytes().filter(|b| *b == b'\n').count() + 1
}

fn chunk(src: &str) -> Result<Vec<Chunk>, String> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        let rest = &src[i..];
        let open = ["{{", "{%", "{#"].iter().filter_map(|d| rest.find(d)).min();
        let Some(off) = open else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..off]);
        i += off;
        let kind = &src[i..i + 2];
        let mut start = i + 2;
        if src[start..].starts_with('-') {
            start += 1;
            let trimmed = text.trim_end().len();
            text.truncate(trimmed);
        }
        let close = match kind {
            "{{" => "}}",
            "{%" => "%}",
            _ => "#}",
        };
        let line = line_of(src, i);
        if kind == "{#" {
            let end = src[start..]
                .find(close)
                .ok_or_else(|| format!("line {line}: unclosed comment"))?;
            i = skip_newline(src, start + end + 2);
            continue;
        }
        let (toks, end, dash) = lex(src, start, close).map_err(|e| format!("line {line}: {e}"))?;
        if !text.is_empty() {
            out.push(Chunk::Text(std::mem::take(&mut text)));
        }
        out.push(if kind == "{{" {
            Chunk::Print(toks, line)
        } else {
            Chunk::Tag(toks, line)
        });
        let after = end + close.len();
        i = if dash {
            after + (src[after..].len() - src[after..].trim_start().len())
        } else {
            skip_newline(src, after)
        };
    }
    if !text.is_empty() {
        out.push(Chunk::Text(text));
    }
    Ok(out)
}

/// Pebble's `newLineTrimming`: a newline right after a closing delimiter is dropped.
fn skip_newline(src: &str, at: usize) -> usize {
    let rest = &src[at..];
    if rest.starts_with("\r\n") {
        at + 2
    } else if rest.starts_with('\n') {
        at + 1
    } else {
        at
    }
}

const OPS: [&str; 17] = [
    "==", "!=", "<=", ">=", "..", "<", ">", "+", "-", "*", "/", "%", "~", "?", ":", "=", "|",
];

/// Lexes code from `start` up to the closing delimiter; returns the tokens, the delimiter's
/// position and whether a `-` (trim the whitespace after) preceded it.
fn lex(src: &str, start: usize, close: &str) -> Result<(Vec<Tok>, usize, bool), String> {
    let mut toks = Vec::new();
    let b = src.as_bytes();
    let mut i = start;
    loop {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Err(format!("missing {close}"));
        }
        if src[i..].starts_with(close) {
            return Ok((toks, i, false));
        }
        if src[i..].starts_with('-') && src[i + 1..].starts_with(close) {
            return Ok((toks, i + 1, true));
        }
        let c = b[i] as char;
        if c.is_ascii_alphabetic() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            toks.push(Tok::Name(src[s..i].to_string()));
        } else if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                toks.push(Tok::Float(src[s..i].parse().map_err(|_| "bad number")?));
            } else {
                toks.push(Tok::Int(src[s..i].parse().map_err(|_| "bad number")?));
            }
        } else if c == '\'' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(ch) = src[i..].chars().next() else {
                    return Err("unclosed string".into());
                };
                if ch == '\\' {
                    let next = src[i + 1..].chars().next().ok_or("unclosed string")?;
                    s.push(next);
                    i += 1 + next.len_utf8();
                } else if ch == '\'' {
                    i += 1;
                    break;
                } else {
                    s.push(ch);
                    i += ch.len_utf8();
                }
            }
            toks.push(Tok::Str(s));
        } else if c == '"' {
            i += 1;
            let s = i;
            loop {
                if i >= b.len() {
                    return Err("unclosed string".into());
                }
                if b[i] == b'\\' {
                    i += 2;
                } else if b[i] == b'"' {
                    break;
                } else if b[i] == b'#' && b.get(i + 1) == Some(&b'{') {
                    let end = src[i..].find('}').ok_or("unclosed interpolation")?;
                    i += end + 1;
                } else {
                    i += 1;
                }
            }
            toks.push(Tok::DStr(src[s..i].to_string()));
            i += 1;
        } else if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(**op)) {
            toks.push(Tok::Op(op));
            i += op.len();
        } else if "()[]{},.".contains(c) {
            toks.push(Tok::Punct(c));
            i += 1;
        } else {
            return Err(format!("unexpected character {c:?}"));
        }
    }
}

struct Parser {
    chunks: Vec<Chunk>,
    pos: usize,
}

impl Parser {
    /// Parses nodes until one of the `ends` tags; returns that tag's name (its tokens stay
    /// readable via `self.end_toks`).
    fn nodes(&mut self, ends: &[&str]) -> Result<(Vec<Node>, Option<String>), String> {
        let mut nodes = Vec::new();
        while self.pos < self.chunks.len() {
            let idx = self.pos;
            self.pos += 1;
            match &self.chunks[idx] {
                Chunk::Text(t) => nodes.push(Node::Text(t.clone())),
                Chunk::Print(toks, line) => {
                    let mut e = Exprs {
                        toks: toks.clone(),
                        pos: 0,
                    };
                    let expr = e.expr().map_err(|err| format!("line {line}: {err}"))?;
                    e.done().map_err(|err| format!("line {line}: {err}"))?;
                    nodes.push(Node::Print(expr));
                }
                Chunk::Tag(toks, line) => {
                    let line = *line;
                    let toks = toks.clone();
                    let Some(Tok::Name(name)) = toks.first().cloned() else {
                        return Err(format!("line {line}: tag without a name"));
                    };
                    if ends.contains(&name.as_str()) {
                        self.pos = idx; // the caller reads the end tag
                        return Ok((nodes, Some(name)));
                    }
                    let node = self.tag(&name, toks, line).map_err(|err| {
                        if err.starts_with("line ") {
                            err
                        } else {
                            format!("line {line}: {err}")
                        }
                    })?;
                    nodes.push(node);
                }
            }
        }
        if ends.is_empty() {
            Ok((nodes, None))
        } else {
            Err(format!("missing {{% {} %}}", ends.join(" / ")))
        }
    }

    /// The tokens of the end tag `nodes` stopped at, consuming it.
    fn take_end(&mut self) -> Vec<Tok> {
        let idx = self.pos;
        self.pos += 1;
        match &self.chunks[idx] {
            Chunk::Tag(t, _) => t.clone(),
            _ => Vec::new(),
        }
    }

    fn tag(&mut self, name: &str, toks: Vec<Tok>, _line: usize) -> Result<Node, String> {
        let mut e = Exprs { toks, pos: 1 };
        match name {
            "if" => {
                let cond = e.expr()?;
                e.done()?;
                let mut branches = Vec::new();
                let mut cond = Some(cond);
                let otherwise;
                loop {
                    let (body, end) = self.nodes(&["elseif", "else", "endif"])?;
                    let end_toks = self.take_end();
                    branches.push((cond.take().unwrap(), body));
                    match end.as_deref() {
                        Some("elseif") => {
                            let mut e = Exprs {
                                toks: end_toks,
                                pos: 1,
                            };
                            cond = Some(e.expr()?);
                            e.done()?;
                        }
                        Some("else") => {
                            let (body, _) = self.nodes(&["endif"])?;
                            self.take_end();
                            otherwise = body;
                            break;
                        }
                        _ => {
                            otherwise = Vec::new();
                            break;
                        }
                    }
                }
                Ok(Node::If {
                    branches,
                    otherwise,
                })
            }
            "for" => {
                let var = e.name()?;
                if e.next() != Some(Tok::Name("in".into())) {
                    return Err("expected `in`".into());
                }
                let iter = e.expr()?;
                e.done()?;
                let (body, end) = self.nodes(&["else", "endfor"])?;
                self.take_end();
                let otherwise = if end.as_deref() == Some("else") {
                    let (b, _) = self.nodes(&["endfor"])?;
                    self.take_end();
                    b
                } else {
                    Vec::new()
                };
                Ok(Node::For {
                    var,
                    iter,
                    body,
                    otherwise,
                })
            }
            "set" => {
                let name = e.name()?;
                if e.next() != Some(Tok::Op("=")) {
                    return Err("expected `=`".into());
                }
                let value = e.expr()?;
                e.done()?;
                Ok(Node::Set { name, value })
            }
            "include" => {
                let path = e.expr()?;
                let with = if e.peek() == Some(&Tok::Name("with".into())) {
                    e.pos += 1;
                    Some(e.expr()?)
                } else {
                    None
                };
                e.done()?;
                Ok(Node::Include { path, with })
            }
            "autoescape" => {
                let strategy = match e.next() {
                    None => Some("html".to_string()),
                    Some(Tok::Str(s)) | Some(Tok::DStr(s)) => Some(s),
                    Some(Tok::Name(n)) if n == "false" => None,
                    Some(Tok::Name(n)) if n == "true" => Some("html".to_string()),
                    Some(t) => return Err(format!("bad autoescape argument {t:?}")),
                };
                e.done()?;
                let (body, _) = self.nodes(&["endautoescape"])?;
                self.take_end();
                Ok(Node::Autoescape { strategy, body })
            }
            other => Err(format!("unknown tag {other:?}")),
        }
    }
}

struct Exprs {
    toks: Vec<Tok>,
    pos: usize,
}

fn binop(t: &Tok) -> Option<(u8, BinOp)> {
    Some(match t {
        Tok::Name(n) => match n.as_str() {
            "or" => (10, BinOp::Or),
            "and" => (15, BinOp::And),
            "contains" => (20, BinOp::Contains),
            "equals" => (30, BinOp::Eq),
            _ => return None,
        },
        Tok::Op(op) => match *op {
            "==" => (30, BinOp::Eq),
            "!=" => (30, BinOp::Ne),
            "<" => (30, BinOp::Lt),
            ">" => (30, BinOp::Gt),
            "<=" => (30, BinOp::Le),
            ">=" => (30, BinOp::Ge),
            "+" => (40, BinOp::Add),
            "-" => (40, BinOp::Sub),
            "*" => (60, BinOp::Mul),
            "/" => (60, BinOp::Div),
            "%" => (60, BinOp::Mod),
            "~" => (110, BinOp::Concat),
            ".." => (120, BinOp::Range),
            _ => return None,
        },
        _ => return None,
    })
}

const IS_PREC: u8 = 20;
const FILTER_PREC: u8 = 100;

impl Exprs {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn done(&self) -> Result<(), String> {
        match self.peek() {
            None => Ok(()),
            Some(t) => Err(format!("unexpected token {t:?}")),
        }
    }

    fn name(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Name(n)) => Ok(n),
            t => Err(format!("expected a name, found {t:?}")),
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        match self.next() {
            Some(Tok::Punct(p)) if p == c => Ok(()),
            t => Err(format!("expected {c:?}, found {t:?}")),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let cond = self.binary(0)?;
        if self.peek() == Some(&Tok::Op("?")) {
            self.pos += 1;
            let a = self.expr()?;
            if self.next() != Some(Tok::Op(":")) {
                return Err("expected `:` in a ternary".into());
            }
            let b = self.expr()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn binary(&mut self, min: u8) -> Result<Expr, String> {
        let mut left = self.unary()?;
        while let Some(tok) = self.peek().cloned() {
            if tok == Tok::Name("is".into()) {
                if IS_PREC < min {
                    break;
                }
                self.pos += 1;
                let negated = if self.peek() == Some(&Tok::Name("not".into())) {
                    self.pos += 1;
                    true
                } else {
                    false
                };
                let name = self.name()?;
                let args = if self.peek() == Some(&Tok::Punct('(')) {
                    self.args()?
                } else {
                    Vec::new()
                };
                left = Expr::Test {
                    value: Box::new(left),
                    name,
                    negated,
                    args,
                };
                continue;
            }
            if tok == Tok::Op("|") {
                if FILTER_PREC < min {
                    break;
                }
                self.pos += 1;
                let name = self.name()?;
                let args = if self.peek() == Some(&Tok::Punct('(')) {
                    self.args()?
                } else {
                    Vec::new()
                };
                left = Expr::Filter(Box::new(left), name, args);
                continue;
            }
            let Some((prec, op)) = binop(&tok) else { break };
            if prec < min {
                break;
            }
            self.pos += 1;
            let right = self.binary(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Name(n)) if n == "not" => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Tok::Op("-")) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.unary()
            }
            _ => {
                let p = self.primary()?;
                self.postfix(p)
            }
        }
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect('(')?;
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::Punct(')')) {
            self.pos += 1;
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            match self.next() {
                Some(Tok::Punct(',')) => continue,
                Some(Tok::Punct(')')) => return Ok(args),
                t => return Err(format!("expected `,` or `)`, found {t:?}")),
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Int(i)) => Ok(Expr::Lit(Value::Int(i))),
            Some(Tok::Float(f)) => Ok(Expr::Lit(Value::Float(f))),
            Some(Tok::Str(s)) => Ok(Expr::StrLit(Arc::from(s))),
            Some(Tok::DStr(s)) => interpolate(&s),
            Some(Tok::Name(n)) => match n.as_str() {
                "true" => Ok(Expr::Lit(Value::Bool(true))),
                "false" => Ok(Expr::Lit(Value::Bool(false))),
                "null" | "none" => Ok(Expr::Lit(Value::Null)),
                _ if self.peek() == Some(&Tok::Punct('(')) => Ok(Expr::Function(n, self.args()?)),
                _ => Ok(Expr::Var(n)),
            },
            Some(Tok::Punct('(')) => {
                let e = self.expr()?;
                self.expect(')')?;
                Ok(e)
            }
            Some(Tok::Punct('[')) => {
                let mut items = Vec::new();
                if self.peek() == Some(&Tok::Punct(']')) {
                    self.pos += 1;
                    return Ok(Expr::List(items));
                }
                loop {
                    items.push(self.expr()?);
                    match self.next() {
                        Some(Tok::Punct(',')) => continue,
                        Some(Tok::Punct(']')) => return Ok(Expr::List(items)),
                        t => return Err(format!("expected `,` or `]`, found {t:?}")),
                    }
                }
            }
            Some(Tok::Punct('{')) => {
                let mut items = Vec::new();
                if self.peek() == Some(&Tok::Punct('}')) {
                    self.pos += 1;
                    return Ok(Expr::MapLit(items));
                }
                loop {
                    let k = self.expr()?;
                    if self.next() != Some(Tok::Op(":")) {
                        return Err("expected `:` in a map".into());
                    }
                    let v = self.expr()?;
                    items.push((k, v));
                    match self.next() {
                        Some(Tok::Punct(',')) => continue,
                        Some(Tok::Punct('}')) => return Ok(Expr::MapLit(items)),
                        t => return Err(format!("expected `,` or `}}`, found {t:?}")),
                    }
                }
            }
            t => Err(format!("unexpected token {t:?}")),
        }
    }

    fn postfix(&mut self, mut e: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                Some(Tok::Punct('.')) => {
                    self.pos += 1;
                    let name = match self.next() {
                        Some(Tok::Name(n)) => n,
                        Some(Tok::Int(i)) => i.to_string(),
                        t => return Err(format!("expected an attribute, found {t:?}")),
                    };
                    if self.peek() == Some(&Tok::Punct('(')) {
                        let args = self.args()?;
                        e = Expr::Call(Box::new(e), name, args);
                    } else {
                        e = Expr::Attr(Box::new(e), name);
                    }
                }
                Some(Tok::Punct('[')) => {
                    self.pos += 1;
                    let idx = self.expr()?;
                    self.expect(']')?;
                    e = Expr::Index(Box::new(e), Box::new(idx));
                }
                _ => return Ok(e),
            }
        }
    }
}

/// A double-quoted string: `#{expr}` interpolation, backslashes kept as written (Pebble 3.1).
fn interpolate(raw: &str) -> Result<Expr, String> {
    if !raw.contains("#{") {
        // A plain string: `\x` is `x` (Pebble keeps backslashes only in interpolated strings).
        let mut s = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    s.push(n);
                }
            } else {
                s.push(c);
            }
        }
        return Ok(Expr::StrLit(Arc::from(s)));
    }
    let mut parts = Vec::new();
    let mut rest = raw;
    while let Some(i) = rest.find("#{") {
        if i > 0 {
            parts.push(Expr::StrLit(Arc::from(&rest[..i])));
        }
        let end = rest[i..].find('}').ok_or("unclosed interpolation")? + i;
        let code = format!("{}%}}", &rest[i + 2..end]);
        let (toks, _, _) = lex(&code, 0, "%}")?;
        let mut e = Exprs { toks, pos: 0 };
        parts.push(e.expr()?);
        e.done()?;
        rest = &rest[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(Expr::StrLit(Arc::from(rest)));
    }
    Ok(Expr::Interp(parts))
}
