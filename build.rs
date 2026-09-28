//! Embeds the web console (`web/dist`, committed, built with `bun run build` in `web/`) into the
//! binary, so `cargo build` never needs Bun. Without it the daemon serves a page explaining how
//! to build one.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=web/dist");
    let dist = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("set by cargo")).join("web/dist");
    let mut files = Vec::new();
    collect(&dist, &mut files);
    files.sort();

    let mut out = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for path in files {
        let name = path.strip_prefix(&dist).expect("inside dist").components();
        let name: Vec<_> = name.map(|c| c.as_os_str().to_string_lossy()).collect();
        out += &format!("    ({:?}, include_bytes!({:?})),\n", name.join("/"), path.display().to_string());
    }
    out += "];\n";
    let target = Path::new(&env::var("OUT_DIR").expect("set by cargo")).join("web_assets.rs");
    fs::write(target, out).expect("writing web_assets.rs");
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
