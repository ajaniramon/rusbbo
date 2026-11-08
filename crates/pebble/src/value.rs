//! Template values: what Havana-Web's Java beans, lists and maps look like to a template.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

/// A method or lazy property: called with the template's arguments (none for a property).
pub type Func = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

/// A template value. Java beans are [`Value::Map`]s whose keys are the bean's property names
/// (`getName()`/`isActive()` become `name`/`active`); methods that take arguments, and properties
/// worth computing only when a template reads them, are [`Value::Func`]s.
#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Arc<str>),
    /// Pebble's `SafeString`: output of `escape`/`raw`, never escaped again.
    Safe(Arc<str>),
    List(Arc<Vec<Value>>),
    Map(Arc<Map>),
    Func(Func),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Safe(s) => write!(f, "safe({s:?})"),
            Value::List(l) => f.debug_list().entries(l.iter()).finish(),
            Value::Map(m) => f
                .debug_map()
                .entries(m.entries.iter().map(|(k, v)| (k, v)))
                .finish(),
            Value::Func(_) => write!(f, "<func>"),
        }
    }
}

/// An insertion-ordered map (a Java bean, `LinkedHashMap`, or map literal).
#[derive(Clone, Default, Debug)]
pub struct Map {
    entries: Vec<(String, Value)>,
}

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style insert.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.insert(key, value);
        self
    }

    /// A method taking arguments (or a lazily computed property).
    pub fn with_fn(
        mut self,
        key: impl Into<String>,
        f: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        self.insert(key, Value::Func(Arc::new(f)));
        self
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        let value = value.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn into_value(self) -> Value {
        Value::Map(Arc::new(self))
    }
}

impl Value {
    pub fn str(s: impl AsRef<str>) -> Value {
        Value::Str(Arc::from(s.as_ref()))
    }

    /// A method or lazy property (see [`Map::with_fn`]).
    pub fn func(f: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static) -> Value {
        Value::Func(Arc::new(f))
    }

    pub fn list<T: Into<Value>>(items: impl IntoIterator<Item = T>) -> Value {
        Value::List(Arc::new(items.into_iter().map(Into::into).collect()))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Safe(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub(crate) fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_))
    }

    /// Java's `String.valueOf` as Pebble prints a value (`null` prints nothing).
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write_java(&mut out, false);
        out
    }

    /// `String.valueOf(value)`, with `null` as `"null"` when `null_text` (Java string `+`).
    pub(crate) fn write_java(&self, out: &mut String, null_text: bool) {
        match self {
            Value::Null => {
                if null_text {
                    out.push_str("null");
                }
            }
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Float(f) => out.push_str(&java_double(*f)),
            Value::Str(s) | Value::Safe(s) => out.push_str(s),
            Value::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_java(out, true);
                }
                out.push(']');
            }
            Value::Map(m) => {
                out.push('{');
                for (i, (k, v)) in m.entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push('=');
                    v.write_java(out, true);
                }
                out.push('}');
            }
            Value::Func(_) => out.push_str("<func>"),
        }
    }
}

/// Java's `Double.toString`.
pub fn java_double(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let abs = d.abs();
    if (1e-3..1e7).contains(&abs) {
        let s = format!("{d}");
        if s.contains('.') { s } else { s + ".0" }
    } else {
        // Shortest round-trip digits, in Java's d.dddE[-]n form.
        let s = format!("{:e}", d);
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {$(impl From<$t> for Value {
        fn from(v: $t) -> Self { Value::Int(v as i64) }
    })*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, usize);

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Int(v as i64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v as f64)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::str(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(Arc::from(v))
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::str(v)
    }
}

impl From<Map> for Value {
    fn from(v: Map) -> Self {
        Value::Map(Arc::new(v))
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::list(v)
    }
}

/// The variables a template is rendered with.
pub type Context = HashMap<String, Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_print_like_java() {
        assert_eq!(java_double(3.0), "3.0");
        assert_eq!(java_double(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(java_double(100000000.0), "1.0E8");
        assert_eq!(java_double(1234567.0), "1234567.0");
        assert_eq!(java_double(12345678.9), "1.23456789E7");
        assert_eq!(java_double(0.001), "0.001");
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(10.0 / 3.0), "3.3333333333333335");
    }
}
