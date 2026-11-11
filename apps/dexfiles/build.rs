//! Turns `strings.tsv` (key, English, Spanish; tab-separated, any order) into the sorted table
//! `i18n.rs` includes.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=strings.tsv");
    let text = std::fs::read_to_string("strings.tsv").unwrap_or_default();
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            println!("cargo:warning=strings.tsv line {}: expected 3 tab-separated fields", n + 1);
            continue;
        }
        rows.push((parts[0].to_owned(), parts[1].to_owned(), parts[2].to_owned()));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.dedup_by(|a, b| {
        if a.0 == b.0 {
            println!("cargo:warning=strings.tsv: duplicate key {}", a.0);
        }
        a.0 == b.0
    });
    let mut out = String::from("[\n");
    for (k, en, es) in rows {
        let _ = writeln!(out, "    ({k:?}, {en:?}, {es:?}),", en = en.replace("\\n", "\n"), es = es.replace("\\n", "\n"));
    }
    out.push(']');
    let dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into());
    let _ = std::fs::write(format!("{dir}/strings.in"), out);
}
