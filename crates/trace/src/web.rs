//! Web traces: HTTP request scripts recorded against the Java Havana-Web and checked against the
//! Rust web server, response by response (status, headers, body).
//!
//! A scenario (`scenarios/web/*.toml`):
//!
//! ```toml
//! description = "log in and open /me"
//! mask = ['var andSoItBegins = \d+']   # extra regexes; matches compare as equal
//!
//! [[step]]
//! post = "/account/submit"
//! form = { username = "tracebot1", password = "tracepass" }
//!
//! [[step]]
//! get = "/me"
//! client = "b"        # another browser (cookie jar); "a" by default
//! body = "ignore"     # or "hash" (binary bodies), "image" (PNG/GIF pixels); "text" by default
//! headers = { "X-Requested-With" = "XMLHttpRequest" }
//! capture = { token = 'name="hk_token" value="([0-9a-f]+)"' }  # from this response's body
//!
//! [[step]]
//! post = "/allseeingeye/hk/bans/unban"
//! form = { hk_token = "{{token}}" }   # a captured value, in paths and form values
//! ```
//!
//! Recording runs the scenario `runs` times and fails if the (masked) responses differ, so every
//! volatile part of a page is masked explicitly.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// The session cookie, and anything else every page carries that changes per run.
const DEFAULT_MASKS: &[&str] = &[
    r"HTTPSESSID=[0-9a-f]{64}",
    r"Expires=[A-Za-z]{3}, \d\d [A-Za-z]{3} \d{4} \d\d:\d\d:\d\d GMT",
    // TagDao.getPopularTags shuffles the tag cloud: each tag is masked (their number still counts).
    r#"<li><a href="[^"]*/tag/[^"]*" class="tag"[^>]*>[^<]*</a>"#,
];

/// Response headers that are not compared.
const IGNORED_HEADERS: &[&str] = &[
    "content-length",
    "connection",
    "date",
    "transfer-encoding",
    "keep-alive",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebScenario {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mask: Vec<String>,
    #[serde(rename = "step")]
    pub steps: Vec<WebStep>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebStep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub form: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Milliseconds to wait before the request (for work the server does after answering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
    /// Values to take from this response's body (before masking): name -> regex whose first
    /// group is the value. Later steps use them as `{{name}}` in their path and form values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capture: BTreeMap<String, String>,
}

/// `{{name}}` replaced by the captured values.
fn substitute(s: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn with_vars(step: &WebStep, vars: &BTreeMap<String, String>) -> WebStep {
    let mut s = step.clone();
    s.get = s.get.map(|p| substitute(&p, vars));
    s.post = s.post.map(|p| substitute(&p, vars));
    for v in s.form.values_mut() {
        *v = substitute(v, vars);
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// The body's bytes, until `normalise` has used them.
    #[serde(skip)]
    pub raw: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebTrace {
    pub scenario: String,
    pub description: String,
    pub mask: Vec<String>,
    pub steps: Vec<WebTraceStep>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebTraceStep {
    pub request: WebStep,
    pub response: WebResponse,
}

impl WebScenario {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let s: WebScenario =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        for (i, step) in s.steps.iter().enumerate() {
            if step.get.is_some() == step.post.is_some() {
                bail!(
                    "{}: step {} needs exactly one of get/post",
                    path.display(),
                    i + 1
                );
            }
        }
        Ok(s)
    }
}

struct Masks(Vec<Regex>);

impl Masks {
    fn new(extra: &[String]) -> Result<Self> {
        let mut v = Vec::new();
        for m in DEFAULT_MASKS
            .iter()
            .map(|s| s.to_string())
            .chain(extra.iter().cloned())
        {
            v.push(Regex::new(&m).with_context(|| format!("mask {m:?}"))?);
        }
        Ok(Masks(v))
    }

    fn apply(&self, s: &str) -> String {
        let mut out = s.to_string();
        for re in &self.0 {
            out = re.replace_all(&out, "<masked>").into_owned();
        }
        out
    }
}

/// Cookie jars, one per client name.
#[derive(Default)]
struct Jars(BTreeMap<String, BTreeMap<String, String>>);

fn run_steps(target: &str, steps: &[WebStep], masks: &Masks) -> Result<Vec<WebResponse>> {
    let mut jars = Jars::default();
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if let Some(ms) = step.wait_ms {
            std::thread::sleep(Duration::from_millis(ms));
        }
        let jar = jars
            .0
            .entry(step.client.clone().unwrap_or_else(|| "a".into()))
            .or_default();
        let sent = with_vars(step, &vars);
        let r = request(target, &sent, jar).with_context(|| format!("step {}", i + 1))?;
        for (name, pattern) in &step.capture {
            let re = Regex::new(pattern).with_context(|| format!("capture {name}"))?;
            match re.captures(&r.body).and_then(|c| c.get(1)) {
                Some(m) => {
                    vars.insert(name.clone(), m.as_str().to_string());
                }
                None => bail!("step {}: nothing to capture as {name}", i + 1),
            }
        }
        out.push(normalise(r, step, masks));
    }
    Ok(out)
}

fn normalise(mut r: WebResponse, step: &WebStep, masks: &Masks) -> WebResponse {
    r.headers
        .retain(|(k, _)| !IGNORED_HEADERS.contains(&k.as_str()));
    for (_, v) in r.headers.iter_mut() {
        *v = masks.apply(v);
    }
    r.headers.sort();
    r.body = match step.body.as_deref() {
        Some("ignore") => String::new(),
        Some("hash") => format!(
            "{} bytes, fnv {:016x}",
            r.body.len(),
            fnv(r.body.as_bytes())
        ),
        Some("image") => super::image::describe(&r.raw),
        _ => masks.apply(&r.body),
    };
    r.raw.clear();
    r
}

pub(crate) fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &x| {
        (h ^ x as u64).wrapping_mul(0x100000001b3)
    })
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn request(
    target: &str,
    step: &WebStep,
    jar: &mut BTreeMap<String, String>,
) -> Result<WebResponse> {
    let (method, path) = match (&step.get, &step.post) {
        (Some(p), _) => ("GET", p.as_str()),
        (_, Some(p)) => ("POST", p.as_str()),
        _ => unreachable!(),
    };
    let body = step
        .form
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: rusbbo-trace\r\nConnection: close\r\n"
    );
    if !jar.is_empty() {
        let cookies = jar
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        req.push_str(&format!("Cookie: {cookies}\r\n"));
    }
    for (k, v) in &step.headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if method == "POST" {
        req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(&body);

    let mut s = TcpStream::connect(target).with_context(|| format!("connecting to {target}"))?;
    s.set_read_timeout(Some(Duration::from_secs(20)))?;
    s.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    let header_end;
    loop {
        let n = s.read(&mut chunk).context("reading response")?;
        if n == 0 {
            bail!("connection closed before the response headers");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            header_end = i;
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .context("status line")?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    let mut body = buf[header_end + 4..].to_vec();
    if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        while body.len() < len {
            let n = s.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(len);
    } else if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 || find(&body, b"0\r\n\r\n").is_some() {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body = dechunk(&body);
    } else if method != "HEAD" && status != 304 && status != 204 {
        let _ = s.read_to_end(&mut body);
    }
    for (k, v) in &headers {
        if k == "set-cookie"
            && let Some((pair, attrs)) = v.split_once(';').or(Some((v.as_str(), "")))
            && let Some((name, value)) = pair.split_once('=')
        {
            let expired = attrs.to_ascii_lowercase().contains("max-age=0") || value.is_empty();
            if expired {
                jar.remove(name.trim());
            } else {
                jar.insert(name.trim().to_string(), value.trim().to_string());
            }
        }
    }
    let raw = body.clone();
    let body = match String::from_utf8(body) {
        Ok(s) => s,
        Err(e) => format!(
            "<binary {} bytes, fnv {:016x}>",
            e.as_bytes().len(),
            fnv(e.as_bytes())
        ),
    };
    Ok(WebResponse {
        status,
        headers,
        body,
        raw,
    })
}

fn dechunk(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(eol) = find(&b[i..], b"\r\n") {
        let size =
            usize::from_str_radix(String::from_utf8_lossy(&b[i..i + eol]).trim(), 16).unwrap_or(0);
        i += eol + 2;
        if size == 0 || i + size > b.len() {
            break;
        }
        out.extend_from_slice(&b[i..i + size]);
        i += size + 2;
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn reset(cmd: Option<&str>) -> Result<()> {
    if let Some(cmd) = cmd {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .status()
            .context("reset command")?;
        if !status.success() {
            bail!("reset command failed: {cmd}");
        }
    }
    Ok(())
}

/// The first differing line of two texts, with a little context.
fn first_diff(want: &str, got: &str) -> String {
    let w: Vec<&str> = want.lines().collect();
    let g: Vec<&str> = got.lines().collect();
    for i in 0..w.len().max(g.len()) {
        if w.get(i) != g.get(i) {
            let ctx = |v: &[&str]| {
                v.iter()
                    .skip(i.saturating_sub(2))
                    .take(5)
                    .map(|l| format!("    {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            return format!(
                "line {}:\n  expected:\n{}\n  got:\n{}",
                i + 1,
                ctx(&w),
                ctx(&g)
            );
        }
    }
    "texts equal".into()
}

fn describe(step: &WebStep) -> String {
    match (&step.get, &step.post) {
        (Some(p), _) => format!("GET {p}"),
        (_, Some(p)) => format!("POST {p}"),
        _ => String::new(),
    }
}

/// Differences between two responses, or `None`.
fn diff(want: &WebResponse, got: &WebResponse) -> Option<String> {
    let mut out = Vec::new();
    if want.status != got.status {
        out.push(format!("status {} != {}", want.status, got.status));
    }
    if want.headers != got.headers {
        out.push(format!(
            "headers\n  expected: {:?}\n  got:      {:?}",
            want.headers, got.headers
        ));
    }
    if want.body != got.body {
        out.push(format!(
            "body ({} vs {} bytes), {}",
            want.body.len(),
            got.body.len(),
            first_diff(&want.body, &got.body)
        ));
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

pub fn record(
    path: &Path,
    target: &str,
    reset_cmd: Option<&str>,
    runs: usize,
    out_dir: &Path,
) -> Result<()> {
    let scenario = WebScenario::load(path)?;
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let masks = Masks::new(&scenario.mask)?;
    let mut first: Option<Vec<WebResponse>> = None;
    for run in 0..runs.max(1) {
        reset(reset_cmd)?;
        let got = run_steps(target, &scenario.steps, &masks)?;
        if let Some(prev) = &first {
            for (i, (a, b)) in prev.iter().zip(&got).enumerate() {
                if let Some(d) = diff(a, b) {
                    bail!(
                        "{name}: run {} differs from run 1 at step {} ({}): mask it\n{d}",
                        run + 1,
                        i + 1,
                        describe(&scenario.steps[i])
                    );
                }
            }
        } else {
            first = Some(got);
        }
    }
    let responses = first.unwrap();
    let trace = WebTrace {
        scenario: name.clone(),
        description: scenario.description.clone(),
        mask: scenario.mask.clone(),
        steps: scenario
            .steps
            .iter()
            .cloned()
            .zip(responses)
            .map(|(request, response)| WebTraceStep { request, response })
            .collect(),
    };
    std::fs::create_dir_all(out_dir)?;
    let out = out_dir.join(format!("{name}.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&trace)? + "\n")?;
    eprintln!("recorded {} ({} steps)", out.display(), trace.steps.len());
    Ok(())
}

pub fn check(path: &Path, target: &str, reset_cmd: Option<&str>) -> Result<bool> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let trace: WebTrace =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let masks = Masks::new(&trace.mask)?;
    reset(reset_cmd)?;
    let steps: Vec<WebStep> = trace.steps.iter().map(|s| s.request.clone()).collect();
    let got = run_steps(target, &steps, &masks)?;
    for (i, (want, got)) in trace.steps.iter().zip(&got).enumerate() {
        if let Some(d) = diff(&want.response, got) {
            println!(
                "FAIL {} step {} ({}):\n{d}",
                trace.scenario,
                i + 1,
                describe(&want.request)
            );
            return Ok(false);
        }
    }
    println!("ok   {} ({} steps)", trace.scenario, got.len());
    Ok(true)
}

/// Runs a scenario once and prints the responses (for writing scenarios).
pub fn probe(path: &Path, target: &str) -> Result<()> {
    let scenario = WebScenario::load(path)?;
    let masks = Masks::new(&scenario.mask)?;
    for (step, r) in scenario
        .steps
        .iter()
        .zip(run_steps(target, &scenario.steps, &masks)?)
    {
        println!("=== {} -> {} {:?}", describe(step), r.status, r.headers);
        println!("{}", r.body);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_session_cookies() {
        let m = Masks::new(&[r"\d{10}".into()]).unwrap();
        assert_eq!(
            m.apply("HTTPSESSID=3dc4bd1d0981f0c0826f47ccac351265bc920e7a0ab0cbcb7797d425da292c51; Path=/ 1234567890"),
            "<masked>; Path=/ <masked>"
        );
    }

    #[test]
    fn dechunks() {
        assert_eq!(dechunk(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), b"abcde");
    }
}
