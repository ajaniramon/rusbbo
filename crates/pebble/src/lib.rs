//! A Pebble 3.1 template engine for the subset of Pebble that Havana-Web's templates use
//! (`tools/www-tpl`): `if`/`elseif`/`else`, `for` (with `else` and `loop`), `set`, `include`
//! (with `with`), `autoescape`, comments, the `escape`/`raw`/`length`/`default`/`numberformat`/
//! `replace` filters, the `empty`/`null`/`present` tests, and bean-style attribute and method
//! access. It reproduces what Havana-Web's `PebbleEngine` (strict variables off, autoescaping
//! off, newline trimming on) prints, byte for byte: the semantics were taken from the Java engine
//! (see the tests).

mod escape;
mod render;
mod syntax;
mod value;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

pub use escape::{html as escape_html, js as escape_js, number_format};
pub use render::{equals, truthy};
pub use syntax::Node;
pub use value::{Context, Func, Map, Value, java_double};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("template {name}: {message}")]
    Template { name: String, message: String },
}

/// Loads templates from a directory (Havana's `template.directory/template.name`), caching the
/// parsed form.
pub struct Engine {
    root: PathBuf,
    cache: RwLock<HashMap<String, Arc<Vec<Node>>>>,
    /// Re-read templates on every render (for template development).
    reload: bool,
}

impl Engine {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache: RwLock::new(HashMap::new()),
            reload: false,
        }
    }

    pub fn with_reload(mut self, reload: bool) -> Self {
        self.reload = reload;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether `name` (e.g. `"index.tpl"`) exists.
    pub fn exists(&self, name: &str) -> bool {
        self.root.join(name).is_file()
    }

    pub(crate) fn load(&self, name: &str) -> Result<Arc<Vec<Node>>, String> {
        if !self.reload
            && let Some(t) = self.cache.read().get(name)
        {
            return Ok(t.clone());
        }
        if name.split('/').any(|seg| seg == "..") {
            return Err(format!(
                "template path {name} leaves the template directory"
            ));
        }
        let path = self.root.join(name);
        // Havana-Web's loader reads templates as ISO-8859-1 (`StringUtil.getCharset()`).
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let src = latin1(&bytes);
        let nodes = Arc::new(syntax::parse(&src).map_err(|e| format!("{name}: {e}"))?);
        self.cache.write().insert(name.to_string(), nodes.clone());
        Ok(nodes)
    }

    /// Renders the template file `name` (relative to the root, with its `.tpl`).
    pub fn render(&self, name: &str, ctx: Context) -> Result<String, Error> {
        let err = |message: String| Error::Template {
            name: name.to_string(),
            message,
        };
        let nodes = self.load(name).map_err(err)?;
        self.render_nodes(name, &nodes, ctx).map_err(err)
    }

    /// Renders template source given inline (tests, e-mails).
    pub fn render_str(&self, src: &str, ctx: Context) -> Result<String, Error> {
        let err = |message: String| Error::Template {
            name: "<inline>".into(),
            message,
        };
        let nodes = syntax::parse(src).map_err(err)?;
        self.render_nodes("inline.tpl", &nodes, ctx).map_err(err)
    }

    fn render_nodes(&self, name: &str, nodes: &[Node], ctx: Context) -> Result<String, String> {
        let mut r = render::Renderer {
            engine: self,
            scopes: vec![ctx],
            path: name.to_string(),
            depth: 0,
            escape: None,
        };
        let mut out = String::with_capacity(16 * 1024);
        r.nodes(nodes, &mut out)?;
        Ok(out)
    }
}

/// ISO-8859-1 bytes to text.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bean() -> Value {
        Map::new()
            .with("name", "Al<b>'\"&")
            .with("active", true)
            .with("id", 7)
            .with("big", 12345678i64)
            .with("ratio", 1.5)
            .with("nul", Value::Null)
            .with_fn("can", |a| {
                Ok(Value::Bool(a.first().and_then(Value::as_str) == Some("x")))
            })
            .into()
    }

    fn ctx() -> Context {
        let mut c = Context::new();
        c.insert("b".into(), bean());
        c.insert("s".into(), "abc".into());
        c.insert("es".into(), "".into());
        c.insert("z".into(), 0.into());
        c.insert("n".into(), 3.into());
        c.insert("l".into(), Value::list([1, 2, 3]));
        c.insert("el".into(), Value::list(Vec::<Value>::new()));
        c.insert("m".into(), Map::new().with("k", "v").into());
        c
    }

    fn r(src: &str) -> String {
        Engine::new(".")
            .render_str(src, ctx())
            .unwrap_or_else(|e| format!("ERR {e}"))
    }

    /// Each pair is a template and what Pebble 3.1.5 (as Havana-Web configures it) printed.
    #[test]
    fn matches_pebble() {
        let cases = [
            ("A{% if true %}\nB\n{% endif %}\nC", "AB\nC"),
            ("{{ s }}\nX\n{{ s }}", "abcX\nabc"),
            (
                "X\n{# c #}\nY|{{ s }}  \nW|{% if true %} \nV{% endif %}",
                "X\nY|abc  \nW| \nV",
            ),
            (
                "  {% set x = 5 %}  \n{{ x }}\n{# c #}\nZ\n{%- if true -%}  T  {%- endif -%}  \nQ",
                "    \n5ZTQ",
            ),
            (
                "[{{ b.name }}][{{ b.getName() }}][{{ b.name|escape }}][{{ b.name|escape('js') }}][{{ b.active }}][{{ b.id }}][{{ b.ratio }}][{{ b.big }}][{{ b.nul }}][{{ missing }}]",
                "[Al<b>'\"&][Al<b>'\"&][Al&lt;b&gt;&#39;&quot;&amp;][Al<b>\\'\\\"\\x26][true][7][1.5][12345678][][]",
            ),
            ("{% if s %}T{% else %}F{% endif %}", "T"),
            ("{% if es %}T{% else %}F{% endif %}", "F"),
            ("{% if z %}T{% else %}F{% endif %}", "F"),
            ("{% if n %}T{% else %}F{% endif %}", "T"),
            ("{% if missing %}T{% else %}F{% endif %}", "F"),
            ("{% if not missing %}T{% else %}F{% endif %}", "T"),
            (
                "{% if b.can('x') %}yes{% endif %}{% if b.id == 7 %}=7{% endif %}{% if b.id == '7' %}='7'{% endif %}{% if s == 'abc' %}=abc{% endif %}",
                "yes=7=abc",
            ),
            (
                "{% for x in l %}{{ loop.index }}:{{ x }}{% if loop.last %}L{% endif %},{% else %}empty{% endfor %}|{% for x in el %}x{% else %}empty{% endfor %}",
                "0:1,1:2,2:3L,|empty",
            ),
            (
                "{% if el is empty %}EE{% endif %}{% if es is empty %}ES{% endif %}{% if missing is empty %}MS{% endif %}{% if l is empty %}L{% endif %}",
                "EEESMS",
            ),
            (
                "{{ 1000000|numberformat('#,###') }}|{{ 1234.5|numberformat('#,###.00') }}",
                "1,000,000|1,234.50",
            ),
            (
                "{{ l|length }}|{{ s|length }}|{{ missing|length }}",
                "3|3|0",
            ),
            (
                "{{ missing|default('d') }}|{{ es|default('d') }}|{{ z|default('d') }}",
                "d|d|0",
            ),
            ("{{ 'a' ~ 1 ~ true ~ missing }}", "a1true"),
            (
                "{{ 7 / 2 }}|{{ 7 * 2 }}|{{ 7 - 2.5 }}|{{ 7 % 3 }}|{{ n + 1 }}",
                "3|14|4.5|1|4",
            ),
            ("{{ 2 * 3.0 }}|{{ 10 / 4 }}|{{ 10.0 / 4 }}", "6.0|2|2.5"),
            ("{{ b.big + 1 }}|{{ b.ratio * 2 }}", "12345679|3.0"),
            (
                "{% set x = l|length %}{{ x }}|{% set y = 'q' %}{{ y }}",
                "3|q",
            ),
            (
                "{% for i in 0..3 %}{{i}}{% endfor %}|{% for i in range(1,3) %}{{i}}{% endfor %}",
                "0123|123",
            ),
            (
                "{% for x in l %}{{ loop.index }}{{ loop.revindex }}{{ loop.length }}{{ loop.first }}{% endfor %}",
                "023true113false203false",
            ),
            (
                "{{ b.name|escape|length }}|{{ es == '' }}|{{ missing == null }}|{{ missing is null }}|{{ n == 3.0 }}|{{ n >= 3 }}",
                "27|true|true|true|true|true",
            ),
            (
                "{{ \"dq \\\"x\\\" #{s}\" }}|{{ 'sq \\'y\\'' }}",
                "dq \\\"x\\\" abc|sq 'y'",
            ),
            (
                "{% if n == 3 %}a{% elseif n == 4 %}b{% else %}c{% endif %}",
                "a",
            ),
            (
                "{% set c = \" class=\\\"copyright\\\"\" %}[{{ c }}]",
                "[ class=\"copyright\"]",
            ),
            (
                "{% if b.active %}A{% endif %}|{% if b.isActive() %}B{% endif %}|{{ b.getId() + 1 }}",
                "A|B|8",
            ),
            ("{{ missing.foo }}|{{ b.nothing }}|{{ b.nul.foo }}", "||"),
            ("{{ not false and false }}", "false"),
            ("{{ not true or true }}", "true"),
            ("{{ 'a' ~ 1 + 2 }}", "a12"),
            ("{{ 1 + 2 ~ 'a' }}", "12a"),
            ("{{ 'ab' ~ s|upper }}", "ABABC"),
            ("{{ 'x' ~ s|length }}", "4"),
            ("{{ s|length + 1 }}", "4"),
            ("{{ -n|abs }}", "3"),
            ("{{ 1 + 2 * 3 }}", "7"),
            ("{{ 2 > 1 == true }}", "true"),
            ("{{ true ? 'a' : false ? 'b' : 'c' }}", "a"),
            ("{{ n > 2 ? 'big' : 'small' }}", "big"),
            ("{% if s == 'abc' and not (n == 4) %}OK{% endif %}", "OK"),
            ("{{ s is empty ? 'e' : 'ne' }}", "ne"),
            ("{{ n is not null and n > 1 }}", "true"),
            ("{{ 'abc' contains 'b' and true }}", "true"),
            ("{{ 7 % 3 * 2 }}", "2"),
            ("{{ 1..3 }}", "[1, 2, 3]"),
            ("{{ [1, 'a', true] }}", "[1, a, true]"),
            ("{{ {'a': 1} }}", "{a=1}"),
            ("{{ m }}", "{k=v}"),
            (
                "{{ 10 / 3.0 }}|{{ -7 / 2 }}|{{ -7 % 3 }}",
                "3.3333333333333335|-3|-1",
            ),
            ("{{ s[0] }}|{{ l[5] }}|{{ m['k'] }}", "||v"),
            (
                "{{ \"a\" == \"a\" }}|{{ 1 == true }}|{{ null == false }}",
                "true|false|false",
            ),
            ("{{ n == '3' }}|{{ z == false }}", "false|false"),
            ("{{ not n == 4 }}", "false"),
            ("{{ not s is empty }}", "false"),
            ("{{ not false ~ 'x' }}", "truex"),
            (
                "{{ 'a' + 'b' }}|{{ 'a' + 1 }}|{{ 1 + 'a' }}|{{ s + missing }}",
                "ab|a1|1a|abcnull",
            ),
            (
                "{{ b.id > 5 ? b.name|escape : '-' }}",
                "Al&lt;b&gt;&#39;&quot;&amp;",
            ),
            ("{{ s|escape ~ '<' }}", "abc<"),
            (
                "{% autoescape 'html' %}{{ '<lit>' }}|{{ s ~ '<' }}|{{ b.name|escape('js') }}|{{ '<' ~ b.name|raw }}|{% set q = '<q>' %}{{ q }}|{{ b.name|default('x') }}{% endautoescape %}",
                "<lit>|abc&lt;|Al<b>\\'\\\"\\x26|<Al<b>'\"&|&lt;q&gt;|Al&lt;b&gt;&#39;&quot;&amp;",
            ),
            (
                "{% autoescape %}{{ b.name }}{% endautoescape %}",
                "Al&lt;b&gt;&#39;&quot;&amp;",
            ),
            (
                "{% for x in m %}{{ x.key }}={{ x.value }};{% endfor %}",
                "k=v;",
            ),
            ("{% for x in missing %}a{% else %}none{% endfor %}", ""),
            (
                "{% for x in l %}{% set inner = x %}{% endfor %}[{{ inner }}]",
                "[]",
            ),
            (
                "{% set a = 1 %}{% for i in [1,2] %}{% set a = a + i %}{% endfor %}{{ a }}",
                "4",
            ),
            ("{% if true %}{% set d = 4 %}{% endif %}{{ d }}", "4"),
            (
                "{% if 's' is present %}P{% endif %}{% if 'zz' is present %}Z{% endif %}{% if 'zz' is not present %}N{% endif %}",
                "PN",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(r(src), want, "template: {src}");
        }
    }

    #[test]
    fn errors_like_pebble() {
        for src in [
            "{% if l %}T{% endif %}",
            "{{ missing + 1 }}",
            "{{ not 1 > 2 }}",
            "{% if x %}",
            "{{ 1.0e10 }}",
            "{% for k, v in m %}{% endfor %}",
        ] {
            assert!(
                r(src).starts_with("ERR"),
                "{src} should fail, got {}",
                r(src)
            );
        }
    }

    /// Every Havana-Web template parses (when the Java checkout is around: `HAVANA_DIR`).
    #[test]
    fn parses_havana_templates() {
        let root = std::env::var("HAVANA_DIR").unwrap_or_else(|_| "../../../aeyra-havana".into());
        let dir = Path::new(&root).join("tools/www-tpl/default");
        if !dir.is_dir() {
            eprintln!("skipped: no {}", dir.display());
            return;
        }
        let mut stack = vec![dir.clone()];
        let mut failures = Vec::new();
        let mut count = 0;
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "tpl") {
                    count += 1;
                    let src = latin1(&std::fs::read(&p).unwrap());
                    if let Err(e) = syntax::parse(&src) {
                        failures.push(format!("{}: {e}", p.strip_prefix(&dir).unwrap().display()));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{count} templates, failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn includes_share_scopes_like_pebble() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(
            dir.path().join("main.tpl"),
            "{% set a = 1 %}{% for i in [1,2] %}{% set a = a + i %}{% set b = i %}{% endfor %}a={{ a }} b={{ b }}|{% include \"sub/inc.tpl\" %}|after c={{ c }} a={{ a }}|{% include \"sub/inc.tpl\" with {\"a\": 9} %}|a={{ a }}|{% if true %}{% set d = 4 %}{% endif %}d={{ d }}",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("sub/inc.tpl"),
            "in a={{ a }}{% set c = 3 %}{% set a = 5 %}{% include \"../leaf.tpl\" %}",
        )
        .unwrap();
        std::fs::write(dir.path().join("leaf.tpl"), "[leaf a={{ a }}]").unwrap();
        let out = Engine::new(dir.path())
            .render("main.tpl", Context::new())
            .unwrap();
        assert_eq!(
            out,
            "a=4 b=|in a=4[leaf a=5]|after c= a=5|in a=9[leaf a=5]|a=5|d=4"
        );
    }
}
