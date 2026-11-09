//! Embeds rusbbo's own templates (`templates/housekeeping/**`: the housekeeping with its texts as
//! locale keys, see `web::i18n`) so the binary needs no copy of them on disk.

use std::fmt::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "tpl") {
            out.push(path);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("templates");
    println!("cargo:rerun-if-changed=templates");
    let mut files = Vec::new();
    walk(&root.join("housekeeping"), &mut files);
    files.sort();
    let mut code = String::from("pub static HK_TEMPLATES: &[(&str, &[u8])] = &[\n");
    for f in &files {
        let name = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            code,
            "    ({name:?}, include_bytes!({:?})),",
            f.to_string_lossy()
        )
        .unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("hk_templates.rs");
    std::fs::write(out, code).unwrap();
}
