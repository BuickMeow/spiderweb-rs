"""tumour.py 的对照向量（tools/gen_tumour_vectors.py）。"""

import math
import random

import numpy as np

from vec_common import write

from notes import tumour as T


def tolist(x):
    if isinstance(x, np.ndarray):
        return x.tolist()
    if isinstance(x, (list, tuple)):
        return [tolist(v) for v in x]
    if isinstance(x, dict):
        return {k: tolist(v) for k, v in x.items()}
    return x


def tm(**kw):
    d = dict(T.TUMOUR_DEFAULTS)
    d.update(kw)
    return d


# ---- 路径 -----------------------------------------------------------------

LINE = [[0.0, 60.0], [1.0, 60.0]]
VLINE = [[0.0, 60.0], [1.0, 64.0]]
POLY = [[0.0, 60.0], [0.4, 62.0], [0.8, 59.0], [1.2, 63.0]]
CIRCLE = [
    [0.5 + 0.5 * math.cos(2 * math.pi * i / 16), 60.0 + 0.5 * math.sin(2 * math.pi * i / 16)]
    for i in range(17)
]
SQUARE_LOOP = [[0.0, 60.0], [1.0, 60.0], [1.0, 64.0], [0.0, 64.0], [0.0, 60.0]]
BACK = [[0.0, 60.0], [0.5, 61.0], [0.0, 62.0]]
DUPES = [[0.0, 60.0], [0.0, 60.0], [0.5, 61.0], [0.5, 61.0], [1.0, 60.5]]
TINY = [[0.0, 60.0], [1e-13, 60.0]]
SHORT = [[0.0, 60.0]]
ZERO = [[0.0, 60.0], [0.0, 60.0]]
DIAG = [[0.0, 60.0], [0.5, 63.0], [1.0, 60.0], [1.5, 57.0]]


def gen():
    cases = []

    def add(fn, args, out):
        cases.append({"fn": fn, "args": tolist(args), "out": tolist(out)})

    def add_clean(raw):
        add("clean_tumour", [raw], T.clean_tumour(raw))

    def add_tp(path, raw):
        cleaned = T.clean_tumour(raw)
        add("tumour_path", [path, raw], T.tumour_path(path, cleaned))

    # ---- clean_tumour ----
    add_clean(None)
    add_clean([])
    add_clean(5)
    add_clean("tumour")
    add_clean({})
    add_clean(tm())
    add_clean(tm(shape="circle", side="right", wrap="wrap", size=7.5, length=0.25, dist=0.5,
                 start=0.1, end=0.9, ease=0.2, fit=True, seed=99, mirror=True, k=2.0, on=False))
    add_clean({"shape": "hexagon", "side": "up", "wrap": "spiral"})
    add_clean({"shape": 42, "side": None, "wrap": ["simple"]})
    add_clean({"size": -5000})
    add_clean({"size": 5000})
    add_clean({"length": -3})
    add_clean({"length": 2000000})
    add_clean({"dist": 0})
    add_clean({"dist": -5})
    add_clean({"dist": 2000000})
    add_clean({"start": -1, "end": 5})
    add_clean({"start": -0.5, "end": 0.75})
    add_clean({"ease": -2})
    add_clean({"ease": 3000000})
    add_clean({"k": 0})
    add_clean({"k": -1})
    add_clean({"k": 10000000000})
    add_clean({"seed": "42"})
    add_clean({"seed": 3.7})
    add_clean({"seed": True})
    add_clean({"seed": "0x10"})
    add_clean({"seed": None})
    add_clean({"seed": "1_0"})
    add_clean({"seed": -7})
    add_clean({"on": False})
    add_clean({"on": True})
    add_clean({"on": 0})
    add_clean({"on": 1})
    add_clean({"on": "false"})
    add_clean({"mirror": True})
    add_clean({"mirror": False})
    add_clean({"mirror": 0})
    add_clean({"mirror": "true"})
    add_clean({"fit": True})
    add_clean({"fit": False})
    add_clean({"fit": 1})
    add_clean({"fit": "true"})
    add_clean({"size": "1_0.5", "length": "0.5", "k": "2.5", "start": "0.25", "end": 0.75})
    add_clean({"size": "abc"})
    add_clean({"size": "inf"})
    add_clean({"size": "-inf"})
    add_clean({"size": "nan"})
    add_clean({"size": True})
    add_clean({"dist": None})
    add_clean({"start": [0.5], "end": {"a": 1}})
    add_clean({"k": "1e-3", "ease": " 0.5 "})

    # ---- clean_tumour: rot / slant (1.2.0) ----
    add_clean({"rot": 45, "slant": -0.3})
    add_clean({"rot": -181, "slant": 5})
    add_clean({"rot": 181, "slant": -5})
    add_clean({"rot": "45.5", "slant": "0.25"})
    add_clean({"rot": None, "slant": None})
    add_clean({"rot": "abc", "slant": [1]})
    add_clean({"rot": "nan", "slant": "inf"})

    # ---- clean_graph ----
    for raw in [
        None,
        [],
        [[0, 1]],
        [[0, 1], [1, 1]],
        [[0, 0.5], [1, 2]],
        [[0, 0.5], [0.5, 2], [1, 0.5]],
        [[2, 0.5], [1, 2]],
        [[0, 100], [1, -100]],
        [[0, "x"], [1, 1]],
        [[0, 1], ["a", 2]],
        [[0, 1], None],
        [[0, 1], [1]],
        [[0, "nan"], [1, 2]],
        [["a", 1], [1, 2]],
        "abc",
        {"a": 1},
        [0, 1],
        [[0, 0.5], [0.25, 2], [0.75, 0.5], [1, 2], [1, 3]],
    ]:
        add("clean_graph", [raw], T.clean_graph(raw))

    # ---- graphs in clean_tumour ----
    add_clean({"graphs": 5})
    add_clean({"graphs": []})
    add_clean({"graphs": {"size": [[0, 1], [1, 1]]}})
    add_clean({"graphs": {"size": "x"}})
    add_clean({"graphs": {"bogus": [[0, 2], [1, 2]]}})
    add_clean({"graphs": {"size": [[0, 0.5], [1, 2]], "bogus": [[0, 2], [1, 2]], "dist": [[0, 1], [0.5, 3], [1, 1]]}})
    add_clean({"size": 4.0, "graphs": {"size": [[0, 0], [1, 3]], "rot": [[0, -1], [1, 1]]}})

    # ---- template (1.2.0: slant) ----
    for shape, length, size, slant in [
        ("triangle", 0.5, 2.0, 0.0),
        ("triangle", 0.125, -3.0, 0.0),
        ("triangle", 0.0, 2.0, 0.0),
        ("triangle", 0.5, 2.0, 0.75),
        ("square", 0.5, 2.0, 0.0),
        ("square", 0.25, 0.0, 0.0),
        ("square", 0.5, 2.0, 0.5),
        ("square", 0.5, 2.0, -1.0),
        ("square", 0.5, 2.0, 1.0),
        ("square", 0.25, -3.0, 0.3),
        ("parabola", 0.5, 2.0, 0.0),
        ("parabola", 0.25, 0.0, 0.0),
        ("parabola", 0.1, -1.5, 0.5),
        ("circle", 0.5, 2.0, 0.0),
        ("circle", 0.5, 0.0, 0.0),
        ("circle", 0.1, 5.0, 0.5),
        ("circle", 0.5, -2.0, 0.0),
    ]:
        add("template", [shape, length, size, slant], T.template(shape, length, size, slant))

    # ---- cut ----
    tri = T.template("triangle", 0.5, 2.0)
    sq = T.template("square", 0.5, 2.0)
    circ = T.template("circle", 0.5, 2.0)
    for pts, x_end in [
        (tri, 0.5),
        (tri, 0.25),
        (tri, 0.0),
        (tri, -0.1),
        (sq, 0.1),
        (sq, 0.7),
        (circ, 0.3),
        (circ, 0.6),
    ]:
        add("cut", [pts, x_end], T.cut(pts, x_end))

    # ---- subdivide ----
    tb = T.template("triangle", 0.5, 2.0)
    sb = T.template("square", 0.5, 2.0)
    folded = [[0.0, 0.0], [0.5, 1.0], [0.2, 2.0]]
    for bump, xs in [
        (tb, [0.1, 0.2, 0.3]),
        (tb, []),
        (tb, [-1.0, 2.0]),
        (sb, [0.05, 0.3, 0.49]),
        (folded, [0.1, 0.3, 0.4]),
        (folded, [0.45, 0.25]),
    ]:
        add("subdivide", [bump, xs], T.subdivide(bump, xs))

    # ---- sub_graph (1.2.0) ----
    for g, a, b in [
        ([[0.0, 1.0], [1.0, 1.0]], 0.0, 1.0),
        ([[0.0, 0.5], [0.5, 2.0], [1.0, 0.5]], 0.0, 0.5),
        ([[0.0, 0.5], [0.5, 2.0], [1.0, 0.5]], 0.5, 1.0),
        ([[0.0, 0.5], [0.5, 2.0], [1.0, 0.5]], 0.25, 0.75),
        ([[0.0, 0.5], [1.0, 2.0]], 0.3, 0.9),
        ([[0.0, 2.0], [0.5, 0.0], [1.0, 2.0]], 0.1, 0.4),
    ]:
        add("sub_graph", [g, a, b], T.sub_graph(g, a, b))

    # ---- graph_fn / graph_starts (1.2.0) ----
    for raw, key, total, ds in [
        ({"graphs": {"size": [[0, 0.5], [1, 2.0]]}}, "size", 2.0, [0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0]),
        ({"graphs": {"size": [[0, 0.5], [0.5, 2.0], [1, 0.5]]}}, "size", 1.0, [0.0, 0.1, 0.5, 0.9, 1.0]),
        ({"graphs": {"size": [[0, 3.0], [1, 0.0]]}}, "size", 4.0, [-1.0, 0.0, 2.0, 4.0, 5.0]),
        ({}, "size", 1.0, [0.5]),
        ({"graphs": {"dist": [[0, 2.0], [1, 0.5]]}}, "dist", 1.0, [0.0, 0.3, 0.7, 1.0]),
    ]:
        cleaned = T.clean_tumour(raw)
        dg = T.graph_fn(cleaned, key, total)
        add("graph_fn", [raw, key, total, ds], None if dg is None else [float(dg(d)) for d in ds])

    for raw, lo, hi, dist, fit, even in [
        ({"graphs": {"dist": [[0, 0.5], [1, 1.5]]}}, 0.0, 1.0, 0.125, False, False),
        ({"graphs": {"dist": [[0, 2.0], [1, 0.5]]}}, 0.0, 1.0, 0.1, False, False),
        ({"graphs": {"dist": [[0, 2.0], [1, 0.5]]}}, 0.0, 1.0, 0.1, True, False),
        ({"graphs": {"dist": [[0, 0.5], [0.5, 2.0], [1, 1.0]]}}, 0.0, 1.0, 0.15, False, True),
        ({"graphs": {"dist": [[0, 0.5], [0.5, 2.0], [1, 1.0]]}}, 0.25, 0.75, 0.05, True, False),
        ({"graphs": {"dist": [[0, 3.0], [1, 0.25]]}}, 0.0, 1.0, 0.4, True, True),
        ({"graphs": {"dist": [[0, 1.5], [1, 0.5]]}}, 0.5, 0.5, 0.1, False, False),
        ({"graphs": {"dist": [[0, 10.0], [1, 0.1]]}}, 0.0, 1.0, 0.01, False, False),
    ]:
        cleaned = T.clean_tumour(raw)
        dg = T.graph_fn(cleaned, "dist", 1.0)
        add("graph_starts", [raw, lo, hi, dist, fit, even], T.graph_starts(dg, lo, hi, dist, fit, even))

    # ---- tumour_path：4 shape × 4 side（random 用不同 seed）× 2 wrap ----
    for shape in ["triangle", "square", "circle", "parabola"]:
        for wrap in ["simple", "wrap"]:
            for side in ["alt", "left", "right"]:
                add_tp(LINE, tm(shape=shape, side=side, wrap=wrap))
            for seed in [1, 42]:
                add_tp(LINE, tm(shape=shape, side="random", wrap=wrap, seed=seed))
    # 另一条折线 / 竖线 / 反向路径上的代表用例
    add_tp(VLINE, tm(shape="triangle", side="alt", wrap="simple"))
    add_tp(VLINE, tm(shape="circle", side="left", wrap="wrap", size=-2.0))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="simple"))
    add_tp(POLY, tm(shape="square", side="right", wrap="wrap"))
    add_tp(POLY, tm(shape="parabola", side="alt", wrap="wrap", dist=0.5))
    add_tp(BACK, tm(shape="triangle", side="alt", wrap="simple"))
    add_tp(BACK, tm(shape="circle", side="alt", wrap="wrap"))
    add_tp(DIAG, tm(shape="square", side="random", wrap="simple", seed=7))
    add_tp(DUPES, tm(shape="triangle", side="alt", wrap="simple"))
    add_tp(DUPES, tm(shape="triangle", side="alt", wrap="wrap"))

    # ---- 太短 / 空路径：原样返回 ----
    add_tp(TINY, tm())
    add_tp(SHORT, tm())
    add_tp(ZERO, tm())
    add_tp([[0.0, 60.0], [0.0, 60.0], [1.0, 61.0]], tm())

    # ---- size 为 0 / 极小：原样返回 ----
    add_tp(LINE, tm(size=0.0))
    add_tp(LINE, tm(size=1e-13, length=0.0))

    # ---- 尖刺：length = 0 ----
    for side in ["alt", "left", "right"]:
        add_tp(LINE, tm(shape="triangle", side=side, wrap="simple", length=0.0))
        add_tp(LINE, tm(shape="square", side=side, wrap="wrap", length=0.0))
    add_tp(LINE, tm(shape="circle", side="random", wrap="wrap", length=0.0, seed=3))
    add_tp(LINE, tm(shape="parabola", side="random", wrap="simple", length=0.0, seed=8))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="simple", length=0.0, fit=True))
    add_tp(POLY, tm(shape="triangle", side="left", wrap="wrap", length=0.0, ease=0.2))

    # ---- ease ----
    add_tp(LINE, tm(shape="triangle", side="alt", ease=0.2))
    add_tp(LINE, tm(shape="circle", side="left", wrap="wrap", ease=0.5))
    add_tp(LINE, tm(shape="square", side="right", wrap="simple", ease=0.05, length=0.3))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="wrap", ease=0.15))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", ease=0.3))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", ease=0.3, fit=True))
    add_tp(LINE, tm(shape="parabola", side="random", wrap="wrap", ease=1.0, seed=11))

    # ---- fit ----
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True))
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True, dist=0.3))
    add_tp(LINE, tm(shape="circle", side="random", fit=True, seed=5))
    add_tp(LINE, tm(shape="square", side="right", wrap="wrap", fit=True, dist=1.0 / 3.0))
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True, dist=1.0 / 5.0))
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True, start=0.2, end=0.9))
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True, ease=0.1))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="wrap", fit=True, dist=0.4))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", fit=True))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", fit=True, dist=1.0 / 7.0))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="simple", fit=True, dist=1.0 / 3.0))
    add_tp(CIRCLE, tm(shape="triangle", side="left", wrap="wrap", fit=True))
    add_tp(CIRCLE, tm(shape="square", side="random", wrap="wrap", fit=True, seed=21))
    add_tp(SQUARE_LOOP, tm(shape="triangle", side="alt", wrap="wrap", fit=True))
    add_tp(SQUARE_LOOP, tm(shape="triangle", side="alt", wrap="simple", fit=True, dist=0.5))
    add_tp(SQUARE_LOOP, tm(shape="circle", side="right", wrap="wrap", fit=True, dist=0.2))

    # ---- 闭合环（非 fit） ----
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap"))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="simple"))
    add_tp(CIRCLE, tm(shape="circle", side="left", wrap="wrap"))
    add_tp(CIRCLE, tm(shape="triangle", side="random", wrap="wrap", seed=42))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", start=0.25, end=0.75))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", start=0.75, end=0.25))
    add_tp(SQUARE_LOOP, tm(shape="square", side="left", wrap="wrap"))
    add_tp(SQUARE_LOOP, tm(shape="triangle", side="alt", wrap="simple", dist=0.3))

    # ---- mirror ----
    add_tp(LINE, tm(shape="triangle", side="alt", mirror=True))
    add_tp(LINE, tm(shape="triangle", side="right", mirror=True, wrap="wrap"))
    add_tp(LINE, tm(shape="triangle", side="random", mirror=True, seed=9))
    add_tp(LINE, tm(shape="circle", side="alt", mirror=True, length=0.0))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", mirror=True, wrap="wrap", fit=True))

    # ---- start / end 区间（含半段与零长） ----
    add_tp(LINE, tm(shape="triangle", side="alt", start=0.2, end=0.8))
    add_tp(LINE, tm(shape="triangle", side="alt", start=0.8, end=0.2))
    add_tp(LINE, tm(shape="triangle", side="alt", start=0.3, end=0.3))
    add_tp(LINE, tm(shape="triangle", side="left", wrap="wrap", start=0.0, end=0.5))
    add_tp(LINE, tm(shape="triangle", side="right", wrap="simple", start=0.5, end=1.0))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="wrap", start=0.1, end=0.9))
    add_tp(TINY, tm(start=0.0, end=0.5))
    add_tp(LINE, tm(shape="circle", side="alt", start=0.25, end=0.25, length=0.0))

    # ---- k ≠ 1 ----
    add_tp(LINE, tm(shape="triangle", side="alt", k=2.0))
    add_tp(LINE, tm(shape="triangle", side="alt", k=0.5))
    add_tp(LINE, tm(shape="circle", side="alt", k=1.0, wrap="wrap"))
    add_tp(LINE, tm(shape="triangle", side="random", k=0.0625, seed=4))
    add_tp(POLY, tm(shape="triangle", side="alt", k=2.0, wrap="wrap", ease=0.1))

    # ---- 尺寸 / 距离 / 长度 ----
    add_tp(LINE, tm(shape="triangle", side="alt", size=-2.0))
    add_tp(LINE, tm(shape="circle", side="alt", size=8.0, wrap="wrap"))
    add_tp(LINE, tm(shape="triangle", side="alt", dist=0.5))
    add_tp(LINE, tm(shape="triangle", side="alt", dist=0.01))
    add_tp(LINE, tm(shape="triangle", side="alt", wrap="wrap", dist=0.01))
    add_tp(LINE, tm(shape="square", side="alt", length=0.5))
    add_tp(LINE, tm(shape="triangle", side="alt", dist=0.3, length=0.5))
    add_tp(LINE, tm(shape="circle", side="alt", dist=0.3, length=0.5, wrap="wrap"))
    add_tp(POLY, tm(shape="triangle", side="alt", dist=0.2, length=0.1, wrap="wrap"))
    add_tp(LINE, tm(shape="triangle", side="alt", dist=0.0625, length=0.0625))

    # ---- rot / slant (1.2.0) ----
    for rot in [45.0, 90.0, 180.0, -30.0, -180.0, 179.9]:
        add_tp(LINE, tm(shape="triangle", side="alt", rot=rot))
    add_tp(LINE, tm(shape="square", side="alt", rot=45.0))
    add_tp(LINE, tm(shape="circle", side="left", wrap="wrap", rot=30.0))
    add_tp(POLY, tm(shape="triangle", side="random", wrap="simple", rot=-60.0, seed=3))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", rot=90.0, fit=True))
    add_tp(LINE, tm(shape="triangle", side="alt", length=0.0, rot=45.0))
    add_tp(LINE, tm(shape="square", side="alt", length=0.0, rot=120.0))
    add_tp(LINE, tm(shape="square", side="alt", slant=0.5))
    add_tp(LINE, tm(shape="square", side="alt", slant=-1.0))
    add_tp(LINE, tm(shape="square", side="alt", slant=1.0, rot=45.0))
    add_tp(POLY, tm(shape="square", side="right", wrap="wrap", slant=0.25))
    add_tp(LINE, tm(shape="triangle", side="alt", slant=0.5))

    # ---- graphs in tumour_path (1.2.0) ----
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"size": [[0, 0.0], [1, 2.0]]}))
    add_tp(LINE, tm(shape="circle", side="left", wrap="wrap", graphs={"size": [[0, 0.5], [0.5, 2.0], [1, 0.5]]}))
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"length": [[0, 0.0], [0.5, 1.0], [1, 0.0]]}))
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"rot": [[0, -1.0], [1, 1.0]]}))
    add_tp(LINE, tm(shape="square", side="alt", graphs={"slant": [[0, 0.0], [1, 1.0]]}))
    add_tp(LINE, tm(shape="triangle", side="alt", fit=True, graphs={"dist": [[0, 2.0], [1, 0.5]]}))
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"dist": [[0, 0.5], [1, 2.0]]}))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="wrap", graphs={"size": [[0, 1.0], [1, 3.0]]}))
    add_tp(POLY, tm(shape="triangle", side="alt", wrap="wrap", graphs={"dist": [[0, 1.5], [1, 0.5]]}))
    add_tp(CIRCLE, tm(shape="triangle", side="alt", wrap="wrap", fit=True,
                      graphs={"size": [[0, 1.0], [1, 2.0]], "rot": [[0, 1.0], [1, -1.0]]}))
    add_tp(LINE, tm(shape="square", side="alt", rot=30.0,
                    graphs={"size": [[0, 1.0], [1, 2.0]], "length": [[0, 1.0], [0.5, 0.5], [1, 1.0]],
                            "rot": [[0, 0.5], [1, 1.5]], "slant": [[0, 0.5], [1, 1.0]],
                            "dist": [[0, 1.0], [1, 2.0]]}))
    add_tp(POLY, tm(shape="circle", side="random", wrap="wrap", seed=13, rot=-45.0,
                    graphs={"size": [[0, 2.0], [1, 0.5]], "length": [[0, 1.0], [1, 0.0]]}))
    # (a graph that only ever says 100 % is dropped: same as no graph)
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"size": [[0, 1.0], [1, 1.0]]}))
    # (0 graph value: no size / no length)
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"size": [[0, 0.0], [1, 0.0]]}))
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"length": [[0, 0.0], [1, 0.0]]}))
    # (negative graph value)
    add_tp(LINE, tm(shape="triangle", side="alt", graphs={"size": [[0, -1.0], [1, 1.0]]}))

    # ---- split_tumour (1.2.0) ----
    SPLIT_POLY = [[0.0, 60.0], [0.5, 62.0], [1.0, 60.0], [1.5, 63.0]]
    for raw, path, cut in [
        (tm(shape="triangle", side="alt"), SPLIT_POLY, 2),
        (tm(shape="triangle", side="alt", fit=True, dist=0.3), SPLIT_POLY, 1),
        (tm(shape="square", side="random", seed=5, wrap="wrap"), SPLIT_POLY, 3),
        (tm(shape="triangle", side="alt", start=0.1, end=0.9), SPLIT_POLY, 2),
        (tm(shape="triangle", side="alt", graphs={"size": [[0, 0.5], [1, 2.0]],
                                                   "dist": [[0, 1.0], [1, 2.0]]}), SPLIT_POLY, 2),
        (tm(shape="triangle", side="alt", graphs={"rot": [[0, -1.0], [1, 1.0]],
                                                   "length": [[0, 1.0], [0.5, 0.5], [1, 1.0]]}), SPLIT_POLY, 2),
        (tm(shape="circle", side="left", wrap="wrap", rot=30.0), CIRCLE, 5),
        (tm(shape="triangle", side="alt", length=0.0), SPLIT_POLY, 2),
    ]:
        cleaned = T.clean_tumour(raw)
        left, right = path[:cut], path[cut - 1:]
        add("split_tumour", [cleaned, left, right], list(T.split_tumour(cleaned, left, right)))

    # ---- 随机压力：固定种子，形状 / 方向 / 包裹 / 区间 / 缓动 / fit 混着来 ----
    rnd = random.Random(12345)
    shapes = ["triangle", "square", "circle", "parabola"]
    sides = ["alt", "left", "right", "random"]
    wraps = ["simple", "wrap"]

    def rand_path():
        kind = rnd.choice(["line", "poly", "circle", "square_loop", "back"])
        if kind == "line":
            x0, y0 = rnd.uniform(0, 1), rnd.uniform(58, 62)
            x1, y1 = x0 + rnd.uniform(0.3, 2), y0 + rnd.uniform(-3, 3)
            return [[x0, y0], [x1, y1]]
        if kind == "poly":
            t, y = rnd.uniform(0, 1), rnd.uniform(58, 62)
            pts = [[t, y]]
            for _ in range(rnd.randint(2, 5)):
                t += rnd.uniform(0.1, 0.6) * rnd.choice([1, -1])
                y += rnd.uniform(-2, 2)
                pts.append([round(t, 4), round(y, 4)])
            return pts
        if kind == "circle":
            cx, cy = rnd.uniform(0.5, 1.5), rnd.uniform(59, 61)
            r = rnd.uniform(0.2, 0.8)
            return [
                [cx + r * math.cos(2 * math.pi * i / 12), cy + r * math.sin(2 * math.pi * i / 12)]
                for i in range(13)
            ]
        if kind == "square_loop":
            x, y, w, h = rnd.uniform(0, 1), rnd.uniform(58, 62), rnd.uniform(0.3, 1), rnd.uniform(0.5, 3)
            return [[x, y], [x + w, y], [x + w, y + h], [x, y + h], [x, y]]
        return [[0.0, 60.0], [rnd.uniform(0.2, 1), 61.0], [0.0, rnd.uniform(60.5, 62)]]

    for _ in range(80):
        graphs = {}
        if rnd.random() < 0.45:
            key = rnd.choice(["size", "length", "dist", "rot", "slant"])
            graphs[key] = [[0.0, rnd.choice([0.5, 1.0, 2.0])],
                           [round(rnd.uniform(0, 1), 3), rnd.choice([0.0, 0.5, 1.5, 3.0])],
                           [1.0, rnd.choice([0.5, 1.0, 2.0])]]
        raw = tm(
            shape=rnd.choice(shapes),
            side=rnd.choice(sides),
            wrap=rnd.choice(wraps),
            size=rnd.choice([3.0, -2.0, 5.0, 0.5]),
            length=rnd.choice([0.0, 0.05, 0.125, 0.25, 0.4]),
            dist=rnd.choice([0.05, 0.1, 0.125, 0.2, 0.3]),
            start=round(rnd.uniform(0, 1), 3),
            end=round(rnd.uniform(0, 1), 3),
            ease=rnd.choice([0.0, 0.0, 0.05, 0.2, 0.5]),
            rot=rnd.choice([0.0, 0.0, 30.0, -45.0, 90.0, 180.0]),
            slant=rnd.choice([0.0, 0.0, 0.5, -1.0]),
            fit=rnd.random() < 0.35,
            seed=rnd.randint(0, 1000),
            mirror=rnd.random() < 0.3,
            k=rnd.choice([0.125, 0.25, 0.5, 1.0, 2.0]),
            graphs=graphs,
        )
        add_tp(rand_path(), raw)

    return {"module": "tumour", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("tumour", data["cases"])
