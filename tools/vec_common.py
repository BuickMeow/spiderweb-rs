"""Common pieces of the vector generator scripts: import the Python originals and write the cases as JSON.

One generator script per module, tools/gen_<module>_vectors.py, writes to
crates/spiderweb-core/tests/vectors/<module>.json for the Rust tests to compare case by case.
"""

import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Original source dir: defaults to the 1.1.0 reference copy; SPIDERWEB_SRC can point at another version (e.g. 1.2.0)
SCRIPTS = os.environ.get("SPIDERWEB_SRC") or os.path.join(os.path.dirname(REPO), "Spiderweb-main", "scripts")
if not os.path.isdir(SCRIPTS):
    # when the worktree is in a temp dir the relative path misses the original, so fall back to a fixed path
    SCRIPTS = "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts"
OUT_DIR = os.path.join(REPO, "crates", "spiderweb-core", "tests", "vectors")

if SCRIPTS not in sys.path:
    sys.path.insert(0, SCRIPTS)


def write(module, cases):
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, f"{module}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"module": module, "cases": cases}, f, ensure_ascii=False)
    print(f"{module}: {len(cases)} cases -> {path}")
