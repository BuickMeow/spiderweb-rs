"""向量生成脚本的公共部分：导入 Python 原版模块并把用例写成 JSON。

每个模块一个生成脚本 tools/gen_<module>_vectors.py，输出到
crates/spiderweb-core/tests/vectors/<module>.json，Rust 测试逐用例对照。
"""

import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPTS = os.path.join(os.path.dirname(REPO), "Spiderweb-main", "scripts")
OUT_DIR = os.path.join(REPO, "crates", "spiderweb-core", "tests", "vectors")

if SCRIPTS not in sys.path:
    sys.path.insert(0, SCRIPTS)


def write(module, cases):
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, f"{module}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"module": module, "cases": cases}, f, ensure_ascii=False)
    print(f"{module}: {len(cases)} cases -> {path}")
