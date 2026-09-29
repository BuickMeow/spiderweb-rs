//! `cargo xtask i18n-sync`: keep the app's English catalog in step with the
//! upstream language file.
//!
//! Upstream keeps flat dotted keys in `scripts/lang/en.json` with `{name}`
//! placeholders; the app keeps nested YAML in `crates/spiderweb-app/locales/en.yml`
//! with rust-i18n's `%{name}` placeholders. This task reports keys the UI uses
//! that are missing from the catalog, keys whose wording differs from upstream,
//! and catalog keys the UI no longer uses. With `--write` it applies upstream
//! wording to the used keys (port-only keys are preserved).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const LOCALES: &str = "crates/spiderweb-app/locales/en.yml";
const APPSRC: &str = "crates/spiderweb-app/src";

pub fn run(args: &[String]) -> i32 {
    let mut upstream: Option<PathBuf> = None;
    let mut repo = PathBuf::from(".");
    let mut write = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--upstream" => {
                i += 1;
                match args.get(i) {
                    Some(v) => upstream = Some(PathBuf::from(v)),
                    None => {
                        eprintln!("--upstream needs a path");
                        return 1;
                    }
                }
            }
            "--repo" => {
                i += 1;
                match args.get(i) {
                    Some(v) => repo = PathBuf::from(v),
                    None => {
                        eprintln!("--repo needs a path");
                        return 1;
                    }
                }
            }
            "--write" => write = true,
            other => {
                eprintln!("unknown argument: {other}");
                return 1;
            }
        }
        i += 1;
    }
    let Some(upstream) = upstream else {
        eprintln!("i18n-sync needs --upstream <dir-or-en.json>");
        return 1;
    };
    let upstream = if upstream.is_dir() {
        upstream.join("lang/en.json")
    } else {
        upstream
    };
    match sync(&repo, &upstream, write) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("i18n-sync: {e}");
            1
        }
    }
}

fn sync(repo: &Path, upstream_path: &Path, write: bool) -> Result<i32, String> {
    let upstream_text = fs::read_to_string(upstream_path)
        .map_err(|e| format!("cannot read {}: {e}", upstream_path.display()))?;
    let raw: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&upstream_text).map_err(|e| format!("bad upstream JSON: {e}"))?;
    // Help topics are arrays of lines (see upstream files/lang.py); the app keeps
    // those in help_texts.rs, so only string entries are compared here.
    let skipped = raw.values().filter(|v| !v.is_string()).count();
    let upstream: BTreeMap<String, String> = raw
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .collect();

    let locales_path = repo.join(LOCALES);
    let locales_text = fs::read_to_string(&locales_path)
        .map_err(|e| format!("cannot read {}: {e}", locales_path.display()))?;
    let mut catalog = parse_yml(&locales_text)?;

    let used = scan_used(&repo.join(APPSRC))?;

    let mut missing_everywhere = Vec::new();
    let mut add = Vec::new();
    let mut diff = Vec::new();
    for key in &used {
        match (catalog.get(key), upstream.get(key)) {
            (None, None) => missing_everywhere.push(key.clone()),
            (None, Some(up)) => add.push((key.clone(), placeholder(up))),
            (Some(ours), Some(up)) => {
                let want = placeholder(up);
                if strip_percent(ours) != strip_percent(&want) {
                    diff.push((key.clone(), ours.clone(), want));
                }
            }
            (Some(_), None) => {}
        }
    }
    let unused: Vec<&String> = catalog.keys().filter(|k| !used.contains(*k)).collect();
    let port_only: Vec<&String> = catalog
        .keys()
        .filter(|k| used.contains(*k) && !upstream.contains_key(*k))
        .collect();

    println!(
        "{} used keys, {} upstream string keys ({} help pages skipped), {} catalog keys",
        used.len(),
        upstream.len(),
        skipped,
        catalog.len()
    );
    println!("missing everywhere: {}", missing_everywhere.len());
    for k in &missing_everywhere {
        println!("  ! {k}");
    }
    println!("in upstream, not in catalog: {}", add.len());
    for (k, v) in &add {
        println!("  + {k} = {v:?}");
    }
    println!("wording differs: {}", diff.len());
    for (k, ours, want) in &diff {
        println!("  ~ {k}\n      ours: {ours:?}\n      up:   {want:?}");
    }
    println!("port-only (no upstream key): {}", port_only.len());
    for k in &port_only {
        println!("  . {k}");
    }
    println!("catalog keys no longer used: {}", unused.len());
    for k in &unused {
        println!("  - {k}");
    }

    if write && (!add.is_empty() || !diff.is_empty()) {
        for (k, v) in add {
            catalog.insert(k, v);
        }
        for (k, _, want) in diff {
            catalog.insert(k, want);
        }
        fs::write(&locales_path, emit_yml(&catalog))
            .map_err(|e| format!("cannot write {}: {e}", locales_path.display()))?;
        println!("wrote {}", locales_path.display());
        return Ok(0);
    }
    if missing_everywhere.is_empty() && add.is_empty() && diff.is_empty() {
        Ok(0)
    } else {
        // Missing or stale wording: a real problem the caller should fix.
        Ok(1)
    }
}

/// Flat `dotted.key -> value` view of the nested catalog.
fn parse_yml(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent % 2 != 0 {
            return Err(format!("indent is not a multiple of 2: {line:?}"));
        }
        let depth = indent / 2;
        let Some(colon) = trimmed.find(':') else {
            return Err(format!("line without ':': {line:?}"));
        };
        let key = trimmed[..colon].trim().to_string();
        let rest = trimmed[colon + 1..].trim();
        stack.truncate(depth);
        stack.push((depth, key));
        if !rest.is_empty() {
            let path = stack
                .iter()
                .map(|(_, k)| k.as_str())
                .collect::<Vec<_>>()
                .join(".");
            out.insert(path, unquote(rest));
        }
    }
    Ok(out)
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}

/// Nested YAML from the flat map (2-space indent, double-quoted values).
fn emit_yml(flat: &BTreeMap<String, String>) -> String {
    struct Tree {
        children: BTreeMap<String, Tree>,
        leaf: Option<String>,
    }
    let mut tree = Tree {
        children: BTreeMap::new(),
        leaf: None,
    };
    for (key, value) in flat {
        let mut node = &mut tree;
        let parts: Vec<&str> = key.split('.').collect();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                node.children.insert(
                    part.to_string(),
                    Tree {
                        children: BTreeMap::new(),
                        leaf: Some(value.clone()),
                    },
                );
            } else {
                node = node
                    .children
                    .entry(part.to_string())
                    .or_insert_with(|| Tree {
                        children: BTreeMap::new(),
                        leaf: None,
                    });
            }
        }
    }
    fn dump(node: &Tree, depth: usize, out: &mut String) {
        for (key, child) in &node.children {
            for _ in 0..depth {
                out.push_str("  ");
            }
            match &child.leaf {
                Some(value) => {
                    out.push_str(key);
                    out.push_str(": ");
                    out.push_str(&quote(value));
                    out.push('\n');
                }
                None => {
                    out.push_str(key);
                    out.push_str(":\n");
                    dump(child, depth + 1, out);
                }
            }
        }
    }
    let mut out = String::from(
        "# Spiderweb English catalog: wording copied verbatim from upstream\n\
         # (scripts/lang/en.json), never invented. Managed by `cargo xtask i18n-sync`.\n",
    );
    dump(&tree, 0, &mut out);
    out
}

fn quote(v: &str) -> String {
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// rust-i18n `%{name}` -> upstream `{name}` for comparison.
fn strip_percent(v: &str) -> String {
    v.replace("%{", "{")
}

/// upstream `{name}` -> rust-i18n `%{name}`.
fn placeholder(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 4);
    let mut prev = '\0';
    for c in v.chars() {
        if c == '{' && prev != '%' {
            out.push('%');
        }
        out.push(c);
        prev = c;
    }
    out
}

/// Collect every `t!("...")` key in the app sources.
fn scan_used(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().map(|e| e != "rs").unwrap_or(true) {
                continue;
            }
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            for line in text.lines() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                    continue;
                }
                for key in keys_in(line) {
                    out.insert(key);
                }
            }
        }
    }
    Ok(out)
}

fn keys_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        // Skip macro names ending in `t!(` such as `format!(` / `assert!(`.
        let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if boundary && &bytes[i..i + 4] == b"t!(\"" {
            i += 4;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'"' && i > start {
                out.push(text[start..i].to_string());
            }
        }
        i += 1;
    }
    out
}
