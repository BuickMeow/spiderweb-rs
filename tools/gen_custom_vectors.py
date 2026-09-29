"""custom.py 的对照向量。

Python 原版（notes/custom.py）直接跑，输出写到
crates/spiderweb-core/tests/vectors/custom.json，Rust 测试逐用例对照。
打包音符（pack_notes）因为压缩实现不同，向量给出 Python 的文本，
Rust 侧验证两边解出的行完全一致（zlib 流本身互通）。

用 1.2.0 的源码生成（ends / union / apart / fill_plan / chop 等新语义）：
    SPIDERWEB_SRC=/path/to/Spiderweb-1.2.0/scripts python3 tools/gen_custom_vectors.py
"""

import copy
import random

import numpy as np

from vec_common import write

from notes import custom as C


def tolist(x):
    if isinstance(x, dict):
        return {str(k): tolist(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [tolist(v) for v in x]
    if isinstance(x, np.ndarray):
        return x.tolist()
    if isinstance(x, np.integer):
        return int(x)
    if isinstance(x, np.floating):
        return float(x)
    if isinstance(x, np.bool_):
        return bool(x)
    return x


# ---------------------------------------------------------------- 构造小工具

def poly(pts, free=False, smooth=0, k=1.0):
    d = {"kind": "poly", "pts": [[float(u), float(v)] for u, v in pts]}
    if free:
        d.update(free=True, smooth=smooth, k=k)
    return d


def curve(pts, sharp=None, sym=None):
    d = {"kind": "curve", "pts": [[float(u), float(v)] for u, v in pts]}
    if sharp:
        d["sharp"] = list(sharp)
    if sym:
        d["sym"] = sym
    return d


def arc(pts, k=1.0):
    return {"kind": "arc", "pts": [[float(u), float(v)] for u, v in pts], "k": k}


def ell(box):
    return {"kind": "ellipse", "box": [float(x) for x in box]}


def shape(strokes, pts=None, fill="empty", gate=0.0625, align="auto", ends=None, union=None, apart=None,
          notes=None, own_vel=False, text=None, vel0=None, vel1=None, end_dot=None):
    if pts is None:
        pts = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    d = {"kind": "custom", "pts": [[float(b), float(p)] for b, p in pts],
         "strokes": [copy.deepcopy(s) for s in strokes], "fill": fill, "gate": gate, "align": align}
    if ends is not None:
        d["ends"] = ends
    if union is not None:
        d["union"] = bool(union)
    if apart is not None:
        d["apart"] = bool(apart)
    if notes is not None:
        d["notes"] = notes
    if own_vel:
        d["own_vel"] = True
    if text is not None:
        d["text"] = dict(text)
    if vel0 is not None:
        d["vel0"] = vel0
    if vel1 is not None:
        d["vel1"] = vel1
    if end_dot is not None:
        d["end_dot"] = end_dot
    return d


def mapfn(name, p):
    if name == "shift":
        return lambda u, v: (u + p[0], v + p[1])
    if name == "scale":
        return lambda u, v: (u * p[0], v * p[1])
    if name == "shear":
        return lambda u, v: (u + p[0] * v, v + p[1] * u)
    if name == "mix":
        return lambda u, v: (0.5 * u + 0.2 * v + 0.1, -0.3 * u + 0.7 * v - 0.05)
    raise ValueError(name)


SQUARE = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
TRIANGLE = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.0, 0.0]]


def gen():
    cases = []
    rnd = random.Random(11)

    def add(fn, args, out):
        cases.append({"fn": fn, "args": tolist(args), "out": tolist(out)})

    # ------------------------------------------------------------ stroke_points
    add("stroke_points", [poly(SQUARE)], C.stroke_points(poly(SQUARE)))
    add("stroke_points", [poly([[0.0, 0.0], [1.0, 1.0], [0.5, 0.2]])],
        C.stroke_points(poly([[0.0, 0.0], [1.0, 1.0], [0.5, 0.2]])))
    # 自由笔画：smooth 0 保持原样，> 0 画整齐
    add("stroke_points", [poly([[0.0, 0.0], [0.1, 0.04], [0.2, 0.0], [0.3, 0.03], [0.4, 0.0], [0.5, 0.0]],
                               free=True, smooth=0, k=1.0)],
        C.stroke_points(poly([[0.0, 0.0], [0.1, 0.04], [0.2, 0.0], [0.3, 0.03], [0.4, 0.0], [0.5, 0.0]],
                             free=True, smooth=0, k=1.0)))
    add("stroke_points", [poly([[0.0, 0.0], [0.1, 0.04], [0.2, 0.0], [0.3, 0.03], [0.4, 0.0], [0.5, 0.0]],
                               free=True, smooth=60, k=1.0)],
        C.stroke_points(poly([[0.0, 0.0], [0.1, 0.04], [0.2, 0.0], [0.3, 0.03], [0.4, 0.0], [0.5, 0.0]],
                             free=True, smooth=60, k=1.0)))
    add("stroke_points", [poly([[0.0, 0.0], [0.1, 0.2], [0.2, 0.1], [0.3, 0.3], [0.4, 0.2], [0.5, 0.5]],
                               free=True, smooth=35, k=2.0)],
        C.stroke_points(poly([[0.0, 0.0], [0.1, 0.2], [0.2, 0.1], [0.3, 0.3], [0.4, 0.2], [0.5, 0.5]],
                             free=True, smooth=35, k=2.0)))
    # 曲线：一段 / 两段
    add("stroke_points", [curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0]])],
        C.stroke_points(curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0]])))
    add("stroke_points", [curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0],
                                 [0.8, -0.4], [0.2, -0.4], [0.0, 0.0]])],
        C.stroke_points(curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0],
                               [0.8, -0.4], [0.2, -0.4], [0.0, 0.0]])))
    # 弧：k = 1 半圆、k = 2 椭圆弧、共线（直线）、整圆
    add("stroke_points", [arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=1.0)],
        C.stroke_points(arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=1.0)))
    add("stroke_points", [arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0)],
        C.stroke_points(arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0)))
    add("stroke_points", [arc([[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]], k=1.0)],
        C.stroke_points(arc([[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]], k=1.0)))
    add("stroke_points", [arc([[0.5, 0.0], [0.5, 1.0], [0.5, 0.0]], k=1.0)],
        C.stroke_points(arc([[0.5, 0.0], [0.5, 1.0], [0.5, 0.0]], k=1.0)))
    # 椭圆
    add("stroke_points", [ell([0.0, 0.0, 1.0, 1.0])], C.stroke_points(ell([0.0, 0.0, 1.0, 1.0])))
    add("stroke_points", [ell([0.2, -0.1, 0.8, 0.9])], C.stroke_points(ell([0.2, -0.1, 0.8, 0.9])))
    add("stroke_points", [ell([0.0, 0.5, 1.0, 0.5])], C.stroke_points(ell([0.0, 0.5, 1.0, 0.5])))

    # ------------------------------------------------------------ clean_strokes
    add("clean_strokes", [None], C.clean_strokes(None))
    add("clean_strokes", [[]], C.clean_strokes([]))
    add("clean_strokes", [[poly(SQUARE), ell([0, 0, 1, 1]), curve([[0, 0], [0.3, 0.3], [0.6, 0.3], [1, 1]]),
                           arc([[0, 0], [0.5, 0.5], [1, 0]]), poly([[0, 0], [1, 1]], free=True, smooth=50, k=3)]],
        C.clean_strokes([poly(SQUARE), ell([0, 0, 1, 1]), curve([[0, 0], [0.3, 0.3], [0.6, 0.3], [1, 1]]),
                         arc([[0, 0], [0.5, 0.5], [1, 0]]), poly([[0, 0], [1, 1]], free=True, smooth=50, k=3)]))
    add("clean_strokes", [[{"kind": "ellipse", "box": [1, 2, 3]}]], C.clean_strokes([{"kind": "ellipse", "box": [1, 2, 3]}]))
    add("clean_strokes", [[{"kind": "ellipse", "box": [1, 2, 3, 4]}]],
        C.clean_strokes([{"kind": "ellipse", "box": [1, 2, 3, 4]}]))
    add("clean_strokes", [[{"kind": "ellipse", "box": "1234"}]],
        C.clean_strokes([{"kind": "ellipse", "box": "1234"}]))
    add("clean_strokes", [[{"kind": "ellipse", "box": ["1", "2", "3", "4"]}]],
        C.clean_strokes([{"kind": "ellipse", "box": ["1", "2", "3", "4"]}]))
    add("clean_strokes", [[{"kind": "ellipse"}]], C.clean_strokes([{"kind": "ellipse"}]))
    add("clean_strokes", [[{"kind": "ellipse", "box": [1, "x", 3, 4]}]],
        C.clean_strokes([{"kind": "ellipse", "box": [1, "x", 3, 4]}]))
    add("clean_strokes", [[{"kind": "curve"}]], C.clean_strokes([{"kind": "curve"}]))
    add("clean_strokes", [[{"kind": "curve", "pts": [[0, 0], [1, 0]]}]],
        C.clean_strokes([{"kind": "curve", "pts": [[0, 0], [1, 0]]}]))
    add("clean_strokes", [[{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0]]}]],
        C.clean_strokes([{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0]]}]))
    add("clean_strokes", [[{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                            "sharp": [1, 2, 3], "sym": "mirror"}]],
        C.clean_strokes([{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                          "sharp": [1, 2, 3], "sym": "mirror"}]))
    add("clean_strokes", [[{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                            "sharp": [1, "x"]}]],
        C.clean_strokes([{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                          "sharp": [1, "x"]}]))
    add("clean_strokes", [[{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                            "sharp": "12"}]],
        C.clean_strokes([{"kind": "curve", "pts": [[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]],
                          "sharp": "12"}]))
    add("clean_strokes", [[{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": "2"}]],
        C.clean_strokes([{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": "2"}]))
    add("clean_strokes", [[{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": "x"}]],
        C.clean_strokes([{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": "x"}]))
    add("clean_strokes", [[{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": 0}]],
        C.clean_strokes([{"kind": "arc", "pts": [[0, 0], [1, 1], [2, 0]], "k": 0}]))
    add("clean_strokes", [[{"kind": "arc", "pts": [[0, 0], [1, 1]]}]],
        C.clean_strokes([{"kind": "arc", "pts": [[0, 0], [1, 1]]}]))
    add("clean_strokes", [[{"pts": [[0, 0], [1, 1]]}]], C.clean_strokes([{"pts": [[0, 0], [1, 1]]}]))
    add("clean_strokes", [[{"pts": [[0, 0], [1, 1]], "free": 1, "smooth": "80", "k": "0.5"}]],
        C.clean_strokes([{"pts": [[0, 0], [1, 1]], "free": 1, "smooth": "80", "k": "0.5"}]))
    add("clean_strokes", [[{"pts": [[0, 0], [1, 1]], "free": True, "smooth": None}]],
        C.clean_strokes([{"pts": [[0, 0], [1, 1]], "free": True, "smooth": None}]))
    add("clean_strokes", [[{"pts": [["0", "1"], ["2", "3"]]}]],
        C.clean_strokes([{"pts": [["0", "1"], ["2", "3"]]}]))
    add("clean_strokes", [[{"pts": [[0, 0], [1, 1, 2]]}]],
        C.clean_strokes([{"pts": [[0, 0], [1, 1, 2]]}]))
    add("clean_strokes", [[]], C.clean_strokes([[]]))
    add("clean_strokes", [[42, "x", None]], C.clean_strokes([42, "x", None]))
    add("clean_strokes", [[{"pts": []}]], C.clean_strokes([{"pts": []}]))

    # ------------------------------------------------------------ clean_curve
    pts7 = [[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [1.0, 0.0], [0.8, -0.3], [0.2, -0.3], [0.0, 0.0]]
    add("clean_curve", [{"sharp": [1, 2, 3], "sym": "mirror"}, pts7], C.clean_curve({"sharp": [1, 2, 3], "sym": "mirror"}, pts7))
    add("clean_curve", [{"sharp": [1], "sym": "turn"}, pts7], C.clean_curve({"sharp": [1], "sym": "turn"}, pts7))
    add("clean_curve", [{"sharp": [True, 2], "sym": "turn"}, pts7], C.clean_curve({"sharp": [True, 2], "sym": "turn"}, pts7))
    add("clean_curve", [{"sharp": None}, pts7], C.clean_curve({"sharp": None}, pts7))
    add("clean_curve", [{"sharp": "12", "sym": "mirror"}, pts7], C.clean_curve({"sharp": "12", "sym": "mirror"}, pts7))
    add("clean_curve", [{"sym": "mirror"}, pts7[:4]], C.clean_curve({"sym": "mirror"}, pts7[:4]))
    add("clean_curve", [{"sym": "turn"}, pts7[:4]], C.clean_curve({"sym": "turn"}, pts7[:4]))
    add("clean_curve", [{"sharp": [2], "sym": "bogus"}, pts7], C.clean_curve({"sharp": [2], "sym": "bogus"}, pts7))

    # ------------------------------------------------------------ path_closed / stroke_closed
    add("path_closed", [[[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]], C.path_closed([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]))
    add("path_closed", [[[0.0, 0.0], [1.0, 0.0], [0.0, 1e-7]]], C.path_closed([[0.0, 0.0], [1.0, 0.0], [0.0, 1e-7]]))
    add("path_closed", [[[0.0, 0.0], [0.0, 0.0]]], C.path_closed([[0.0, 0.0], [0.0, 0.0]]))
    add("stroke_closed", [ell([0, 0, 1, 1])], C.stroke_closed(ell([0, 0, 1, 1])))
    add("stroke_closed", [poly(SQUARE)], C.stroke_closed(poly(SQUARE)))
    add("stroke_closed", [poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])],
        C.stroke_closed(poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])))
    add("stroke_closed", [curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [0.0, 0.0]])],
        C.stroke_closed(curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [0.0, 0.0]])))
    add("stroke_closed", [arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]])],
        C.stroke_closed(arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]])))

    # ------------------------------------------------------------ join_paths
    A, B, Cc, D = [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]
    add("join_paths", [[[A, B], [B, Cc]]], C.join_paths([[A, B], [B, Cc]]))
    add("join_paths", [[[A, B], [Cc, B]]], C.join_paths([[A, B], [Cc, B]]))
    add("join_paths", [[[A, B], [A, Cc]]], C.join_paths([[A, B], [A, Cc]]))
    add("join_paths", [[[A, B], [B, Cc], [Cc, D], [D, A]]], C.join_paths([[A, B], [B, Cc], [Cc, D], [D, A]]))
    add("join_paths", [[[A, B], [A, B], [Cc, D]]], C.join_paths([[A, B], [A, B], [Cc, D]]))
    add("join_paths", [[[A, B, A], [Cc, D]]], C.join_paths([[A, B, A], [Cc, D]]))
    near_b = [B[0] + 1e-7, B[1]]
    add("join_paths", [[[A, B], [near_b, Cc]]], C.join_paths([[A, B], [near_b, Cc]]))
    add("join_paths", [[[A, B, Cc], [D, A]]], C.join_paths([[A, B, Cc], [D, A]]))
    one = [[0.5, 0.5]]
    add("join_paths", [[[A, B], [[1.0, 0.0]]]], C.join_paths([[A, B], [[1.0, 0.0]]]))
    add("join_paths", [[one, [one[0], B]]], C.join_paths([one, [one[0], B]]))
    add("join_paths", [[[A, B], [[0.0, 0.0]]]], C.join_paths([[A, B], [[0.0, 0.0]]]))
    add("join_paths", [[[A, B, A], [[1.0, 1.0]]]], C.join_paths([[A, B, A], [[1.0, 1.0]]]))

    # ------------------------------------------------------------ stroke_span / open_paths / open_ends / closed / fillable
    tri_lines = [[A, B], [B, Cc], [Cc, A]]
    tri_strokes = [poly(p) for p in tri_lines]
    add("stroke_span", [poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])],
        C.stroke_span(poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])))
    add("stroke_span", [poly(SQUARE)], C.stroke_span(poly(SQUARE)))
    add("stroke_span", [ell([0, 0, 1, 1])], C.stroke_span(ell([0, 0, 1, 1])))
    add("stroke_span", [curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [1.0, 1.0]])],
        C.stroke_span(curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [1.0, 1.0]])))
    add("stroke_span", [curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [0.0, 0.0]])],
        C.stroke_span(curve([[0.0, 0.0], [0.2, 0.3], [0.8, 0.3], [0.0, 0.0]])))
    add("stroke_span", [arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]])],
        C.stroke_span(arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]])))

    open_line = poly([[0.0, 0.0], [1.0, 0.5], [2.0, 0.0]])
    gap_shape = poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
    two_gaps = [poly([[0.0, 0.0], [1.0, 1.0]]), poly([[0.0, 1.0], [1.0, 0.0]])]
    add("open_paths", [[open_line]], C.open_paths([open_line]))
    add("open_paths", [[poly(SQUARE)]], C.open_paths([poly(SQUARE)]))
    add("open_paths", [[ell([0, 0, 1, 1])]], C.open_paths([ell([0, 0, 1, 1])]))
    add("open_paths", [tri_strokes], C.open_paths(tri_strokes))
    add("open_paths", [[gap_shape]], C.open_paths([gap_shape]))
    add("open_paths", [two_gaps], C.open_paths(two_gaps))
    add("open_ends", [[open_line]], C.open_ends([open_line]))
    add("open_ends", [tri_strokes], C.open_ends(tri_strokes))
    add("open_ends", [two_gaps], C.open_ends(two_gaps))
    add("open_ends", [[poly(SQUARE), open_line]], C.open_ends([poly(SQUARE), open_line]))
    add("strokes_closed", [[poly(SQUARE)]], C.strokes_closed([poly(SQUARE)]))
    add("strokes_closed", [[poly(SQUARE), ell([0, 0, 1, 1])]], C.strokes_closed([poly(SQUARE), ell([0, 0, 1, 1])]))
    add("strokes_closed", [tri_strokes], C.strokes_closed(tri_strokes))
    add("strokes_closed", [[]], C.strokes_closed([]))
    add("fillable", [[gap_shape]], C.fillable([gap_shape]))
    add("fillable", [two_gaps], C.fillable(two_gaps))
    add("fillable", [[poly(SQUARE)]], C.fillable([poly(SQUARE)]))
    add("fillable", [[]], C.fillable([]))

    # ------------------------------------------------------------ near_ends / flat_path / fill_plan / gap_lines
    add("near_ends", [[0.0, 60.0], [1 / 64, 61.0]], C.near_ends([0.0, 60.0], [1 / 64, 61.0]))
    add("near_ends", [[0.0, 60.0], [1 / 64 + 1e-6, 61.0]], C.near_ends([0.0, 60.0], [1 / 64 + 1e-6, 61.0]))
    add("near_ends", [[0.0, 60.0], [0.0, 61.0000001]], C.near_ends([0.0, 60.0], [0.0, 61.0000001]))
    add("flat_path", [[[0.0, 60.0], [2.0, 60.0]]], C.flat_path([[0.0, 60.0], [2.0, 60.0]]))
    add("flat_path", [[[0.0, 60.0], [1.0, 62.0], [2.0, 64.0]]], C.flat_path([[0.0, 60.0], [1.0, 62.0], [2.0, 64.0]]))
    add("flat_path", [[[0.0, 60.0], [1.0, 60.4], [2.0, 61.0]]], C.flat_path([[0.0, 60.0], [1.0, 60.4], [2.0, 61.0]]))
    add("flat_path", [[[0.0, 60.0], [1.0, 62.0], [1.5, 61.0], [2.0, 64.0]]],
        C.flat_path([[0.0, 60.0], [1.0, 62.0], [1.5, 61.0], [2.0, 64.0]]))
    add("flat_path", [[[0.0, 60.0], [0.01, 61.0]]], C.flat_path([[0.0, 60.0], [0.01, 61.0]]))
    near_touch = [poly([[0.0, 0.0], [0.5, 0.5]]), poly([[0.5 + 1e-4, 0.5], [1.0, 1.0]])]
    add("fill_plan", [shape([poly(SQUARE)])], C.fill_plan(shape([poly(SQUARE)])))
    add("fill_plan", [shape([gap_shape])], C.fill_plan(shape([gap_shape])))
    add("fill_plan", [shape(two_gaps)], C.fill_plan(shape(two_gaps)))
    add("fill_plan", [shape([poly(SQUARE), poly([[0.0, 0.5], [1.0, 0.5]])])],
        C.fill_plan(shape([poly(SQUARE), poly([[0.0, 0.5], [1.0, 0.5]])])))
    add("fill_plan", [shape([poly([[0.0, 0.0], [0.5, 0.5]]), poly([[0.5, 0.5], [0.5, 1.0], [1.0, 1.0]])])],
        C.fill_plan(shape([poly([[0.0, 0.0], [0.5, 0.5]]), poly([[0.5, 0.5], [0.5, 1.0], [1.0, 1.0]])])))
    add("fill_plan", [shape(near_touch, pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]])],
        C.fill_plan(shape(near_touch, pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]])))
    # 闭合不了的一个开放段（不是扁的）：从终点直线连回起点
    open_curve = [poly(SQUARE), poly([[0.2, 0.2], [0.5, 0.8], [0.8, 0.2]])]
    add("fill_plan", [shape(open_curve)], C.fill_plan(shape(open_curve)))
    add("gap_lines", [shape([gap_shape])], C.gap_lines(shape([gap_shape])))
    add("gap_lines", [shape([poly(SQUARE)])], C.gap_lines(shape([poly(SQUARE)])))
    add("gap_lines", [shape(two_gaps)], C.gap_lines(shape(two_gaps)))
    add("gap_lines", [shape([gap_shape], text={"threshold": 50.0, "grow": 0.0, "k": 1.0, "holes": []})],
        C.gap_lines(shape([gap_shape], text={"threshold": 50.0, "grow": 0.0, "k": 1.0, "holes": []})))
    add("gap_lines", [shape([gap_shape], pts=[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]])],
        C.gap_lines(shape([gap_shape], pts=[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]])))
    add("gap_lines", [shape([gap_shape], pts=[[0.0, 0.0], [1.0, 1.0], [-1.0, 1.0]])],
        C.gap_lines(shape([gap_shape], pts=[[0.0, 0.0], [1.0, 1.0], [-1.0, 1.0]])))

    # ------------------------------------------------------------ join_strokes
    add("join_strokes", [tri_strokes], C.join_strokes(tri_strokes))
    mixed = [poly([[0.0, 0.0], [1.0, 0.0]]), poly([[1.0, 0.0], [2.0, 1.0]], free=True, smooth=30, k=1.0),
             curve([[0.0, 2.0], [0.3, 2.3], [0.7, 2.3], [1.0, 2.0]]), ell([0.2, 0.2, 0.8, 0.8]),
             poly(SQUARE)]
    add("join_strokes", [mixed], C.join_strokes(mixed))
    add("join_strokes", [[poly([[0.0, 0.0], [1.0, 1.0]]), poly([[1.0, 1.0], [2.0, 0.0]])]],
        C.join_strokes([poly([[0.0, 0.0], [1.0, 1.0]]), poly([[1.0, 1.0], [2.0, 0.0]])]))
    add("join_strokes", [[poly(SQUARE), ell([0, 0, 1, 1])]], C.join_strokes([poly(SQUARE), ell([0, 0, 1, 1])]))

    # ------------------------------------------------------------ custom_strokes
    add("custom_strokes", [shape([poly(SQUARE), ell([0.2, 0.2, 0.8, 0.8])])],
        C.custom_strokes(shape([poly(SQUARE), ell([0.2, 0.2, 0.8, 0.8])])))
    add("custom_strokes", [shape([poly(SQUARE)], pts=[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]])],
        C.custom_strokes(shape([poly(SQUARE)], pts=[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]])))
    # 斜切 / 旋转过的框
    add("custom_strokes", [shape([poly([[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])],
                                 pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])],
        C.custom_strokes(shape([poly([[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])],
                               pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])))
    add("custom_strokes", [shape([ell([0.0, 0.0, 1.0, 1.0])], pts=[[1.0, 62.0], [1.0, 60.0], [3.0, 62.0]])],
        C.custom_strokes(shape([ell([0.0, 0.0, 1.0, 1.0])], pts=[[1.0, 62.0], [1.0, 60.0], [3.0, 62.0]])))
    # 文本形状：走 text_polys（不放大）
    tx = {"threshold": 50.0, "grow": 0.0, "k": 1.0, "holes": []}
    add("custom_strokes", [shape([curve([[0.0, 0.0], [0.3, 0.5], [0.7, 0.5], [1.0, 0.0]])], text=tx)],
        C.custom_strokes(shape([curve([[0.0, 0.0], [0.3, 0.5], [0.7, 0.5], [1.0, 0.0]])], text=tx)))

    # ------------------------------------------------------------ box_frame
    add("box_frame", [0.0, 0.0, 1.0, 1.0], C.box_frame(0.0, 0.0, 1.0, 1.0))
    add("box_frame", [1.0, 1.0, 0.0, 0.0], C.box_frame(1.0, 1.0, 0.0, 0.0))
    add("box_frame", [2.5, 60.0, -1.5, 64.0], C.box_frame(2.5, 60.0, -1.5, 64.0))
    add("box_frame", [0.5, 3.0, 2.5, 3.0], C.box_frame(0.5, 3.0, 2.5, 3.0))

    # ------------------------------------------------------------ normalize_strokes
    add("normalize_strokes", [[poly([[0.0, 0.0], [2.0, 0.0], [2.0, 4.0], [0.0, 4.0]])]],
        C.normalize_strokes([poly([[0.0, 0.0], [2.0, 0.0], [2.0, 4.0], [0.0, 4.0]])]))
    add("normalize_strokes", [[arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0)]],
        C.normalize_strokes([arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0)]))
    add("normalize_strokes", [[poly([[0.0, 0.0], [2.0, 1.0], [3.0, 0.0], [1.0, -1.0]],
                                    free=True, smooth=40, k=1.5)]],
        C.normalize_strokes([poly([[0.0, 0.0], [2.0, 1.0], [3.0, 0.0], [1.0, -1.0]],
                                  free=True, smooth=40, k=1.5)]))
    add("normalize_strokes", [[ell([1.0, 2.0, 5.0, 6.0])]], C.normalize_strokes([ell([1.0, 2.0, 5.0, 6.0])]))
    add("normalize_strokes", [[poly([[0.0, 0.5], [2.0, 0.5], [1.0, 0.5]])]],
        C.normalize_strokes([poly([[0.0, 0.5], [2.0, 0.5], [1.0, 0.5]])]))
    add("normalize_strokes", [[poly([[0.5, 0.0], [0.5, 2.0], [0.5, 1.0]])]],
        C.normalize_strokes([poly([[0.5, 0.0], [0.5, 2.0], [0.5, 1.0]])]))
    add("normalize_strokes", [[ell([0.5, 0.5, 0.5, 0.5])]], C.normalize_strokes([ell([0.5, 0.5, 0.5, 0.5])]))
    add("normalize_strokes", [[poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]])]],
        C.normalize_strokes([poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]])]))

    # ------------------------------------------------------------ frame_to_bp / frame_to_uv / uv_k / frame_upright
    frames = [
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        [[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]],
        [[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]],
        [[1.0, 60.0], [3.0, 60.0], [1.0, 60.0]],
        [[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]],
    ]
    for fr in frames:
        add("frame_to_bp", [fr, 0.25, 0.75], C.frame_to_bp(fr)(0.25, 0.75))
        add("frame_to_uv", [fr, 2.5, 61.0], C.frame_to_uv(fr)(2.5, 61.0) if C.frame_to_uv(fr) else None)
        add("uv_k", [fr, 1.0], C.uv_k(fr, 1.0))
        add("uv_k", [fr, 2.0], C.uv_k(fr, 2.0))
        add("frame_upright", [fr], C.frame_upright(fr))

    # ------------------------------------------------------------ map_stroke
    nonfree = dict(poly(SQUARE))
    nonfree["k"] = 3.0
    add("map_stroke", ["shift", [1.0, 0.0], nonfree, 2.0, 0.5], C.map_stroke(nonfree, mapfn("shift", [1.0, 0.0]), 2.0, 0.5))
    maps = [
        ("shift", [0.25, -0.4]),
        ("scale", [2.0, 0.5]),
        ("shear", [0.3, -0.2]),
        ("mix", []),
    ]
    map_strokes = [poly(SQUARE), arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0),
                   curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0]]),
                   ell([0.1, 0.2, 0.7, 0.9]), poly([[0.0, 0.0], [1.0, 1.0]], free=True, smooth=30, k=1.5)]
    for mname, mp in maps:
        fn = mapfn(mname, mp)
        for st in map_strokes:
            add("map_stroke", [mname, mp, st, 2.0, 0.5], C.map_stroke(st, fn, 2.0, 0.5))

    # ------------------------------------------------------------ refit
    add("refit", [shape([poly([[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]])])],
        (lambda sh: (C.refit(sh), sh)[1])(shape([poly([[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]])])))
    add("refit", [shape([poly([[0.0, 0.0], [2.0, 0.0], [2.0, 4.0], [0.0, 4.0]])],
                        pts=[[2.0, 60.0], [2.0, 60.0], [2.0, 60.0]])],
        (lambda sh: (C.refit(sh), sh)[1])(shape([poly([[0.0, 0.0], [2.0, 0.0], [2.0, 4.0], [0.0, 4.0]])],
                                                pts=[[2.0, 60.0], [2.0, 60.0], [2.0, 60.0]])))
    add("refit", [shape([poly([[0.0, 0.5], [1.0, 0.5], [2.0, 0.5]])],
                        pts=[[1.0, 60.0], [3.0, 60.0], [1.0, 62.0]])],
        (lambda sh: (C.refit(sh), sh)[1])(shape([poly([[0.0, 0.5], [1.0, 0.5], [2.0, 0.5]])],
                                                pts=[[1.0, 60.0], [3.0, 60.0], [1.0, 62.0]])))
    add("refit", [shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])],
        (lambda sh: (C.refit(sh), sh)[1])(shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])))
    add("refit", [shape([poly(SQUARE)])], (lambda sh: (C.refit(sh), sh)[1])(shape([poly(SQUARE)])))

    # ------------------------------------------------------------ stroke_ends
    add("stroke_ends", [[poly(SQUARE)]], C.stroke_ends([poly(SQUARE)]))
    add("stroke_ends", [[curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0]])]],
        C.stroke_ends([curve([[0.0, 0.0], [0.2, 0.4], [0.8, 0.4], [1.0, 0.0]])]))
    add("stroke_ends", [[arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]]), ell([0, 0, 1, 1])]],
        C.stroke_ends([arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]]), ell([0, 0, 1, 1])]))
    add("stroke_ends", [[ell([0, 0, 1, 1])]], C.stroke_ends([ell([0, 0, 1, 1])]))

    # ------------------------------------------------------------ bp_k / stroke_bp
    frames_bp = [[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                 [[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]],
                 [[1.0, 60.0], [3.0, 60.0], [1.0, 62.0]]]
    for fr in frames_bp:
        for k in [0.5, 1.0, 2.0, 3.0]:
            add("bp_k", [fr, k], C.bp_k(fr, k))
    bp_shape = shape([poly(SQUARE), arc([[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]], k=2.0),
                      poly([[0.0, 0.0], [1.0, 1.0]], free=True, smooth=30, k=1.5),
                      ell([0.1, 0.2, 0.7, 0.9])], pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])
    for k in range(4):
        add("stroke_bp", [bp_shape, k], C.stroke_bp(bp_shape, k))

    # ------------------------------------------------------------ add_stroke
    def add_case(sh, st, at=None):
        target = copy.deepcopy(sh)
        k = C.add_stroke(target, st, at)
        return [k, target]

    base = shape([poly([[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.0, 1.0]])])
    add("add_stroke", [base, poly([[1.0, 0.0], [1.5, 1.0]])],
        add_case(base, poly([[1.0, 0.0], [1.5, 1.0]])))
    add("add_stroke", [base, poly([[0.5, 0.0], [1.0, 0.5]])],
        add_case(base, poly([[0.5, 0.0], [1.0, 0.5]])))
    add("add_stroke", [base, curve([[0.5, 0.5], [0.6, 0.6], [0.8, 0.6], [1.0, 0.5]])],
        add_case(base, curve([[0.5, 0.5], [0.6, 0.6], [0.8, 0.6], [1.0, 0.5]])))
    add("add_stroke", [base, ell([0.2, 0.2, 0.8, 0.8])], add_case(base, ell([0.2, 0.2, 0.8, 0.8])))
    add("add_stroke", [base, poly([[0.5, 1.0], [1.0, 0.0], [0.5, 1.0]])],
        add_case(base, poly([[0.5, 1.0], [1.0, 0.0], [0.5, 1.0]])))
    add("add_stroke", [base, poly([[0.5, 0.5], [1.0, 0.5], [1.5, 0.5]], free=True, smooth=40, k=2.0)],
        add_case(base, poly([[0.5, 0.5], [1.0, 0.5], [1.5, 0.5]], free=True, smooth=40, k=2.0)))
    rot = shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]])
    add("add_stroke", [rot, ell([0.2, 0.2, 0.8, 0.8])], add_case(rot, ell([0.2, 0.2, 0.8, 0.8])))
    add("add_stroke", [base, arc([[0.2, 0.2], [0.5, 0.8], [0.8, 0.2]], k=1.0)],
        add_case(base, arc([[0.2, 0.2], [0.5, 0.8], [0.8, 0.2]], k=1.0)))
    empty = shape([])
    add("add_stroke", [empty, poly([[1.0, 2.0], [3.0, 4.0]])], add_case(empty, poly([[1.0, 2.0], [3.0, 4.0]])))
    # at：插到别的笔画前面 / 排到最后
    add("add_stroke", [base, poly([[1.0, 0.0], [1.5, 1.0]]), 0],
        add_case(base, poly([[1.0, 0.0], [1.5, 1.0]]), 0))
    add("add_stroke", [base, poly([[0.5, 0.0], [1.0, 0.5]]), 1],
        add_case(base, poly([[0.5, 0.0], [1.0, 0.5]]), 1))
    add("add_stroke", [base, poly([[0.5, 0.5], [1.0, 0.5], [1.5, 0.5]], free=True, smooth=40, k=2.0), 99],
        add_case(base, poly([[0.5, 0.5], [1.0, 0.5], [1.5, 0.5]], free=True, smooth=40, k=2.0), 99))

    # ------------------------------------------------------------ new_live_shape
    defaults = {"kind": "line", "vel0": 100.0, "vel1": 80.0, "end_dot": True}
    add("new_live_shape", [defaults, dict(C.CUSTOM_DEFAULTS)], C.new_live_shape(defaults, dict(C.CUSTOM_DEFAULTS)))
    add("new_live_shape", [defaults, {"fill": "spam", "gate": 0.125, "align": "centred", "ends": "keep",
                                      "union": True, "apart": True}],
        C.new_live_shape(defaults, {"fill": "spam", "gate": 0.125, "align": "centred", "ends": "keep",
                                    "union": True, "apart": True}))

    # ------------------------------------------------------------ outline_notes
    add("outline_notes", [shape([poly(SQUARE)]), 960.0], C.outline_notes(shape([poly(SQUARE)]), 960.0))
    add("outline_notes", [shape([gap_shape]), 960.0], C.outline_notes(shape([gap_shape]), 960.0))
    add("outline_notes", [shape([ell([0.0, 0.0, 1.0, 1.0])]), 960.0],
        C.outline_notes(shape([ell([0.0, 0.0, 1.0, 1.0])]), 960.0))
    add("outline_notes", [shape([poly([[0.0, 0.5]])]), 480.0], C.outline_notes(shape([poly([[0.0, 0.5]])]), 480.0))
    add("outline_notes", [shape([poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]),
                                 poly([[0.0, 0.0], [0.0, 1.0]])]), 960.0],
        C.outline_notes(shape([poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]),
                               poly([[0.0, 0.0], [0.0, 1.0]])]), 960.0))
    add("outline_notes", [shape([poly([[0.0, 0.0], [1.0, 2.0]], free=True, smooth=50, k=1.0)]), 960.0],
        C.outline_notes(shape([poly([[0.0, 0.0], [1.0, 2.0]], free=True, smooth=50, k=1.0)]), 960.0))
    add("outline_notes", [shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]]), 960.0],
        C.outline_notes(shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]]), 960.0))
    # only：只要这些编号的笔画
    many = shape([poly([[0.0, 0.0], [1.0, 0.0]]), poly([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]),
                  ell([0.2, 0.2, 0.8, 0.8])])
    add("outline_notes_only", [many, 960.0, [0]], C.outline_notes(many, 960.0, only=[0]))
    add("outline_notes_only", [many, 960.0, [1, 2]], C.outline_notes(many, 960.0, only=[1, 2]))

    # ------------------------------------------------------------ stroke_groups / outline_apart
    add("stroke_groups", [many], C.stroke_groups(many))
    add("outline_apart", [shape([poly(SQUARE)], fill="fill", apart=True)], C.outline_apart(shape([poly(SQUARE)], fill="fill", apart=True)))
    add("outline_apart", [shape([poly(SQUARE)], fill="spam")], C.outline_apart(shape([poly(SQUARE)], fill="spam")))
    add("outline_apart", [shape([poly(SQUARE)], fill="empty", apart=True)], C.outline_apart(shape([poly(SQUARE)], fill="empty", apart=True)))
    add("outline_apart", [shape([poly(SQUARE)], fill="fill", apart=True, notes=C.pack_notes(np.array([[0, 10, 60, 100, 0]], np.int64)))],
        C.outline_apart(shape([poly(SQUARE)], fill="fill", apart=True, notes=C.pack_notes(np.array([[0, 10, 60, 100, 0]], np.int64)))))

    # ------------------------------------------------------------ row_spans
    square = [[0.0, 60.0], [2.0, 60.0], [2.0, 63.0], [0.0, 63.0], [0.0, 60.0]]
    tri = [[0.0, 60.0], [2.0, 60.0], [1.0, 63.0], [0.0, 60.0]]
    holey = [square, [[0.5, 61.0], [1.5, 61.0], [1.5, 62.0], [0.5, 62.0], [0.5, 61.0]]]
    diagonal = [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0], [0.0, 60.0]]
    for polys in [[square], [tri], holey, [diagonal]]:
        for q in range(58, 66):
            add("row_spans", [polys, float(q)], C.row_spans(polys, q))
    add("row_spans", [[], 60.0], C.row_spans([], 60.0))
    add("row_spans", [[[[0.0, 60.0], [2.0, 60.0]]], 60.0], C.row_spans([[[0.0, 60.0], [2.0, 60.0]]], 60.0))
    # 多行、边界刚好在行上
    edge = [[0.0, 59.5], [2.0, 59.5], [2.0, 61.5], [0.0, 61.5], [0.0, 59.5]]
    for q in [59, 60, 61, 62]:
        add("row_spans", [[edge], float(q)], C.row_spans([edge], q))
    # 随机多边形
    for _ in range(4):
        n = rnd.randint(3, 7)
        p = [[rnd.uniform(-1, 3), rnd.uniform(57, 66)] for _ in range(n)]
        p.append(p[0])
        q = rnd.randint(57, 66)
        add("row_spans", [[p], float(q)], C.row_spans([p], q))

    # ------------------------------------------------------------ inside_spans
    sh_fill = shape([poly(SQUARE)])
    sh_gap = shape([gap_shape])
    sh_hole = shape([poly(SQUARE), poly([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75], [0.25, 0.25]])],
                    pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]])
    sh_tri = shape([poly(TRIANGLE)], pts=[[0.0, 59.5], [1.0, 59.5], [0.0, 61.5]])
    for s in [sh_fill, sh_gap, sh_hole, sh_tri]:
        add("inside_spans", [s, 960.0], C.inside_spans(s, 960.0))
    add("inside_spans", [shape(two_gaps), 960.0], C.inside_spans(shape(two_gaps), 960.0))

    # ------------------------------------------------------------ spam_gate / chop / chop_count
    for gate in [0.0625, 0.1, 0.5, 0.0001, 1.0, 0.25]:
        add("spam_gate", [shape([poly(SQUARE)], gate=gate), 960.0], C.spam_gate(shape([poly(SQUARE)], gate=gate), 960.0))
    chop_rows = [[0, 1000, 60], [0, 30, 61], [100, 90, 62], [5, 200, 63], [-61, -1, 64], [100, 100, 65]]
    for align in ["auto", "aligned", "centred"]:
        for ends in ["round", "keep", "drop", "min", "stretch"]:
            sh = shape([poly(SQUARE)], align=align, ends=ends)
            add("chop", [sh, chop_rows, 60], C.chop(sh, np.array(chop_rows, np.int64), 60))
            add("chop_count", [sh, chop_rows, 60], C.chop_count(sh, chop_rows, 60))
    # 没有 ends 键的旧形状按 drop 读
    add("chop", [shape([poly(SQUARE)]), chop_rows, 60],
        C.chop(shape([poly(SQUARE)]), np.array(chop_rows, np.int64), 60))
    # 空表
    add("chop", [shape([poly(SQUARE)]), [], 60], C.chop(shape([poly(SQUARE)]), np.zeros((0, 3), np.int64), 60))
    add("chop_count", [shape([poly(SQUARE)]), [], 60], C.chop_count(shape([poly(SQUARE)]), [], 60))
    # 极小的段：round 至少一个；drop / min / keep / stretch 的零头
    tiny = [[0, 5, 60], [0, 8, 61], [0, 12, 62], [0, 14, 63], [0, 15, 64], [0, 16, 65], [5, 23, 66]]
    for ends in ["round", "drop", "min", "keep", "stretch"]:
        sh = shape([poly(SQUARE)], ends=ends)
        add("chop", [sh, tiny, 16], C.chop(sh, np.array(tiny, np.int64), 16))
    # aligned / centred 下 16 ticks 门限的零头
    for align in ["aligned", "centred"]:
        for ends in ["round", "keep", "drop", "min", "stretch"]:
            sh = shape([poly(SQUARE)], align=align, ends=ends)
            add("chop", [sh, tiny, 16], C.chop(sh, np.array(tiny, np.int64), 16))

    # ------------------------------------------------------------ outline_spam / flat_notes
    for ends in ["round", "keep", "drop", "min", "stretch"]:
        sh = shape([poly(SQUARE)], ends=ends)
        add("outline_spam", [sh, 960.0], C.outline_spam(sh, 960.0))
    add("outline_spam", [shape([gap_shape], gate=0.25), 960.0], C.outline_spam(shape([gap_shape], gate=0.25), 960.0))
    add("outline_spam", [shape([poly([[0.0, 0.0], [3.0, 4.0]])], gate=0.5), 480.0],
        C.outline_spam(shape([poly([[0.0, 0.0], [3.0, 4.0]])], gate=0.5), 480.0))
    add("outline_spam", [shape([poly([[0.0, 0.0], [0.01, 0.2]])], gate=0.5), 480.0],
        C.outline_spam(shape([poly([[0.0, 0.0], [0.01, 0.2]])], gate=0.5), 480.0))
    # 扁的开放段保留自己的轮廓音符
    flat_shape = shape([poly(SQUARE), poly([[0.0, 0.5], [1.0, 0.5]])])
    add("flat_notes", [flat_shape, 960.0], C.flat_notes(flat_shape, 960.0))
    add("flat_notes", [shape([poly(SQUARE)]), 960.0], C.flat_notes(shape([poly(SQUARE)]), 960.0))
    add("flat_notes", [shape([poly(SQUARE)], text={"threshold": 50.0, "grow": 0.0, "k": 1.0, "holes": []}), 960.0],
        C.flat_notes(shape([poly(SQUARE)], text={"threshold": 50.0, "grow": 0.0, "k": 1.0, "holes": []}), 960.0))
    add("flat_notes", [shape(two_gaps), 960.0], C.flat_notes(shape(two_gaps), 960.0))

    # ------------------------------------------------------------ custom_note_count
    shapes_count = [
        shape([poly(SQUARE)]),
        shape([poly(SQUARE)], fill="fill"),
        shape([poly(SQUARE)], fill="spam"),
        shape([poly(SQUARE)], fill="spam", align="aligned"),
        shape([poly(SQUARE)], fill="spam", align="centred"),
        shape([poly(SQUARE)], fill="outline_spam"),
        shape([gap_shape], fill="spam"),
        shape(two_gaps, fill="fill"),
        shape([]),
        shape([poly(SQUARE)], fill="spam", ends="keep"),
        shape([poly(SQUARE)], fill="spam", ends="min"),
        shape([poly(SQUARE)], fill="spam", ends="stretch"),
        flat_shape,
        shape([flat_shape["strokes"][0], flat_shape["strokes"][1]], fill="fill"),
        shape([flat_shape["strokes"][0], flat_shape["strokes"][1]], fill="spam"),
    ]
    for s in shapes_count:
        add("custom_note_count", [s, 960.0], C.custom_note_count(s, 960.0))
    ns_rows = [[0, 240, 60, 100, 0], [240, 480, 62, 110, 1], [480, 720, 64, 90, 0]]
    ns = C.notes_shape(np.array(ns_rows, np.int64), 960.0, "Pasted notes")
    add("custom_note_count", [ns, 960.0], C.custom_note_count(ns, 960.0))
    # apart 的 Fill / Spam 不数（生成出来再数）
    add("custom_note_count", [shape([poly(SQUARE)], fill="fill", apart=True), 960.0],
        C.custom_note_count(shape([poly(SQUARE)], fill="fill", apart=True), 960.0))
    add("custom_note_count", [shape([poly(SQUARE)], fill="spam", apart=True), 960.0],
        C.custom_note_count(shape([poly(SQUARE)], fill="spam", apart=True), 960.0))

    # ------------------------------------------------------------ custom_notes / custom_notes_groups
    sh_union = shape([poly(SQUARE), poly([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75], [0.25, 0.25]])],
                     pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]])
    sh_union2 = dict(sh_union, union=True)
    notes_shapes = [
        shape([poly(SQUARE)]),
        shape([poly(SQUARE)], fill="fill"),
        shape([poly(SQUARE)], fill="spam"),
        shape([poly(SQUARE)], fill="outline_spam"),
        shape([gap_shape], fill="spam"),
        shape(two_gaps, fill="fill"),
        shape([poly(SQUARE)], pts=[[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]]),
        sh_union, sh_union2,
        flat_shape,
        dict(flat_shape, fill="fill"),
        dict(flat_shape, fill="spam"),
        shape([poly(SQUARE)], fill="fill", apart=True),
        shape([poly(SQUARE)], fill="spam", apart=True),
        shape([poly(SQUARE)], fill="spam", apart=True, ends="keep", align="centred"),
        shape([poly(SQUARE)], fill="spam", ends="stretch"),
        sh_union2,
    ]
    for poses in [sh_union, sh_union2]:
        add("inside_spans", [poses, 960.0], C.inside_spans(poses, 960.0))
    for s in notes_shapes:
        add("custom_notes", [s, 960.0], C.custom_notes(s, 960.0))
        add("custom_note_count", [s, 960.0], C.custom_note_count(s, 960.0))
    add("custom_notes", [ns, 960.0], C.custom_notes(ns, 960.0))
    for s in [shape([poly(SQUARE)]), shape([poly(SQUARE)], fill="fill", apart=True),
              shape([poly(SQUARE)], fill="spam", apart=True), shape([gap_shape], fill="outline_spam"),
              flat_shape]:
        add("custom_notes_groups", [s, 960.0], C.custom_notes_groups(s, 960.0))
        add("outline_groups", [s, 960.0, False], C.outline_groups(s, 960.0))
        add("outline_groups", [s, 960.0, True], C.outline_groups(s, 960.0, spam=True))

    # ------------------------------------------------------------ union_spans / merged_by_key / covered / cut_out / on_edge / edge_parts
    hole = [[0.5, 61.0], [1.5, 61.0], [1.5, 62.0], [0.5, 62.0], [0.5, 61.0]]
    for q in [59, 60, 61, 62, 63, 64]:
        add("union_spans", [[square, hole], float(q)], C.union_spans([square, hole], q))
        add("union_spans", [[square], float(q)], C.union_spans([square], q))
        add("union_spans", [[], float(q)], C.union_spans([], q))
    mk_notes = [[0, 10, 60], [5, 15, 60], [20, 30, 60], [0, 10, 61], [80, 90, 59], [15, 25, 61]]
    add("merged_by_key", [mk_notes], C.merged_by_key(np.array(mk_notes, np.int64)))
    add("merged_by_key", [[]], C.merged_by_key(np.zeros((0, 3), np.int64)))
    cov_notes = [[0, 10, 60], [2, 4, 60], [5, 25, 60], [15, 25, 61], [12, 18, 61]]
    add("covered", [cov_notes, mk_notes], C.covered(np.array(cov_notes, np.int64), np.array(mk_notes, np.int64)))
    add("covered", [[], mk_notes], C.covered(np.zeros((0, 3), np.int64), np.array(mk_notes, np.int64)))
    add("covered", [cov_notes, []], C.covered(np.array(cov_notes, np.int64), np.zeros((0, 3), np.int64)))
    add("cut_out", [cov_notes, mk_notes], C.cut_out(np.array(cov_notes, np.int64), np.array(mk_notes, np.int64)))
    add("cut_out", [cov_notes, []], C.cut_out(np.array(cov_notes, np.int64), np.zeros((0, 3), np.int64)))
    add("on_edge", [mk_notes], C.on_edge(np.array(mk_notes, np.int64)))
    add("on_edge", [cov_notes], C.on_edge(np.array(cov_notes, np.int64)))
    add("on_edge", [[]], C.on_edge(np.zeros((0, 3), np.int64)))
    add("edge_parts", [mk_notes], C.edge_parts(np.array(mk_notes, np.int64)))
    add("edge_parts", [[]], C.edge_parts(np.zeros((0, 3), np.int64)))
    # apart 的 edge_parts / cut_out 用真实形状的长音符
    for s in [sh_union, dict(sh_union, fill="fill"), dict(sh_union, apart=True)]:
        spans = np.asarray(C.inside_spans(s, 960.0), np.int64).reshape(-1, 3)[:, [1, 2, 0]]
        add("edge_parts", [spans], C.edge_parts(spans))
        add("cut_out", [spans, C.edge_parts(spans)], C.cut_out(spans, C.edge_parts(spans)))

    # ------------------------------------------------------------ pack / unpack / check
    rows5 = [[0, 240, 60, 100, 0], [240, 480, 62, 110, 1], [480, 720, 64, 90, 0]]
    add("pack_notes", [rows5], C.pack_notes(np.array(rows5, np.int64)))
    add("pack_notes", [[[10, 20, 30, 40, 50]]], C.pack_notes(np.array([[10, 20, 30, 40, 50]], np.int64)))
    add("pack_notes", [[]], C.pack_notes(np.zeros((0, 5), np.int64)))
    add("unpack_notes", [C.pack_notes(np.array(rows5, np.int64))], C.unpack_notes(C.pack_notes(np.array(rows5, np.int64))))
    # 旧格式（没有 track 列）
    import base64
    import zlib
    legacy = base64.b64encode(zlib.compress(np.asarray([[0, 120, 60, 100], [240, 360, 64, 90]], "<i4").tobytes(), 1)).decode("ascii")
    add("unpack_notes", [legacy], C.unpack_notes(legacy))
    add("unpack_notes", [C.pack_notes(np.zeros((0, 5), np.int64))], C.unpack_notes(C.pack_notes(np.zeros((0, 5), np.int64))))
    add("check_notes", [C.pack_notes(np.array(rows5, np.int64))], C.check_notes(C.pack_notes(np.array(rows5, np.int64))))
    add("check_notes", [legacy], C.check_notes(legacy))
    add("check_notes", [C.pack_notes(np.zeros((0, 5), np.int64))], C.check_notes(C.pack_notes(np.zeros((0, 5), np.int64))))
    add("check_notes", [C.pack_notes(np.array([[-1, 240, 60, 100, 0]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[-1, 240, 60, 100, 0]], np.int64))))
    add("check_notes", [C.pack_notes(np.array([[0, 0, 60, 100, 0]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[0, 0, 60, 100, 0]], np.int64))))
    add("check_notes", [C.pack_notes(np.array([[0, 240, -1, 100, 0]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[0, 240, -1, 100, 0]], np.int64))))
    add("check_notes", [C.pack_notes(np.array([[0, 240, 60, 0, 0]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[0, 240, 60, 0, 0]], np.int64))))
    add("check_notes", [C.pack_notes(np.array([[0, 240, 60, 128, 0]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[0, 240, 60, 128, 0]], np.int64))))
    add("check_notes", [C.pack_notes(np.array([[0, 240, 60, 100, -1]], np.int64))],
        C.check_notes(C.pack_notes(np.array([[0, 240, 60, 100, -1]], np.int64))))
    add("check_notes", [""], C.check_notes(""))
    add("check_notes", ["t:not-base64!!"], C.check_notes("t:not-base64!!"))
    add("check_notes", ["eJwDAAAAAAE="], C.check_notes("eJwDAAAAAAE="))

    # ------------------------------------------------------------ notes_shape / block_notes
    for rows, name in [(rows5, "Pasted notes"),
                       ([[0, 240, 60, 100, 0], [240, 480, 62, 101, 0]], "Half"),
                       ([[0, 240, 60, 101, 0], [240, 480, 62, 102, 0]], "Half up"),
                       ([[960, 1200, 72, 127, 2], [0, 240, 60, 1, 0], [480, 600, 60, 64, 1]], "Chord"),
                       ([[100, 101, 60, 64, 0]], "One")]:
        ns = C.notes_shape(np.array(rows, np.int64), 960.0, name)
        add("notes_shape", [rows, 960.0, name], ns)
        add("block_notes", [ns, 960.0], C.block_notes(ns, 960.0))
    # 移动 / 拉伸 / 翻转 / 旋转的粘贴音符框
    pasted = C.notes_shape(np.array(rows5, np.int64), 960.0, "Pasted notes")
    for pts in [[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]],
                [[0.0, 60.0], [4.0, 60.0], [0.0, 64.0]],
                [[2.0, 64.0], [2.0, 60.0], [4.0, 64.0]],
                [[0.0, 60.0], [2.0, 62.0], [1.0, 65.0]]]:
        sh = copy.deepcopy(pasted)
        sh["pts"] = pts
        add("block_notes", [sh, 960.0], C.block_notes(sh, 960.0))
    # 空框（不转）里的 block_notes
    sh_empty = copy.deepcopy(pasted)
    sh_empty["pts"] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    add("block_notes", [sh_empty, 480.0], C.block_notes(sh_empty, 480.0))

    return {"module": "custom", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("custom", data["cases"])
