"""text.py 的对照向量。

字体相关部分（layout / build / text_font）没法在 macOS 上生成对照向量：原版 fonts.py 走
Windows GDI。这里只把用到字体 cap 的少数分支用桩字体固定住（CAPS），其余都是纯几何 /
纯设置的函数。输入的浮点数先量化到 15 位有效数字再交给 Python 原版计算，保证 serde_json
默认浮点解析能精确还原。字体部分的冒烟测试见 tests/text_vectors.rs。
"""

import os
import sys
from types import SimpleNamespace

from vec_common import write

# 在 worktree 里跑时 vec_common 推不出原版脚本目录，这里补一个回退路径
_SCRIPTS = os.environ.get("SPIDERWEB_SCRIPTS", "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

# ctypes.wintypes 只是 import 不碰 GDI，macOS 上也能导入 text.py；字体查询换成本地桩。
from notes import text as T

CAPS = {"Arial": 0.716, "Times New Roman": 0.662, "Courier New": 0.572, "Verdana": 0.727}


def fake_get_font(family, weight=400, italic=False):
    return SimpleNamespace(cap=CAPS.get(family, 0.7))


T.get_font = fake_get_font


def cap_of(family):
    return CAPS.get(family, 0.7)


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
    """复刻 serde_json（未开 float_roundtrip）的浮点解析，用于自检。"""
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


def add(cases, fn, args, call, **extra):
    a = rjson(args)
    check(a)
    case = {"fn": fn, "args": a, "out": rjson(call(*a))}
    case.update(extra)
    cases.append(case)


# ---------------------------------------------------------------- 合成数据

def curve(poly):
    """折线 -> 贝塞尔点列（直线：控制点在 1/3、2/3 处）。"""
    out = [list(poly[0])]
    for a, b in zip(poly, poly[1:]):
        out += [[a[0] + (b[0] - a[0]) / 3, a[1] + (b[1] - a[1]) / 3],
                [a[0] + (b[0] - a[0]) * 2 / 3, a[1] + (b[1] - a[1]) * 2 / 3],
                list(b)]
    return out


# 折线多边形（首点 = 末点）
SQ = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]
SQ_REV = [[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0], [0.0, 0.0]]
TRI = [[0.0, 0.0], [3.0, 0.0], [0.0, 2.0], [0.0, 0.0]]
SPIKE = [[0.0, 0.0], [4.0, 0.0], [0.0, 0.4], [0.0, 0.0]]       # (4,0) 是很尖的角
DUP = [[0.0, 0.0], [0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 0.0]]
TINY = [[0.0, 0.0], [1e-12, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]
DEGEN = [[0.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
COLLINEAR = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
INNER = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5], [0.5, 0.5]]
INNER_REV = [[0.5, 0.5], [0.5, 1.5], [1.5, 1.5], [1.5, 0.5], [0.5, 0.5]]
CORE = [[0.75, 0.75], [1.25, 0.75], [1.25, 1.25], [0.75, 1.25], [0.75, 0.75]]
CORE_REV = [[0.75, 0.75], [0.75, 1.25], [1.25, 1.25], [1.25, 0.75], [0.75, 0.75]]
RING = [SQ, INNER_REV]  # 带洞的环

# 贝塞尔轮廓（layout / find_holes 用）
SQ_C = curve(SQ)
SQ_REV_C = curve(SQ_REV)
INNER_C = curve(INNER)
INNER_REV_C = curve(INNER_REV)
CORE_C = curve(CORE)
CORE_REV_C = curve(CORE_REV)
# 四段三次贝塞尔近似的圆（半径 1）
CIRCLE_C = [[1.0, 0.0], [1.0, 0.5522847498307935], [0.5522847498307935, 1.0], [0.0, 1.0],
            [-0.5522847498307935, 1.0], [-1.0, 0.5522847498307935], [-1.0, 0.0],
            [-1.0, -0.5522847498307935], [-0.5522847498307935, -1.0], [0.0, -1.0],
            [0.5522847498307935, -1.0], [1.0, -0.5522847498307935], [1.0, 0.0]]
def mktx(**kw):
    """一份完整的文本设置（生成脚本里的 Python 字典）。"""
    tx = {"text": "Hi", "font": "Arial", "size": 24.0, "unit": "font", "weight": 400, "italic": False,
          "tracking": 0.0, "leading": 100.0, "align": "left", "threshold": 50.0, "grow": 0.0,
          "bbox": [0.0, 0.0, 1.0, 1.0], "cap": 0.716, "k": 1.0, "holes": []}
    tx.update(kw)
    return tx


AX = [[10.0, 60.0], [12.0, 62.0], [8.0, 64.0]]  # (O, X, Y)
AX_FLAT = [[10.0, 60.0], [12.0, 62.0], [14.0, 66.0]]  # det = 0
AX_ZEROY = [[10.0, 60.0], [12.0, 62.0], [10.0, 60.0]]


def axes_out(axes):
    return [list(axes[0]), list(axes[1]), list(axes[2])]


def restyle_call(tx, axes, changes):
    new, out = T.restyle(tx, axes, changes)
    return [new, axes_out(out)]


def text_polys_call(sh):
    return T.text_polys(sh)


def gen():
    cases = []

    # ---- flatten：直线一步、弯按容差、退化点列、闭合轮廓
    flat_cases = [
        ([[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]], 0.004),
        ([[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0]], 0.004),
        ([[0.0, 0.0], [1.0, 3.0], [3.0, 3.0], [4.0, 0.0]], 0.004),
        ([[0.0, 0.0], [1.0, 3.0], [3.0, 3.0], [4.0, 0.0]], 0.05),
        ([[0.0, 0.0], [1.0, 3.0], [3.0, 3.0], [4.0, 0.0]], 1e-6),
        ([[0.0, 0.0], [1.0, 3.0], [3.0, 3.0], [4.0, 0.0]], 2.0),
        ([[0.5, 1.5]], 0.004),
        ([[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], 0.004),
        ([[0.0, 0.0], [1.0, 2.0], [2.0, 1.0], [3.0, 0.0], [2.0, -1.0], [1.0, -2.0], [0.0, 0.0]], 0.004),
        ([[0.0, 0.0], [1.0, 2.0], [2.0, 1.0], [3.0, 0.0], [2.0, -1.0], [1.0, -2.0], [0.0, 0.0]], 0.0005),
        (SQ_C, 0.004),
        (CIRCLE_C, 0.002),
    ]
    for pts, tol in flat_cases:
        add(cases, "flatten", [pts, tol], T.flatten)

    # ---- area：方向、开折线、退化
    for poly in (SQ, SQ_REV, TRI, [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]], [], [[1.0, 2.0]],
                 [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0], [0.0, 0.0]]):
        add(cases, "area", [poly], T.area)

    # ---- winding：里 / 外 / 边上、反向、非凸
    for poly, x, y in (
        (SQ, 1.0, 1.0), (SQ, 3.0, 1.0), (SQ, 1.0, 3.0), (SQ, 0.0, 1.0), (SQ, 2.0, 1.0),
        (SQ, 0.0, 0.0), (SQ, 1.0, 0.0), (SQ_REV, 1.0, 1.0), (TRI, 1.0, 0.5), (TRI, 2.5, 0.5),
        (COLLINEAR, 1.0, 0.5), (COLLINEAR, 2.5, 0.5), (DEGEN, 0.5, 0.5), (INNER_REV, 1.0, 1.0),
        (INNER_REV, 0.25, 1.0),
    ):
        add(cases, "winding", [poly, x, y], T.winding)

    # ---- offset：正 / 负、尖角、重复点、退化
    for poly, d in (
        (SQ, 0.5), (SQ, -0.5), (SQ, 0.0), (SQ_REV, 0.5), (SQ_REV, -0.5), (TRI, 0.25),
        (SPIKE, 0.3), (SPIKE, -0.3), (DUP, 0.5), (TINY, 0.5), (DEGEN, 0.5), (COLLINEAR, 0.4),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [1.0, -1.0], [0.0, 0.0]], 0.6),
    ):
        add(cases, "offset", [poly, d], T.offset)

    # ---- row_edges / line_spans / threshold_spans
    for polys, lo, hi in ((RING, 0.5, 1.5), (RING, 0.0, 2.0), ([SQ, TRI], 0.5, 1.5),
                          ([SQ_C], 0.0, 2.0), ([SQ], 2.0, 3.0)):
        add(cases, "row_edges", [polys, lo, hi], T.row_edges)

    edge_sets = [T.row_edges(RING, 0.0, 2.0), T.row_edges([SQ], 0.0, 2.0)]
    for edges, y in ((edge_sets[0], 1.0), (edge_sets[0], 0.6), (edge_sets[0], 0.5),
                     (edge_sets[0], 3.0), (edge_sets[1], 1.0), (edge_sets[1], 0.0), (edge_sets[1], 2.0)):
        add(cases, "line_spans", [edges, y], T.line_spans)

    # 覆盖阈值 0 / 50 / 100：正方形铺满 key 1；环在 key 1 上有洞
    for polys, q, threshold in (
        ([SQ], 1.0, 0.0), ([SQ], 1.0, 50.0), ([SQ], 1.0, 100.0), ([SQ], 1.0, 33.0),
        (RING, 1.0, 0.0), (RING, 1.0, 50.0), (RING, 1.0, 100.0), (RING, 1.0, 21.0),
        ([SQ, TRI], 1.0, 50.0), ([SQ], 3.0, 50.0), ([SQ], 0.0, 50.0),
    ):
        add(cases, "threshold_spans", [polys, q, threshold], T.threshold_spans)

    # ---- find_holes：同字母的嵌套（含方向相反）、不同 glyph、圆环
    for contours in (
        [(0, SQ_C), (0, INNER_REV_C)],
        [(0, SQ_C), (0, INNER_C)],
        [(0, SQ_C), (1, INNER_REV_C)],
        [(0, SQ_C), (0, INNER_REV_C), (0, CORE_C)],
        [(0, SQ_C), (0, INNER_REV_C), (0, CORE_REV_C)],
        [(0, SQ_C), (0, SQ_REV_C)],
        [(0, CIRCLE_C), (0, curve([[0.4, 0.0], [0.0, 0.4], [-0.4, 0.0], [0.0, -0.4], [0.4, 0.0]]))],
        [(0, SQ_C)],
    ):
        add(cases, "find_holes", [contours], T.find_holes)

    # ---- clean_text：合法、缺省、坏掉
    full = {"text": "Hi\nthere", "font": "Times New Roman", "size": 12.5, "unit": "rows", "weight": 700,
            "italic": True, "tracking": -30.0, "leading": 90.0, "align": "center", "threshold": 20.0,
            "grow": 0.25, "bbox": [-1.0, -0.5, 2.0, 1.5], "cap": 0.6, "k": 2.0, "holes": [1, 0, 1]}
    clean_cases = [
        full,
        {"bbox": [0.0, 0.0, 1.0, 1.0]},
        {"text": None, "font": None, "bbox": [0, 0, 1, 1]},
        {"text": 5, "font": 7, "bbox": [0, 0, 1, 1]},
        {"font": "", "bbox": [0, 0, 1, 1]},
        {"text": "x", "bbox": [0, 0, 1, 1], "size": "3.5", "weight": "250", "italic": 1},
        {"text": "x", "bbox": [0, 0, 1, 1], "size": None},
        {"text": "x", "bbox": [0, 0, 1, 1], "size": "abc"},
        {"text": "x", "bbox": [0, 0, 1, 1], "weight": 1500},
        {"text": "x", "bbox": [0, 0, 1, 1], "weight": -5},
        {"text": "x", "bbox": [0, 0, 1, 1], "weight": 3.9},
        {"text": "x", "bbox": [0, 0, 1, 1], "weight": True},
        {"text": "x", "bbox": [0, 0, 1, 1], "unit": "ROWS"},
        {"text": "x", "bbox": [0, 0, 1, 1], "unit": "rows"},
        {"text": "x", "bbox": [0, 0, 1, 1], "align": "Centre"},
        {"text": "x", "bbox": [0, 0, 1, 1], "align": "right"},
        {"text": "x", "bbox": [0, 0, 1, 1], "threshold": -50.0},
        {"text": "x", "bbox": [0, 0, 1, 1], "threshold": 150.0},
        {"text": "x", "bbox": [0, 0, 1, 1], "cap": 0.0, "k": 0.0},
        {"text": "x", "bbox": [0, 0, 1, 1], "holes": [3, 1, 3, 2.7, "5", True]},
        {"text": "x", "bbox": [0, 0, 1, 1], "holes": [3, 1, 3, 2.7, "5", True, None]},
        {"text": "x", "bbox": [0, 0, 1, 1], "holes": "31"},
        {"text": "x", "bbox": [0, 0, 1, 1], "holes": 5},
        {"text": "x", "bbox": "1234"},
        {"text": "x"},
        {"text": "x", "bbox": [0, 0, 1]},
        {"text": "x", "bbox": [0, 0, 1, 1, 2]},
        {"text": "x", "bbox": [0, 0, "a", 1]},
        {"text": "x", "bbox": 3},
        None,
        [],
        5,
        "text",
    ]
    for tx in clean_cases:
        add(cases, "clean_text", [tx], T.clean_text)

    # ---- text_name
    for text in ("", "hello", "a" * 25, "a" * 26, "  one\ttwo\nthree  ", "“x”", "中" * 25, "中" * 26):
        add(cases, "text_name", [text], T.text_name)

    # ---- text_axes
    sh_cases = [
        {"pts": [[10.0, 60.0], [20.0, 60.0], [10.0, 80.0]], "text": {"bbox": [-1.0, -0.5, 2.0, 1.5]}},
        {"pts": [[0.0, 0.0], [4.0, 0.0], [0.0, 2.0]], "text": {"bbox": [0.0, 0.0, 1.0, 1.0]}},
        {"pts": [[1.0, 2.0], [1.0, 2.0], [1.0, 2.0]], "text": {"bbox": [0.0, 0.0, 1.0, 1.0]}},
        {"pts": [[10.0, 60.0], [20.0, 60.0], [10.0, 80.0]], "text": {"bbox": [2.0, 4.0, 4.0, 12.0]}},
    ]
    for sh in sh_cases:
        add(cases, "text_axes", [sh], T.text_axes)

    # ---- 轴变换
    for axes, x, y in ((AX, 1.0, 2.0), (AX, 0.0, 0.0), (AX, -1.5, 0.25)):
        add(cases, "to_roll", [axes, x, y], T.to_roll)
    for axes, b, p in ((AX, 10.0, 60.0), (AX, 14.0, 66.0), (AX_FLAT, 10.0, 60.0)):
        add(cases, "from_roll", [axes, b, p], T.from_roll)
    for axes, k in ((AX, 1.0), (AX, 2.0), (AX, -1.0), (AX_ZEROY, 1.0)):
        add(cases, "axes_em", [axes, k], T.axes_em)
    for axes, f in ((AX, 1.0), (AX, 0.5), (AX, -2.0), (AX, 0.0)):
        add(cases, "scale_axes", [axes, f], T.scale_axes)

    # ---- em 单位相关（字体只从桩拿 cap）
    em_cases = [
        (mktx(), None),
        (mktx(), 40.0),
        (mktx(unit="rows"), None),
        (mktx(unit="rows"), 12.0),
        (mktx(unit="rows", cap=0.0), None),
        (mktx(unit="rows", cap=0.0, font="Times New Roman"), 10.0),
    ]
    for tx, size in em_cases:
        add(cases, "em_keys", [tx, size], T.em_keys, font_cap=cap_of(tx["font"]))

    for tx, b, p, k in (
        (mktx(), 10.0, 60.0, 1.0), (mktx(unit="rows"), 4.0, 3.0, 0.5),
        (mktx(unit="rows", cap=0.0, font="Verdana"), 0.0, 0.0, 2.0),
    ):
        add(cases, "new_axes", [tx, b, p, k], T.new_axes, font_cap=cap_of(tx["font"]))

    for tx, axes in (
        (mktx(), AX), (mktx(unit="rows"), AX), (mktx(unit="rows", cap=0.0), AX),
        (mktx(unit="rows", cap=0.5, k=2.0), [[0.0, 1.0], [1.0, 0.0], [0.0, 2.0]]),
        (mktx(), AX_ZEROY),
    ):
        add(cases, "shown_size", [tx, axes], T.shown_size, font_cap=cap_of(tx["font"]))

    restyle_cases = [
        (mktx(), AX, {"align": "center"}),
        (mktx(), AX, {"size": 40.0}),
        (mktx(), AX, {"font": "Times New Roman"}),
        (mktx(), AX, {"unit": "rows"}),
        (mktx(unit="rows", cap=0.5), AX, {"unit": "font"}),
        (mktx(cap=0.0), AX, {"align": "right"}),
        (mktx(), AX, {"size": 8.0, "tracking": 20.0, "weight": 700, "italic": True}),
        (mktx(), AX_ZEROY, {"size": 8.0}),
        (mktx(unit="rows", cap=0.0, font="Courier New"), AX, {"font": "Verdana", "size": 5.0}),
    ]
    for tx, axes, changes in restyle_cases:
        new_family = changes.get("font", tx["font"])
        add(cases, "restyle", [tx, axes, changes], restyle_call,
            font_cap=cap_of(tx["font"]), new_cap=cap_of(new_family))

    # ---- text_polys：合成笔画 + grow / 洞 / k
    hole = curve(INNER_REV)
    sh_text = mktx(grow=0.0)
    sh_base = {"pts": [[10.0, 60.0], [20.0, 60.0], [10.0, 80.0]], "text": sh_text,
               "strokes": [{"kind": "curve", "pts": SQ_C}]}
    sh_grow = {"pts": [[10.0, 60.0], [20.0, 60.0], [10.0, 80.0]], "text": mktx(grow=0.1, holes=[1]),
               "strokes": [{"kind": "curve", "pts": SQ_C}, {"kind": "curve", "pts": hole}]}
    sh_shrink = {"pts": [[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]], "text": mktx(grow=-0.05, k=2.0, holes=[1]),
                 "strokes": [{"kind": "curve", "pts": SQ_C}, {"kind": "curve", "pts": hole}]}
    sh_smooth = {"pts": [[0.0, 0.0], [1.0, 1.0], [-1.0, 1.0]], "text": mktx(grow=0.2),
                 "strokes": [{"kind": "curve", "pts": [[0.0, 0.0], [0.4, 0.0], [0.6, 0.0], [1.0, 0.0],
                                                        [1.0, 0.0], [1.0, 0.0], [1.0, 1.0]]}]}
    for sh in (sh_base, sh_grow, sh_shrink, sh_smooth):
        add(cases, "text_polys", [sh], text_polys_call)

    return cases


if __name__ == "__main__":
    write("text", gen())
