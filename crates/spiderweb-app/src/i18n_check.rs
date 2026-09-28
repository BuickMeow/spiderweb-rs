//! i18n 的守卫测试（手写扫描，不引 regex）：
//! 1. `src/**.rs` 里 `t!("...")` 用到的字面量键都要在 `locales/en.yml` 里；
//! 2. 非注释、非测试代码的字符串字面量里不再有中文字符（注释 / 测试代码例外）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 遍历目录下的 .rs 文件（`src/**.rs`）。
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 去掉注释（字符串 / 字符字面量里的 `//`、`/*` 不算注释）。
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let mut depth = 1;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if src[i..].starts_with('"') {
            i = copy_literal(src, i, &mut out);
            continue;
        }
        if bytes[i] == b'\''
            && let Some(end) = char_literal_end(&src[i + 1..])
        {
            i += 1 + end;
            continue;
        }
        let c = src[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// 把从 `start`（一个 `"`）开始的字符串字面量原样抄进 `out`，返回结束后的下标。
fn copy_literal(src: &str, start: usize, out: &mut String) -> usize {
    let bytes = src.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        let c = src[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
        if c == '\\' && i < bytes.len() {
            let n = src[i..].chars().next().unwrap_or(' ');
            out.push(n);
            i += n.len_utf8();
        } else if c == '"' {
            break;
        }
    }
    i
}

/// 找出源码里所有 `t!("key")` / `rust_i18n::t!("key")` 的字面量键。
fn key_literals(src: &str) -> Vec<String> {
    let src = strip_comments(src);
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b't' && bytes[i + 1] == b'!' && (i == 0 || !is_ident_byte(bytes[i - 1])) {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'(' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    let start = j + 1;
                    let mut k = start;
                    while k < bytes.len() && bytes[k] != b'"' {
                        k += if bytes[k] == b'\\' { 2 } else { 1 };
                    }
                    if k <= bytes.len() {
                        out.push(src[start..k].to_string());
                    }
                    i = k;
                }
            }
        }
        i += 1;
    }
    out
}

/// `locales/en.yml` 里的键（只认 `键:` 行；嵌套按缩进拼成 `a.b.c`）。
fn yaml_keys(text: &str) -> BTreeSet<String> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let key = &trimmed[..colon];
        if key.is_empty() || key.contains(' ') || key.contains('"') {
            continue;
        }
        while stack.last().is_some_and(|(ind, _)| *ind >= indent) {
            stack.pop();
        }
        let full = match stack.last() {
            Some((_, parent)) => format!("{parent}.{key}"),
            None => key.to_string(),
        };
        if trimmed[colon + 1..].trim().is_empty() {
            stack.push((indent, full));
        } else {
            out.insert(full);
        }
    }
    out
}

/// 测试代码（`#[cfg(test)] mod …` 起的）从扫描里去掉。
fn without_test_modules(src: &str) -> &str {
    let mut from = 0;
    while let Some(at) = src[from..].find("#[cfg(test)]") {
        let pos = from + at;
        let rest = src[pos + "#[cfg(test)]".len()..].trim_start();
        if rest.starts_with("mod ") {
            return &src[..pos];
        }
        from = pos + "#[cfg(test)]".len();
    }
    src
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

/// 字符串字面量（跳过字符字面量 / 注释）里出现的汉字：`(第几行, 字面量)`。
fn han_string_literals(src: &str) -> Vec<(usize, String)> {
    let src = without_test_modules(src);
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let mut depth = 1;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let c = src[i..].chars().next().unwrap_or(' ');
        if c == '"' {
            let line = src[..i].matches('\n').count() + 1;
            let mut text = String::new();
            i += 1;
            while i < bytes.len() {
                let c = src[i..].chars().next().unwrap_or(' ');
                i += c.len_utf8();
                if c == '\\' && i < bytes.len() {
                    let n = src[i..].chars().next().unwrap_or(' ');
                    text.push(c);
                    text.push(n);
                    i += n.len_utf8();
                } else if c == '"' {
                    break;
                } else {
                    text.push(c);
                }
            }
            if text.chars().any(is_han) {
                out.push((line, text));
            }
            continue;
        }
        if c == '\'' {
            // 字符字面量：整段跳过；生命周期 / 单引号直接跳过
            let rest = &src[i + 1..];
            if let Some(end) = char_literal_end(rest) {
                i += 1 + end;
                continue;
            }
        }
        i += c.len_utf8();
    }
    out
}

/// `'` 后面到配对的 `'` 的距离（含收尾的 `'`）；不是字符字面量返回 None。
fn char_literal_end(rest: &str) -> Option<usize> {
    let mut it = rest.char_indices();
    let (_, first) = it.next()?;
    if first == '\\' {
        it.next()?;
        let (at, last) = it.next()?;
        return (last == '\'').then_some(at + 1);
    }
    let (at, last) = it.next()?;
    (last == '\'').then_some(at + 1)
}

fn skip_file(name: &str) -> bool {
    matches!(name, "test_support.rs" | "i18n_check.rs")
}

#[test]
fn every_t_key_exists_in_en_yml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let yml = std::fs::read_to_string(root.join("locales/en.yml")).expect("读 locales/en.yml");
    let keys = yaml_keys(&yml);
    let mut missing: Vec<String> = Vec::new();
    let mut used = BTreeSet::new();
    for file in rust_files(&root.join("src")) {
        let src = std::fs::read_to_string(&file).expect("读源码");
        for key in key_literals(&src) {
            used.insert(key.clone());
            if !keys.contains(&key) {
                missing.push(format!("{}: {key}", file.display()));
            }
        }
    }
    assert!(!used.is_empty(), "源码里一个 t! 键都没有");
    assert!(
        missing.is_empty(),
        "en.yml 缺少这些键：\n{}",
        missing.join("\n")
    );
}

#[test]
fn no_han_in_production_string_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut bad: Vec<String> = Vec::new();
    for file in rust_files(&root.join("src")) {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if skip_file(name) {
            continue;
        }
        let src = std::fs::read_to_string(&file).expect("读源码");
        for (line, text) in han_string_literals(&src) {
            bad.push(format!("{}:{line}: \"{text}\"", file.display()));
        }
    }
    assert!(
        bad.is_empty(),
        "非测试代码的字符串字面量里还有中文：\n{}",
        bad.join("\n")
    );
}
