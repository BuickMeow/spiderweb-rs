"""Mechanical conversion of help_texts.py: original scripts/window/help_texts.py -> crates/spiderweb-app/src/help_texts.rs.

The original module is imported directly; TOPICS / SECTIONS / NEXT / SEE / TOOL_TOPICS become
Rust constants as they are (strings escaped as Rust literals, non-ASCII kept as UTF-8). To change
the wording, edit the original and run this script again:

    python3 tools/gen_help_texts.py

The original script dir defaults to /Users/jieneng/Documents/GitHub/Spiderweb-main/scripts and
can be overridden with the SPIDERWEB_SCRIPTS environment variable (same convention as
tools/gen_*_vectors.py).
"""

import os
import subprocess
import sys

# when run in the worktree the original script dir cannot be derived; use the same fallback path as gen_*_vectors.py
_SCRIPTS = os.environ.get("SPIDERWEB_SCRIPTS",
                          "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from window.help_texts import NEXT, SECTIONS, SEE, TOOL_TOPICS, TOPICS  # noqa: E402

_OUT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                    "crates", "spiderweb-app", "src", "help_texts.rs")


def lit(s):
    """Python string -> Rust string literal (including the quotes)."""
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif o < 0x20 or o == 0x7F:
            out.append("\\u{%x}" % o)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def topic(t):
    return ("    Topic {\n"
            f"        id: {lit(t['id'])},\n"
            f"        section: {lit(t['section'])},\n"
            f"        title: {lit(t['title'])},\n"
            f"        tip: {lit(t['tip'])},\n"
            f"        text: {lit(t['text'])},\n"
            f"        words: {lit(t.get('words', ''))},\n"
            "    },")


def main():
    sections = {t["section"] for t in TOPICS}
    missing = sections - set(SECTIONS)
    if missing:
        sys.exit(f"section 不在 SECTIONS 里：{sorted(missing)}")
    for t in TOPICS:
        for key in ("id", "section", "title", "tip", "text"):
            if key not in t:
                sys.exit(f"主题 {t.get('id', '?')} 缺 {key}")
    ids = [t["id"] for t in TOPICS]
    if len(ids) != len(set(ids)):
        sys.exit("主题 id 有重复")
    for src, dst in list(NEXT.items()) + [(k, v) for k, vs in SEE.items() for v in vs]:
        for i in (src, dst):
            if i not in ids:
                sys.exit(f"NEXT / SEE 引用了不存在的主题 {i}")

    lines = [
        "//! 帮助文案：由 tools/gen_help_texts.py 从原版 scripts/window/help_texts.py 生成，请勿手改。",
        "//! 重新生成：python3 tools/gen_help_texts.py",
        "",
        "/// 一个帮助主题（原版 TOPICS 的元素）：tip 是首次使用的弹窗 / 工具按钮提示，",
        "/// text 是完整说明（帮助窗口与侧栏；原版的 [clip:...] 标记已去掉）。",
        "pub struct Topic {",
        "    pub id: &'static str,",
        "    pub section: &'static str,",
        "    pub title: &'static str,",
        "    pub tip: &'static str,",
        "    pub text: &'static str,",
        "    pub words: &'static str,",
        "}",
        "",
        "/// 帮助窗口里的小节顺序。",
        "pub const SECTIONS: &[&str] = &[",
    ]
    lines += [f"    {lit(s)}," for s in SECTIONS]
    lines += [
        "];",
        "",
        "/// 全部帮助主题。",
        "pub const TOPICS: &[Topic] = &[",
    ]
    lines += [topic(t) for t in TOPICS]
    lines += [
        "];",
        "",
        "/// 看完一个 tip 后接着弹的下一个主题（原版 NEXT）。",
        "pub const NEXT: &[(&str, &str)] = &[",
    ]
    lines += [f"    ({lit(k)}, {lit(v)})," for k, v in NEXT.items()]
    lines += [
        "];",
        "",
        "/// “See also”：主题 -> 关联主题（原版 SEE）。",
        "pub const SEE: &[(&str, &[&str])] = &[",
    ]
    for k, vs in SEE.items():
        inner = ", ".join(lit(v) for v in vs)
        lines.append(f"    ({lit(k)}, &[{inner}]),")
    lines += [
        "];",
        "",
        "/// 每个工具的 tip 主题（原版 TOOL_TOPICS；Square / Circle / Triangle 共用 box）。",
        "pub const TOOL_TOPICS: &[(&str, &str)] = &[",
    ]
    lines += [f"    ({lit(k)}, {lit(v)})," for k, v in TOOL_TOPICS.items()]
    lines += [
        "];",
        "",
        "/// 按 id 找主题。",
        "pub fn by_id(id: &str) -> Option<&'static Topic> {",
        "    TOPICS.iter().find(|t| t.id == id)",
        "}",
        "",
        "/// 主题的关联主题（SEE 里没有就是空）。",
        "pub fn see(id: &str) -> &'static [&'static str] {",
        "    SEE.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or(&[])",
        "}",
        "",
        "/// 看完 tip 后接着弹的下一个主题。",
        "pub fn next(id: &str) -> Option<&'static str> {",
        "    NEXT.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)",
        "}",
        "",
        "/// 工具 key（原版工具名）-> tip 主题 id。",
        "pub fn tool_topic(tool: &str) -> &'static str {",
        "    TOOL_TOPICS",
        "        .iter()",
        "        .find(|(k, _)| *k == tool)",
        "        .map(|(_, v)| *v)",
        '        .unwrap_or("select")',
        "}",
        "",
    ]
    with open(_OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    # The output is laid out by rustfmt directly, so cargo fmt --check stays clean (skipped without rustfmt)
    try:
        subprocess.run(["rustfmt", "--edition", "2024", _OUT], check=True)
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"rustfmt 跳过（{e}）：提交前请跑 cargo fmt -p spiderweb-app")
    print(f"{len(TOPICS)} 个主题 -> {_OUT}")


if __name__ == "__main__":
    main()
