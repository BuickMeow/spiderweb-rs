"""Differential vectors for bezier.py.

Every input float is first quantised to 15 significant digits before being handed to the Python
original: that way serde_json's default float parsing (without the float_roundtrip feature) can
reproduce it exactly, guaranteeing Rust and the original operate on the same doubles. The output
keeps the full precision the original computes.
"""

import copy
import math
import os
import random
import sys

from vec_common import write

# when run in the worktree, vec_common cannot derive the original script dir; add a fallback path
_SCRIPTS = (os.environ.get("SPIDERWEB_SCRIPTS") or os.environ.get("SPIDERWEB_SRC")
            or "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from notes import bezier as B


# base curves: 3 anchors (7 points), 4 anchors (10 points), 5 anchors (13 points)
C1 = [[0.0, 60.0], [1.0, 61.0], [2.0, 61.0], [3.0, 60.0], [4.0, 59.0], [5.0, 59.0], [6.0, 60.0]]
C2 = [[0.0, 60.0], [1.5, 63.0], [3.0, 60.0], [4.5, 57.0], [6.0, 60.0], [7.5, 63.0],
      [9.0, 60.0], [10.5, 57.0], [12.0, 60.0], [13.5, 63.0]]
S3 = [[0.0, 60.0], [1.0, 62.0], [2.0, 60.0], [3.0, 59.0], [4.0, 60.0], [5.0, 62.0], [6.0, 60.0]]
D3 = [[0.0, 60.0], [1.0, 62.0], [2.0, 63.0], [3.0, 64.0], [4.0, 63.0], [5.0, 62.0], [6.0, 61.0]]
CLOSED = [[0.0, 60.0], [1.0, 62.0], [2.0, 60.0], [3.0, 59.0], [4.0, 60.0], [5.0, 62.0], [0.0, 60.0]]
FIVE = [[0.0, 60.0], [1.0, 61.0], [2.0, 62.0], [3.0, 61.0], [4.0, 60.0], [5.0, 59.0], [6.0, 58.0],
        [7.0, 59.0], [8.0, 60.0], [9.0, 61.0], [10.0, 62.0], [11.0, 61.0], [12.0, 60.0]]


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


def mkcurve(pts, sharp=None, sym=None, gaps=None, splits=None):
    c = {"pts": [list(p) for p in pts]}
    if sharp:
        c["sharp"] = list(sharp)
    if sym:
        c["sym"] = sym
    if gaps:
        c["gaps"] = list(gaps)
    if splits:
        c["splits"] = list(splits)
    return c


def curve_out(c):
    return {
        "pts": [[p[0], p[1]] for p in c["pts"]],
        "sharp": list(c.get("sharp", [])),
        "sym": c.get("sym"),
        "gaps": list(c.get("gaps", [])),
        "splits": list(c.get("splits", [])),
    }


def screen(sx, sy):
    def to_screen(p):
        return (p[0] * sx, p[1] * sy)

    def from_screen(x, y):
        return [x / sx, y / sy]

    return to_screen, from_screen


# ---- wrapper functions with closures / that mutate their arguments

def sym_axis_call(pts, sc, exact):
    to_screen, _ = screen(*sc)
    return B.sym_axis(pts, to_screen, exact)


def half_at_call(pts, sc, x, y):
    to_screen, _ = screen(*sc)
    return B.half_at(pts, to_screen, x, y)


def nearest_call(pts, sc, x, y, n, gaps=None):
    to_screen, _ = screen(*sc)
    if gaps is None:
        return B.nearest(pts, to_screen, x, y, n)
    return B.nearest(pts, to_screen, x, y, n, gaps)


def set_sharp_call(c, sharp):
    c = copy.deepcopy(c)
    B.set_sharp(c, sharp)
    return curve_out(c)


def keep_symmetric_call(c, i, sc, exact):
    c = copy.deepcopy(c)
    to_screen, _ = screen(*sc)
    ok = B.keep_symmetric(c, i, to_screen, exact)
    return {"ret": bool(ok), "curve": curve_out(c)}


def drag_point_call(c, i, new, alt, sc, exact):
    c = copy.deepcopy(c)
    to_screen, from_screen = screen(*sc)
    B.drag_point(c, i, new, alt, to_screen, from_screen, exact)
    return curve_out(c)


def add_anchor_call(c, seg, t, new, sc, exact):
    c = copy.deepcopy(c)
    to_screen, _ = screen(*sc)
    ok = B.add_anchor(c, seg, t, new, to_screen, exact)
    return {"ret": bool(ok), "curve": curve_out(c)}


def delete_point_call(c, i, sc, exact):
    c = copy.deepcopy(c)
    to_screen, _ = screen(*sc)
    B.delete_point(c, i, to_screen, exact)
    return curve_out(c)


def set_symmetry_call(c, mode, source, sc, exact):
    c = copy.deepcopy(c)
    to_screen, _ = screen(*sc)
    B.set_symmetry(c, mode, source, to_screen, exact)
    return curve_out(c)


def shift_marks_call(c, after, d):
    c = copy.deepcopy(c)
    B.shift_marks(c, after, d)
    return curve_out(c)


def gen():
    cases = []

    # ---- basic functions
    for pts in (C1, C2, [[3.0, 4.0]], []):
        add(cases, "anchor_count", [pts], B.anchor_count)
    for p0, p1, p2, p3, t in (
        ([0.0, 60.0], [1.0, 62.0], [2.0, 58.0], [3.0, 60.0], 0.0),
        ([0.0, 60.0], [1.0, 62.0], [2.0, 58.0], [3.0, 60.0], 0.25),
        ([0.0, 60.0], [1.0, 62.0], [2.0, 58.0], [3.0, 60.0], 0.5),
        ([0.0, 60.0], [1.0, 62.0], [2.0, 58.0], [3.0, 60.0], 1.0),
        ([0.0, 60.0], [0.0, 60.0], [3.0, 60.0], [3.0, 60.0], 1.0 / 3.0),
    ):
        add(cases, "seg_point", [p0, p1, p2, p3, t], B.seg_point)
    for pts in (C1, C2, [[0.0, 1.0], [2.0, 3.0], [4.0, 5.0], [6.0, 7.0]], [[0.0, 1.0], [2.0, 3.0]]):
        add(cases, "segments", [pts], B.segments)
    for pts, n in ((C1, 0), (C1, 1), (C1, 3), (C1, 48), (C2, 5), (C2, 1)):
        add(cases, "sample", [pts, n], B.sample)

    # ---- split / remove_anchor / handle_anchor
    for pts, s, t in (
        (C1, 0, 0.0), (C1, 0, 0.25), (C1, 0, 0.5), (C1, 0, 1.0), (C1, 1, 0.75),
        (C2, 2, 0.75), (C2, 1, 1.0 / 3.0), (C2, 0, 0.1),
    ):
        add(cases, "split", [pts, s, t], B.split)
    for pts, a in ((C1, 1), (C2, 1), (C2, 2)):
        add(cases, "remove_anchor", [pts, a], B.remove_anchor)
    for i in range(12):
        add(cases, "handle_anchor", [i], B.handle_anchor)

    # ---- symmetric: mirror axis 0/1/None, turn, source 0/1, sharp, invalid input
    sym_cases = [
        (C1, [], "mirror", None, 0),
        (C1, [], "mirror", 0, 0),
        (C1, [], "mirror", 1, 0),
        (C1, [], "turn", None, 0),
        (C1, [], "turn", None, 1),
        (C1, [], "mirror", None, 1),
        (C1, [0, 2], "mirror", None, 0),
        (C1, [1], "turn", None, 1),
        (S3, [], "mirror", 1, 0),
        (S3, [2], "mirror", 1, 0),
        (S3, [0, 2], "mirror", None, 1),
        (D3, [], "mirror", 0, 0),
        (D3, [0, 2], "mirror", 1, 1),
        (FIVE, [], "mirror", None, 0),
        (FIVE, [1, 2, 3], "turn", None, 0),
        (FIVE, [1, 3], "mirror", None, 1),
        (CLOSED, [], "mirror", None, 0),   # first and last coincide: det=0 -> unchanged
        (C2, [], "mirror", None, 0),       # (n-1)%6 != 0 -> unchanged
    ]
    for p, sh, mode, axis, source in sym_cases:
        add(cases, "symmetric", [p, sh, mode, axis, source], B.symmetric)

    # ---- make_symmetric: an even segment count first splits the middle
    mk_sym = [
        (C2, [], "mirror", None, 0),
        (C2, [0, 3], "mirror", 1, 0),
        (C2, [1], "turn", None, 1),
        (C1, [], "mirror", None, 0),
        (C1, [0], "turn", 1, 1),
    ]
    for p, sh, mode, axis, source in mk_sym:
        add(cases, "make_symmetric", [p, sh, mode, axis, source], B.make_symmetric)

    # ---- set_sharp: the input curve is part of args too (deep-copied before mutating)
    for pts, initial, new_sharp in (
        (C1, [3], [5, 2, 2, 0]),
        (C1, [3], []),
        (C2, [], [3, 1, 3]),
    ):
        add(cases, "set_sharp", [mkcurve(pts, initial), new_sharp], set_sharp_call)

    # ---- sym_axis / half_at
    for pts, sc, exact in (
        (C1, [1.0, 1.0], False), (C1, [3.0, 1.0], False), (C1, [1.0, 3.0], False),
        (D3, [2.0, 0.5], False), (C1, [1.0, 1.0], True), (C1, [2.0, 2.0], False),
    ):
        add(cases, "sym_axis", [pts, sc, exact], sym_axis_call)
    for pts, x, y in (
        (C1, 1.0, 61.0), (C1, 5.0, 59.0), (C1, 3.0, 60.0),
        (C1, 1.5, 61.0), (C1, 4.5, 59.0), (S3, 4.0, 60.0),
    ):
        add(cases, "half_at", [pts, [1.0, 1.0], x, y], half_at_call)

    # ---- pen_handles / handle_lines
    C1C = [[0.0, 60.0], [0.0, 60.0], [0.0, 60.0], [3.0, 60.0], [3.0, 60.0], [3.0, 60.0], [6.0, 60.0]]
    C1H = [[0.0, 60.0], [0.0, 60.0], [2.5, 61.0], [3.0, 60.0], [3.5, 59.0], [6.0, 60.0], [6.0, 60.0]]
    for pts, sel in ((C1, True), (C1, False), (C1C, True), (C1H, True), (C2, True)):
        add(cases, "pen_handles", [pts, sel], B.pen_handles)
    for pts in (C1, C1C, C1H, S3):
        add(cases, "handle_lines", [pts], B.handle_lines)

    # ---- nearest
    for pts, sc, x, y, n in (
        (C1, [1.0, 1.0], 1.0, 61.0, 64),
        (C1, [1.0, 1.0], 5.0, 59.0, 64),
        (C1, [1.0, 1.0], 3.0, 60.0, 8),
        (C1, [2.0, 0.5], 6.0, 30.0, 16),
        (S3, [1.0, 1.0], 0.5, 61.5, 32),
    ):
        add(cases, "nearest", [pts, sc, x, y, n], nearest_call)

    # ---- keep_symmetric
    for pts, sharp, sym, i, sc, exact in (
        (S3, [], "mirror", 1, [1.0, 1.0], False),
        (S3, [], "mirror", 5, [1.0, 1.0], False),
        (S3, [], "turn", 3, [1.0, 1.0], False),
        (S3, [], "mirror", 1, [1.0, 1.0], True),
        (S3, [], None, 1, [1.0, 1.0], False),
        (FIVE, [1], "mirror", 3, [2.0, 0.5], False),
    ):
        add(cases, "keep_symmetric", [mkcurve(pts, sharp, sym), i, sc, exact], keep_symmetric_call)

    # ---- drag_point: anchor / handle / alt / symmetry / exact / screen scale
    drag = [
        (C1, [], None, 3, [3.5, 62.0], False, [1.0, 1.0], False),
        (C1, [], None, 3, [3.5, 62.0], True, [1.0, 1.0], False),
        (C1, [], None, 0, [0.5, 60.0], False, [1.0, 1.0], False),
        (C1, [], None, 0, [0.5, 60.0], True, [1.0, 1.0], False),
        (C1, [], None, 6, [6.5, 60.0], True, [1.0, 1.0], False),
        (C1, [], None, 4, [4.2, 58.0], False, [1.0, 1.0], False),
        (C1, [], None, 4, [4.2, 58.0], True, [1.0, 1.0], False),
        (C1, [], None, 2, [2.4, 61.5], False, [2.0, 0.5], False),
        (C1, [], None, 1, [0.4, 60.5], False, [1.0, 1.0], False),
        (C1, [1], None, 2, [2.4, 61.5], False, [1.0, 1.0], False),
        (S3, [], "mirror", 3, [3.0, 58.0], False, [1.0, 1.0], False),
        (S3, [], "mirror", 4, [4.5, 60.5], False, [1.0, 1.0], False),
        (S3, [], "mirror", 0, [0.5, 61.0], False, [1.0, 1.0], False),
        (S3, [], "mirror", 6, [5.5, 61.0], False, [1.0, 1.0], False),
        (S3, [], "mirror", 3, [3.0, 58.0], False, [1.0, 1.0], True),
        (S3, [], "turn", 4, [4.5, 60.5], False, [1.0, 1.0], False),
        (FIVE, [0, 4], "turn", 1, [1.5, 62.0], False, [1.0, 1.0], False),
    ]
    for pts, sharp, sym, i, new, alt, sc, exact in drag:
        add(cases, "drag_point", [mkcurve(pts, sharp, sym), i, new, alt, sc, exact], drag_point_call)

    # ---- add_anchor: t=0/1, different segments, a symmetric curve pair, the same segment twice
    adda = [
        (C1, [], None, 0, 0.5, [1.5, 60.5], False),
        (C1, [], None, 1, 0.25, [4.0, 62.0], False),
        (C1, [], None, 0, 0.0, [1.5, 60.5], False),
        (C1, [], None, 0, 1.0, [1.5, 60.5], False),
        (C1, [], None, 1, 1.0 / 3.0, [4.0, 61.0], False),
        (S3, [], "mirror", 0, 0.5, [1.5, 61.0], False),
        (S3, [], "mirror", 1, 0.5, [4.5, 61.0], False),
        (S3, [], "mirror", 0, 0.25, [1.0, 61.5], False),
        (S3, [], "mirror", 0, 0.5, [1.5, 61.0], True),
        (C2, [], "mirror", 1, 0.5, [4.0, 61.0], False),   # 4 anchors, 3 segments: the same segment is split twice
    ]
    for pts, sharp, sym, seg, t, new, exact in adda:
        add(cases, "add_anchor", [mkcurve(pts, sharp, sym), seg, t, new, [1.0, 1.0], exact],
            add_anchor_call)

    # ---- can_delete / delete_point
    for pts, sharp, sym, i in (
        (C1, [], None, 0), (C1, [], None, 1), (C1, [], None, 3), (C1, [], None, 4),
        (C1, [], None, 5), (C1, [], None, 6), (S3, [], "mirror", 3), (S3, [], "mirror", 0),
        (S3, [], "mirror", 6), (S3, [], "mirror", 4), (FIVE, [], "mirror", 6),
        (FIVE, [], "turn", 3), (C2, [], None, 5), (C2, [], None, 2),
    ):
        add(cases, "can_delete", [mkcurve(pts, sharp, sym), i], B.can_delete)

    dele = [
        (C1, [], None, 3), (C1, [], None, 4), (C1, [], None, 1), (C1, [], None, 0),
        (S3, [], "mirror", 3), (S3, [], "mirror", 0), (S3, [1], "mirror", 4),
        (FIVE, [0, 4], "mirror", 3), (C2, [1, 3], None, 6), (C2, [], None, 5),
        (FIVE, [], "turn", 9),
    ]
    for pts, sharp, sym, i in dele:
        add(cases, "delete_point", [mkcurve(pts, sharp, sym), i, [1.0, 1.0], False], delete_point_call)

    # ---- set_symmetry
    setsym = [
        (C1, [], None, "mirror", 0, False),
        (C1, [], None, "mirror", 1, False),
        (C1, [1], "turn", "turn", 0, False),
        (C1, [1], "turn", "turn", 1, True),
        (C2, [], None, "mirror", 0, False),
        (C2, [0, 3], None, "turn", 1, False),
        (S3, [], "mirror", "mirror", 0, False),
        (S3, [], "turn", "turn", 0, False),
        (S3, [], "turn", None, 0, False),
        (C1, [], "turn", "turn", 0, True),
    ]
    for pts, sharp, init_sym, mode, source, exact in setsym:
        add(cases, "set_symmetry", [mkcurve(pts, sharp, init_sym), mode, source, [1.0, 1.0], exact],
            set_symmetry_call)

    # ---- resample
    for points, n in (
        ([[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], 6),
        ([[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], 1),
        ([[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], 300),
        ([[1.0, 2.0], [1.0, 2.0]], 4),
        ([[5.0, 5.0]], 3),
        ([[0.0, 0.0], [0.0, 0.0], [2.0, 0.0]], 5),
        ([[6.0, 60.0], [0.0, 60.0]], 4),
        ([[0.0, 60.0], [1.0, 61.0], [2.0, 60.0], [3.0, 59.0]], 7),
    ):
        add(cases, "resample", [points, n], B.resample)

    # ---- difference
    for a, b in ((C1, C1), (C1, C2), (C2, C1), (S3, D3),
                 (C1, [[0.0, 60.0], [1.0, 60.0], [2.0, 60.0], [3.0, 60.0]])):
        add(cases, "difference", [a, b], B.difference)

    # ---- fit: straight line / two points / all identical / circle samples / random polylines
    line_pts = [[i * 1.0, 60.0 + i * 0.5] for i in range(11)]
    add(cases, "fit", [line_pts, 0.003], B.fit)
    add(cases, "fit", [[[0.0, 60.0], [4.0, 64.0]], 0.003], B.fit)
    add(cases, "fit", [[[2.0, 60.0]] * 5, 0.003], B.fit)
    circle_pts = [[5.0 * math.cos(i / 20.0), 5.0 * math.sin(i / 20.0)] for i in range(56)]
    add(cases, "fit", [circle_pts, 0.003], B.fit)
    add(cases, "fit", [circle_pts, 0.05], B.fit)
    add(cases, "fit", [circle_pts, 0.0001], B.fit)
    zig = [[0.0, 60.0], [2.0, 66.0], [4.0, 54.0], [6.0, 66.0], [8.0, 54.0], [10.0, 60.0]]
    add(cases, "fit", [zig, 0.003], B.fit)
    cross = [[0.0, 60.0], [4.0, 64.0], [0.0, 64.0], [4.0, 60.0]]
    add(cases, "fit", [cross, 0.003], B.fit)

    rnd = random.Random(2024)
    for k in range(14):
        n = rnd.randint(4, 40)
        t, y = 0.0, 60.0
        pts = [[t, y]]
        for _ in range(n - 1):
            t += rnd.uniform(0.5, 3.0)
            y += rnd.uniform(-4.0, 4.0)
            pts.append([round(t, 4), round(y, 4)])
        if k % 3 == 1:
            pts[n // 2] = list(pts[n // 2 - 1])  # duplicate point
        if k % 3 == 2:
            y += rnd.uniform(-4.0, 4.0)
            pts.append([round(t + 0.25, 4), round(y, 4)])
        tol = (1e-4, 3e-3, 5e-2)[k % 3]
        add(cases, "fit", [pts, tol], B.fit)

    # ---- 1.2.0: piece_ends / fixed_anchors / shift_marks, and the gap-aware editing
    add(cases, "piece_ends", [mkcurve(C1)], lambda c: sorted(B.piece_ends(c)))
    add(cases, "piece_ends", [mkcurve(C1, gaps=[1])], lambda c: sorted(B.piece_ends(c)))
    add(cases, "piece_ends", [mkcurve(C2, gaps=[0, 2])], lambda c: sorted(B.piece_ends(c)))
    add(cases, "fixed_anchors", [mkcurve(C1)], lambda c: sorted(B.fixed_anchors(c)))
    add(cases, "fixed_anchors", [mkcurve(C1, gaps=[1])], lambda c: sorted(B.fixed_anchors(c)))
    add(cases, "fixed_anchors", [mkcurve(C2, splits=[1, 2])], lambda c: sorted(B.fixed_anchors(c)))
    add(cases, "fixed_anchors", [mkcurve(C2, gaps=[1], splits=[2])],
        lambda c: sorted(B.fixed_anchors(c)))
    for c, after, d in (
        (mkcurve(C1), 0, 1), (mkcurve(C1), 1, -1),
        (mkcurve(C1, gaps=[1], splits=[1]), 0, 1),
        (mkcurve(C2, gaps=[0, 2], splits=[1]), 2, -1),
        (mkcurve(C2, gaps=[1], splits=[2]), 1, 1),
    ):
        add(cases, "shift_marks", [c, after, d], shift_marks_call)

    # pen_handles / handle_lines / nearest with gaps
    for pts, sel, gaps in ((C1, True, [1]), (C1, False, [1]), (C1, True, [0]),
                           (C2, True, [1]), (C1H, True, [1])):
        add(cases, "pen_handles", [pts, sel, list(gaps)], B.pen_handles)
    for pts, gaps in ((C1, [1]), (C1, [0]), (C1C, [1]), (S3, [0, 1])):
        add(cases, "handle_lines", [pts, list(gaps)], B.handle_lines)
    for pts, sc, x, y, n, gaps in (
        (C1, [1.0, 1.0], 1.0, 61.0, 64, [0]),
        (C1, [1.0, 1.0], 5.0, 59.0, 64, [1]),
        (C1, [1.0, 1.0], 3.0, 60.0, 8, [0, 1]),
        (C2, [2.0, 0.5], 6.0, 30.0, 16, [2]),
    ):
        add(cases, "nearest", [pts, sc, x, y, n, list(gaps)], nearest_call)

    # can_delete / delete_point / add_anchor on joined curves (gaps / splits)
    for pts, sharp, sym, gaps, splits, i in (
        (C1, [], None, [1], [], 0), (C1, [], None, [1], [], 1), (C1, [], None, [1], [], 3),
        (C2, [], None, [1], [], 3), (C2, [], None, [1], [], 4), (C2, [], None, [1], [], 6),
        (C2, [], None, [], [1], 3), (C2, [], None, [], [1], 6), (C2, [], None, [], [1], 4),
        (FIVE, [], None, [2], [3], 6), (FIVE, [], None, [2], [3], 9),
    ):
        c = mkcurve(pts, sharp, sym, gaps=gaps, splits=splits)
        add(cases, "can_delete", [c, i], B.can_delete)
    for pts, sharp, sym, gaps, splits, i in (
        (C2, [], None, [1], [], 3), (C2, [], None, [1], [], 4), (C2, [], None, [1], [], 6),
        (C2, [], None, [], [1], 3), (C2, [], None, [], [1], 4),
        (FIVE, [], None, [2], [3], 9), (FIVE, [], None, [2], [], 3),
    ):
        c = mkcurve(pts, sharp, sym, gaps=gaps, splits=splits)
        add(cases, "delete_point", [c, i, [1.0, 1.0], False], delete_point_call)
    for pts, sharp, sym, gaps, splits, seg, t, new in (
        (C1, [], None, [1], [], 1, 0.5, [4.0, 62.0]),   # a gap: not added
        (C1, [], None, [1], [], 0, 0.5, [1.5, 60.5]),   # before the gap: marks shift
        (C2, [], None, [1], [], 2, 0.5, [7.0, 60.0]),   # after the gap: marks shift
        (C2, [], None, [], [1], 0, 0.5, [1.5, 60.0]),   # splits shift
        (C2, [], None, [], [1], 2, 0.5, [7.0, 60.0]),
    ):
        c = mkcurve(pts, sharp, sym, gaps=gaps, splits=splits)
        add(cases, "add_anchor", [c, seg, t, new, [1.0, 1.0], False], add_anchor_call)

    return {"module": "bezier", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("bezier", data["cases"])
