"""Differential vectors for arc.py.

Every input float is first quantised to 15 significant digits before being handed to the Python
original: that way serde_json's default float parsing (without the float_roundtrip feature) can
reproduce it exactly, guaranteeing Rust and the original operate on the same doubles.
"""

import math
import os
import sys

from vec_common import write

# when run in the worktree, vec_common cannot derive the original script dir; add a fallback path
_SCRIPTS = (os.environ.get("SPIDERWEB_SCRIPTS") or os.environ.get("SPIDERWEB_SRC")
            or "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from notes import arc as A


def r15(x):
    return float("%.15g" % x)


def rjson(v):
    if isinstance(v, bool):
        return v
    if isinstance(v, float):
        return r15(v)
    if isinstance(v, (list, tuple)):
        return [rjson(x) for x in v]
    if isinstance(v, dict):
        return {k: rjson(x) for k, x in v.items()}
    return v


def serde_parse(x):
    """Replicate serde_json's float parsing (float_roundtrip off) for self-checks."""
    tok = repr(x)
    neg = tok.startswith("-")
    if neg:
        tok = tok[1:]
    if "e" in tok or "E" in tok:
        mant, _, exp = tok.replace("E", "e").partition("e")
        expl = int(exp)
    else:
        mant, expl = tok, 0
    ip, _, fp = mant.partition(".")
    digits = (ip + fp).lstrip("0")
    sig = int(digits) if digits else 0
    f = float(sig)
    e = expl - len(fp)
    if e >= 0:
        while e > 308:
            f /= 1e308
            e -= 308
        f *= float("1e%d" % e)
    else:
        while -e > 308:
            f /= 1e308
            e += 308
        f /= float("1e%d" % -e)
    return -f if neg else f


def check(v):
    if isinstance(v, bool):
        return
    if isinstance(v, float):
        assert serde_parse(v) == v, (v, repr(v))
    elif isinstance(v, (list, tuple)):
        for x in v:
            check(x)
    elif isinstance(v, dict):
        for x in v.values():
            check(x)


def add(cases, fn, args, call):
    a = rjson(args)
    check(a)
    cases.append({"fn": fn, "args": a, "out": call(*a)})


def arc_k_one(k):
    return A.arc_k({"k": k})


def gen():
    cases = []

    # ---- circle: generic triangle / collinear / coincident / nearly collinear
    triples = [
        ([0.0, 0.0], [1.0, 1.0], [2.0, 0.0]),
        ([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]),           # collinear
        ([0.0, 0.0], [0.0, 0.0], [1.0, 2.0]),           # first two points coincide
        ([1.0, 1.0], [2.0, 2.0], [1.0, 1.0]),           # first and last coincide
        ([0.0, 0.0], [3.0, 4.0], [6.0, 8.0000000001]),  # nearly collinear
        ([-2.5, 3.25], [0.0, 7.0], [4.0, -1.0]),
        ([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]),
        ([5.0, 5.0], [5.0, 5.0], [5.0, 5.0]),           # all coincide
    ]
    for triple in triples:
        add(cases, "circle", list(triple), A.circle)

    # ---- full_circle
    whole = [
        ([0.0, 0.0], [1.0, 0.0], [0.0, 0.0]),
        ([1.0, 1.0], [2.0, 2.0], [1.0, 1.0]),
        ([0.0, 0.0], [1.0, 0.0], [1.0, 0.0]),  # first and last differ
        ([0.0, 0.0], [0.0, 0.0], [0.0, 0.0]),  # all identical
        ([-1.0, 2.0], [3.0, -0.5], [-1.0, 2.0]),
    ]
    for triple in whole:
        add(cases, "full_circle", list(triple), A.full_circle)

    # ---- arc_circle: generic triangle / k / full circle / collinear (None when there is no arc)
    for p, k in (
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 2.0),
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0),
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0),
    ):
        add(cases, "arc_circle", [p, k], A.arc_circle)

    # ---- arc_points: k / step / collinear / full circle / over half circle / too few points
    arc_pts = [
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0, A.STEP),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5, A.STEP),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 2.0, A.STEP),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 1.0, A.STEP),    # over half circle
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0, 0.5),         # full circle (large step)
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0, A.STEP),      # full circle (default step)
        ([[0.0, 1.0], [1.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # exactly past 0
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0, A.STEP),      # collinear
        ([[0.0, 0.0], [0.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # middle point equals start
        ([[0.0, 0.0], [2.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # middle point equals end
        ([[0.0, 0.0], [1.0, 2.0]], 1.0, A.STEP),                  # only two points
        ([[3.0, 4.0]], 1.0, A.STEP),                              # only one point
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0, math.pi / 8),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 0.25, math.pi / 4),
    ]
    for p, k, step in arc_pts:
        add(cases, "arc_points", [p, k, step], A.arc_points)

    # ---- arc_bezier: quarter segmentation / k / full circle / clockwise and counter-clockwise / collinear
    arc_bez = [
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0),        # half circle -> 2 segments
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 2.0),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.7),        # k not an integer multiple
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 1.0),       # over half circle -> 3 segments
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0),        # full circle -> 4 segments
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 0.5),
        ([[0.0, 1.0], [1.0, 0.0], [2.0, 1.0]], 1.0),        # clockwise
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0),        # collinear -> straight line
        ([[0.0, 0.0], [0.0, 0.0], [2.0, 0.0]], 1.0),
        ([[0.0, 0.0], [3.0, 0.0], [0.0, 0.0]], 2.5),        # flattened full circle
        ([[0.0, 0.0], [1.0, 0.001], [2.0, 0.0]], 1.0),      # very flat arc
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 10.0),       # extremely flat (nearly straight)
    ]
    for p, k in arc_bez:
        add(cases, "arc_bezier", [p, k], A.arc_bezier)

    # ---- ellipse_bezier / line_bezier
    for box in ([0.0, 0.0, 4.0, 2.0], [1.0, 2.0, 1.0, 5.0], [-3.0, -2.0, -1.0, 0.0], [2.0, 2.0, 2.0, 2.0]):
        add(cases, "ellipse_bezier", [box], A.ellipse_bezier)

    for a, c in (([0.0, 0.0], [6.0, 3.0]), ([2.0, 5.0], [-2.0, -5.0]), ([1.0, 1.0], [1.0, 1.0])):
        add(cases, "line_bezier", [a, c], A.line_bezier)

    # ---- clean_k (arc_k({"k": ...}) on the Python side): bounds, negatives, inf / nan
    ks = [1.0, 0.5, 2.0, 1e-9, 1.0001e-9, 1e9, 999999999.0, 1e10, -1.0, 0.0, "inf", "-inf", "nan"]
    for k in ks:
        add(cases, "clean_k", [k], arc_k_one)

    return {"module": "arc", "cases": cases}


if __name__ == "__main__":
    data = gen()
    # Self-check: floats in all outputs only need to compare within Rust's 1e-9 tolerance, so no quantisation is needed
    write("arc", data["cases"])
