"""funnel.py 的对照向量。

Python 原版（notes/funnel.py）直接跑，输出写到
crates/spiderweb-core/tests/vectors/funnel.json，Rust 测试逐用例对照。
抛异常的用例记 "err": true、out 为 null，Rust 侧对应函数返回 None / Err。
"""

import copy
import math

import numpy as np

from vec_common import write

from notes import funnel as F


def tolist(x):
    if isinstance(x, dict):
        return {str(k): tolist(v) for k, v in x.items()}
    if isinstance(x, np.ndarray):
        return x.tolist()
    if isinstance(x, np.integer):
        return int(x)
    if isinstance(x, np.floating):
        return float(x)
    if isinstance(x, np.bool_):
        return bool(x)
    if isinstance(x, (list, tuple)):
        return [tolist(v) for v in x]
    return x


# ---------------------------------------------------------------- 构造小工具

def curve(pts, sharp=None, link=None, flip=False):
    d = {"pts": [[float(u), float(f)] for u, f in pts]}
    if sharp is not None:
        d["sharp"] = [int(a) for a in sharp]
    if link is not None:
        d["link"] = int(link)
        d["flip"] = bool(flip)
    return d


def start(line, at, ends):
    return {"line": int(line), "at": float(at), "ends": copy.deepcopy(ends)}


def shape(pts, starts=None, fill="spam", gate0=0.0625, gate1=0.0625, vary=False,
          change="steps", follow="time", wall="in"):
    return {
        "kind": "funnel",
        "pts": [[float(b), float(p)] for b, p in pts],
        "starts": copy.deepcopy(starts) if starts else [],
        "fill": fill,
        "gate0": float(gate0),
        "gate1": float(gate1),
        "vary": bool(vary),
        "change": change,
        "follow": follow,
        "wall": wall,
    }


def with_settings(base, **kw):
    d = copy.deepcopy(base)
    d.update(kw)
    return d


# ---------------------------------------------------------------- 曲线与几何

C1 = [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]]
C2 = [[0.0, 0.0], [0.1, 0.05], [0.2, 0.1], [0.5, 0.5], [0.6, 0.7], [0.8, 0.9], [1.0, 1.0]]
C3 = [[0.0, 0.0], [0.2, 0.15], [0.5, 0.35], [0.5, 0.5], [0.5, 0.65], [0.8, 0.85], [1.0, 1.0]]
C_PULL = [[0.0, 0.0], [0.1, 0.05], [0.2, 0.1], [0.5, 0.5], [0.55, 0.75], [0.8, 0.9], [1.0, 1.0]]

LINE = [[0.0, 60.0], [8.0, 64.0]]
WALL = [[8.0, 66.0], [8.0, 62.0]]
STARTS = [start(0, 0.25, [curve(C1), curve(C2, sharp=[1])])]

SH_SIMPLE = shape(LINE + WALL, starts=STARTS)
SH_EMPTY = shape(LINE + WALL, starts=[])
SH_REVERSE = shape([[8.0, 60.0], [2.0, 64.0], [2.0, 66.0], [2.0, 62.0]],
                   starts=[start(0, 0.5, [curve(C1), curve(C3)])])
SH_EXTRA = shape(LINE + WALL + [[1.0, 58.0], [7.0, 60.0]],
                 starts=STARTS + [start(1, 0.5, [curve(C2), curve(C1, sharp=[1])])])
SH_EXTRA2 = shape(LINE + WALL + [[1.0, 58.0], [7.0, 60.0]] + [[0.5, 62.0], [6.5, 63.0]],
                  starts=[start(2, 0.5, [curve(C1), curve(C1)])])
SH_HALF = shape(LINE + [[8.0, 64.0], [8.0, 59.0]], starts=[start(0, 0.5, [None, curve(C1)])])
SH_VERT = shape([[4.0, 60.0], [4.0, 70.0], [4.0, 55.0], [4.0, 75.0]],
                starts=[start(0, 0.5, [curve(C1), curve(C1)])])
SH_VERT2 = shape([[4.0, 55.0], [4.0, 75.0], [4.0, 60.0], [4.0, 70.0]], starts=[])
SH_LOWWALL = shape([[0.0, 70.0], [4.0, 74.0], [4.0, 60.0], [4.0, 72.0]],
                   starts=[start(0, 0.5, [curve(C1), curve(C1)])])
SH_POINT = shape([[0.0, 60.0], [0.0, 60.0], [2.0, 64.0], [2.0, 60.0]],
                 starts=[start(0, 0.0, [curve(C1), None])])
SH_COLLINEAR = shape([[0.0, 60.0], [8.0, 60.0], [4.0, 60.0], [6.0, 60.0]],
                     starts=[start(0, 0.5, [curve(C1), curve(C1)])])
SH_PARALLEL = shape([[0.0, 60.0], [4.0, 64.0], [5.0, 60.0], [9.0, 64.0]],
                    starts=[start(0, 0.25, [curve(C1), curve(C1)])])
SH_WALLPOINT = shape(LINE + [[8.0, 64.0], [8.0, 64.0]],
                     starts=[start(0, 0.5, [curve(C1), curve(C1)])])
SH_TILT = shape([[0.0, 60.0], [6.0, 63.0], [7.0, 66.0], [6.0, 62.0]],
                starts=[start(0, 0.5, [None, curve(C2, sharp=[1])])])
SH_PULL = shape(LINE + WALL, starts=[
    start(0, 0.25, [curve(C_PULL, sharp=[1]), curve(C3, link=2, flip=True)]),
    start(0, 0.75, [curve(C1, link=2), None]),
])

SHAPES = [SH_SIMPLE, SH_EMPTY, SH_REVERSE, SH_EXTRA, SH_EXTRA2, SH_HALF, SH_VERT, SH_VERT2,
          SH_LOWWALL, SH_POINT, SH_COLLINEAR, SH_PARALLEL, SH_WALLPOINT, SH_TILT, SH_PULL]

PROBES = [(0.5, 61.0), (2.0, 62.0), (4.0, 63.0), (7.0, 64.0), (8.0, 65.0), (8.0, 63.0),
          (0.0, 60.0), (10.0, 60.0)]

FORMULAS = {
    "x": lambda x: x,
    "sq": lambda x: x * x,
    "flip_sq": lambda x: 1 - (1 - x) * (1 - x),
    "scurve": lambda x: x * x * (3 - 2 * x),
    "steep_s": lambda x: x * x * x * (x * (6 * x - 15) + 10),
    "quarter": lambda x: 1 - math.sqrt(1 - x * x),
    "reverse_s": lambda x: 0.5 - math.sin(math.asin(1 - 2 * x) / 3),
    "exp5": lambda x: math.exp(5 * x),
    "log": lambda x: math.log(1 + 20 * x),
    "const": lambda x: 1.0,
    "nan": lambda x: float("nan"),
    "domain": lambda x: math.sqrt(x - 2),
    "raise": lambda x: (_ for _ in ()).throw(ValueError("no")),
}


def gen():
    cases = []

    def add(fn, args, out):
        cases.append({"fn": fn, "args": tolist(args), "out": tolist(out)})

    def add_call(fn, args, call):
        try:
            out = call()
        except Exception:
            cases.append({"fn": fn, "args": tolist(args), "out": None, "err": True})
            return
        cases.append({"fn": fn, "args": tolist(args), "out": tolist(out)})

    # ------------------------------------------------------------ line_index / start_point
    for line in range(6):
        add("line_index", [line], F.line_index(line))
    for sh in (SH_SIMPLE, SH_EXTRA2):
        for line in range(len(F.funnel_lines(sh))):
            for at in (0.0, 0.25, 0.5, 1.0):
                add_call("start_point", [sh, at, line], lambda sh=sh, at=at, line=line: F.start_point(sh, at, line))
    add_call("start_point", [SH_SIMPLE, 0.5, 9], lambda: F.start_point(SH_SIMPLE, 0.5, 9))
    add_call("start_point", [SH_SIMPLE, 0.5, 1], lambda: F.start_point(SH_SIMPLE, 0.5, 1))

    # ------------------------------------------------------------ curve_box / box_point / box_uf
    box_cases = [
        (SH_SIMPLE, 0.0, 0, 0), (SH_SIMPLE, 0.25, 1, 0), (SH_SIMPLE, 0.5, 0, 0), (SH_SIMPLE, 1.0, 1, 0),
        (SH_REVERSE, 0.3, 0, 0), (SH_REVERSE, 0.8, 1, 0),
        (SH_EXTRA, 0.5, 0, 1), (SH_EXTRA, 0.5, 1, 1),
        (SH_HALF, 0.5, 0, 0), (SH_HALF, 0.5, 1, 0),
        (SH_POINT, 0.0, 0, 0), (SH_POINT, 0.0, 1, 0),
        (SH_COLLINEAR, 0.5, 0, 0),
        (SH_PARALLEL, 0.25, 0, 0), (SH_PARALLEL, 0.25, 1, 0),
        (SH_WALLPOINT, 0.5, 0, 0), (SH_WALLPOINT, 0.5, 1, 0),
        (SH_VERT, 0.5, 0, 0),
        (SH_TILT, 0.5, 1, 0),
        (SH_SIMPLE, 0.5, 0, 5),
    ]
    for sh, at, end, line in box_cases:
        add_call("curve_box", [sh, at, end, line], lambda sh=sh, at=at, end=end, line=line: F.curve_box(sh, at, end, line))
        box = None
        try:
            box = F.curve_box(sh, at, end, line)
        except Exception:
            pass
        if box:
            box_args = tolist(box)
            for u, f in ((0.0, 0.0), (0.5, 0.25), (1.0, 1.0), (1.2, -0.3)):
                add_call("box_point", [box_args, u, f], lambda box=box, u=u, f=f: F.box_point(box, u, f))
                p = F.box_point(box, u, f)
                add_call("box_uf", [box_args, p[0], p[1]], lambda box=box, p=p: F.box_uf(box, p[0], p[1]))

    # ------------------------------------------------------------ 直线 / 曲线 / 区域
    for sh in SHAPES:
        add_call("funnel_lines", [sh], lambda sh=sh: F.funnel_lines(sh))
        add_call("funnel_segments", [sh], lambda sh=sh: F.funnel_segments(sh))
        add_call("funnel_strokes", [sh], lambda sh=sh: F.funnel_strokes(sh))
        add_call("funnel_curves", [sh, False], lambda sh=sh: F.funnel_curves(sh, False))
        add_call("funnel_curves", [sh, True], lambda sh=sh: F.funnel_curves(sh, True))
        add_call("funnel_polys", [sh, False], lambda sh=sh: F.funnel_polys(sh, False))
        add_call("funnel_polys", [sh, True], lambda sh=sh: F.funnel_polys(sh, True))
        for b, p in PROBES:
            add_call("funnel_contains", [sh, b, p], lambda sh=sh, b=b, p=p: F.funnel_contains(sh, b, p))
        add("funnel_reversed", [sh], F.funnel_reversed(sh))

    # ------------------------------------------------------------ line_band
    band_cases = [
        ([[0.0, 60.0], [4.0, 60.0]], 60), ([[0.0, 60.0], [4.0, 60.0]], 61),
        ([[0.0, 60.0], [4.0, 64.0]], 62), ([[0.0, 60.0], [4.0, 64.0]], 60),
        ([[0.0, 60.0], [4.0, 64.0]], 64), ([[4.0, 60.0], [0.0, 64.0]], 62),
        ([[2.0, 60.0], [2.0, 64.0]], 61), ([[2.0, 60.0], [2.0, 64.0]], 66),
        ([[0.0, 60.5], [4.0, 60.5]], 60), ([[0.0, 60.5], [4.0, 60.5]], 61),
        ([[0.0, 60.0], [4.0, 60.0]], 59),
    ]
    for (a, b), q in band_cases:
        add_call("line_band", [a, b, q], lambda a=a, b=b, q=q: F.line_band(a, b, q))

    # ------------------------------------------------------------ 布局 / 网格 / 音符
    configs = [
        (SH_SIMPLE, 960, {}),
        (SH_SIMPLE, 960, {"follow": "curve"}),
        (SH_SIMPLE, 960, {"change": "smooth", "gate0": 0.125, "gate1": 0.03125, "vary": True}),
        (SH_SIMPLE, 960, {"fill": "long"}),
        (SH_SIMPLE, 480, {"wall": "past"}),
        (SH_SIMPLE, 960, {"fill": "long", "wall": "past"}),
        (SH_EMPTY, 960, {}),
        (SH_EMPTY, 960, {"fill": "long"}),
        (SH_REVERSE, 960, {}),
        (SH_REVERSE, 960, {"fill": "long", "wall": "past"}),
        (SH_REVERSE, 960, {"follow": "curve", "change": "smooth", "gate0": 0.125, "gate1": 0.09375, "vary": True}),
        (SH_EXTRA, 960, {}),
        (SH_EXTRA, 960, {"fill": "long"}),
        (SH_HALF, 960, {}),
        (SH_VERT, 960, {}),
        (SH_VERT2, 960, {"fill": "long"}),
        (SH_LOWWALL, 960, {"fill": "long"}),
        (SH_LOWWALL, 960, {}),
        (SH_LOWWALL, 96, {}),
        (SH_POINT, 960, {}),
        (SH_COLLINEAR, 960, {}),
        (SH_PARALLEL, 960, {}),
        (SH_WALLPOINT, 960, {"wall": "past", "fill": "long"}),
        (SH_TILT, 960, {}),
        (SH_PULL, 960, {}),
    ]
    openness_ds = [-5.0, -1.0, 0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.5, 8.0, 9.0, 12.0, 100.0]
    for base, ppq, kw in configs:
        sh = with_settings(base, **kw)
        add_call("funnel_key_spans", [sh], lambda sh=sh: F.funnel_key_spans(sh))
        add_call("funnel_axis", [sh], lambda sh=sh: F.funnel_axis(sh, F.funnel_key_spans(sh)))
        add_call("funnel_layout", [sh, ppq], lambda sh=sh, ppq=ppq: F.funnel_layout(sh, ppq))
        add_call("funnel_cells", [sh, ppq], lambda sh=sh, ppq=ppq: F.funnel_cells(sh, ppq))
        add_call("funnel_note_count", [sh, ppq], lambda sh=sh, ppq=ppq: F.funnel_note_count(sh, ppq))
        add_call("funnel_notes", [sh, ppq], lambda sh=sh, ppq=ppq: F.funnel_notes(sh, ppq))
        lay = None
        try:
            lay = F.funnel_layout(sh, ppq)
        except Exception:
            pass
        if lay:
            dspans, walls, length, t0, sign = lay
            add_call("funnel_grid", [sh, ppq], lambda: F.funnel_grid(sh, ppq, dspans, length))
            w = F.funnel_openness(dspans)
            add("funnel_openness", [sh, ppq, openness_ds], [w(d) for d in openness_ds])
        else:
            add("funnel_openness", [sh, ppq, openness_ds], None)

    # 手写的 dspans：funnel_openness / funnel_grid 的纯函数面
    for dspans in (
        {60: [[0.0, 4.0], [6.0, 8.0]], 61: [[2.0, 10.0]], 62: [[8.0, 9.0]]},
        {60: [[0.0, 0.0]]},
        {},
        {64: [[1.0, 2.0], [3.0, 5.0]]},
    ):
        w = F.funnel_openness(dspans)
        add("funnel_openness_spans", [dspans, openness_ds], [w(d) for d in openness_ds])
        if dspans:
            sh = shape(LINE + WALL, starts=[])
            add_call("funnel_grid_spans", [sh, 960, dspans, 8.0],
                     lambda sh=sh, dspans=dspans: F.funnel_grid(sh, 960, dspans, 8.0))

    # funnel_gate
    for change in ("steps", "smooth"):
        sh = shape(LINE + WALL, starts=[], change=change)
        for g0, g1 in ((60.0, 60.0), (60.0, 30.0), (30.0, 120.0), (60.0, 40.0)):
            for w in (0.0, 0.25, 0.5, 0.75, 1.0, 1.5):
                add("funnel_gate", [sh, g0, g1, w], F.funnel_gate(sh, g0, g1, w))

    # ------------------------------------------------------------ new_start / 手柄 / 伙伴
    for sh in (SH_SIMPLE, SH_EXTRA2, SH_HALF, SH_POINT, SH_VERT, SH_COLLINEAR):
        for line in range(len(F.funnel_lines(sh))):
            for at in (0.0, 0.5, 1.0):
                add_call("new_start", [sh, at, line], lambda sh=sh, at=at, line=line: F.new_start(sh, at, line))
    for sh in (SH_SIMPLE, SH_PULL, SH_EXTRA):
        add("next_link", [sh], F.next_link(sh))
        for k in range(len(sh["starts"])):
            for end in (0, 1):
                add_call("partners", [sh, k, end], lambda sh=sh, k=k, end=end: F.partners(sh, k, end))
        add_call("funnel_handles", [sh], lambda sh=sh: F.funnel_handles(sh))
        add_call("funnel_handle_lines", [sh], lambda sh=sh: F.funnel_handle_lines(sh))
    add_call("partners", [SH_SIMPLE, 9, 0], lambda: F.partners(SH_SIMPLE, 9, 0))

    # ------------------------------------------------------------ 曲线变换
    for c, flip in ((curve(C2, sharp=[]), False), (curve(C2, sharp=[]), True), (curve(C2, sharp=[1]), True),
                    (curve(C1, sharp=[]), False)):
        add_call("turned_curve", [c, flip], lambda c=c, flip=flip: F.turned_curve(c, flip))
    add_call("turned", [C2], lambda: F.turned(C2))
    add_call("turned", [C1], lambda: F.turned(C1))
    for c in (curve(C2, sharp=[]), curve(C3, sharp=[1])):
        add_call("inside_out", [c], lambda c=c: F.inside_out(c))
    for a, b in ((curve(C1, link=3, flip=True), curve(C2, sharp=[1])),
                 (curve(C2, link=1), curve(C3, sharp=[]))):
        c = copy.deepcopy(a)
        F.set_shape(c, b)
        add("set_shape", [a, b], c)

    # ------------------------------------------------------------ remove_funnel_parts
    remove_cases = [
        (SH_EXTRA2, [2], []),
        (SH_EXTRA2, [0], [(0, 0)]),
        (SH_EXTRA2, [0, 1, 2], []),
        (SH_EXTRA2, [], []),
        (SH_EXTRA2, [1], [(0, 0)]),
        (SH_EXTRA, [1], []),
        (SH_SIMPLE, [0], []),
    ]
    for base, lines, curves in remove_cases:
        original = copy.deepcopy(base)
        sh = copy.deepcopy(base)
        ok = F.remove_funnel_parts(sh, lines, curves)
        add("remove_funnel_parts", [original, lines, curves], [ok, sh["pts"], sh["starts"]])

    # ------------------------------------------------------------ 旧版 bends / old_funnel
    old_cases = [
        {"pts": [[0.0, 60.0], [4.0, 64.0]], "sides": "one"},
        {"pts": [[0.0, 60.0], [4.0, 64.0]], "sides": "two"},
        {"pts": [[0.0, 64.0], [4.0, 60.0]], "sides": "two", "bend": [0.3, 0.7]},
        {"pts": [[0.0, 60.0], [0.0, 70.0]], "sides": "two", "bend": [0.5, 0.5]},
        {"pts": [[0.0, 60.0], [4.0, 64.0]]},
        {"pts": [[0.0, 60.0], [4.0, 64.0]], "sides": "two", "bend": None},
        {"pts": [[0.0, 60.0], [4.0, 64.0]], "sides": "two", "bend": [2.0, -1.0]},
        {"pts": [[0.0, 64.0], [4.0, 60.0]], "sides": "one", "bend": [0.01, 0.999]},
        {"pts": [[0.0, 60.0]], "sides": "one"},
        {"pts": [[0.0, 60.0], [4.0, 64.0]], "sides": "two", "bend": [0.5]},
    ]
    for old in old_cases:
        add_call("old_funnel", [old], lambda old=old: F.old_funnel(old))
        try:
            pts, starts = F.old_funnel(old)
        except Exception:
            continue
        add("clean_starts", [starts, 1], F.clean_starts(starts, 1))

    bend_cases = [
        [], [[0.75, 0.2]], [[0.75, 0.2], [0.2, 0.8]], [[2, -1], [-1, 2]],
        [[0.5, 0.5], [0.5, 0.5], [0.50005, 0.9]],
        [[0.01 + 0.02 * i, 0.5] for i in range(40)],
        [[0.75, 0.2], [0.75, 0.9]],
        ["ab"], [[0.5]], [[0.5, 0.5, "x"]], 5,
    ]
    for bends in bend_cases:
        add_call("clean_bends", [bends], lambda bends=bends: F.clean_bends(bends))
    for bends in ([[0.75, 0.2]], [[0.2, 0.8]], [[0.75, 0.2], [0.2, 0.8]],
                  [[0.2, 0.8], [0.5, 0.5], [0.75, 0.2]], [[0.5, 0.5]]):
        b = F.clean_bends(bends)
        add_call("old_curve_points", [b], lambda b=b: F.old_curve_points(b))

    # ------------------------------------------------------------ funnel_f / funnel_u / smooth_curve
    for bend in ([0.75, 0.2], [0.2, 0.8], [0.5, 0.5], [0.75, 0.75], [0.01, 0.999], [0.99, 0.001], [2, -1]):
        for x in (0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, 1.2):
            add("funnel_f", [bend, x], F.funnel_f(bend, x))
            add("funnel_u", [bend, x], F.funnel_u(bend, x))

    smooth_cases = [
        ([0.0, 0.5, 1.0], [0.0, 0.2, 1.0]),
        ([0.0, 1 / 3, 2 / 3, 1.0], [0.0, 0.1, 0.9, 1.0]),
        ([0.0, 0.25, 0.75, 1.0], [1.0, 0.5, 0.5, 0.0]),
        ([0.0, 0.5, 1.0], [0.5, 0.5, 0.5]),
        ([0.0, 0.4, 0.9, 1.0], [0.0, 0.6, 0.3, 1.0]),
    ]
    smooth_us = [i / 20 for i in range(-2, 23)]
    for xs, ys in smooth_cases:
        fn = F._smooth_curve(xs, ys)
        add("smooth_curve", [xs, ys, smooth_us], [fn(u) for u in smooth_us])

    # ------------------------------------------------------------ 公式 / 预设
    for name in FORMULAS:
        for n in (1, 2, 7, 64, 400):
            add_call("formula_curve", [name, n], lambda name=name, n=n: F.formula_curve(FORMULAS[name], n))
    for name in FORMULAS:
        add_call("preset_curve", [name], lambda name=name: F.preset_curve(FORMULAS[name]))
    add("preset_curve", [None], F.preset_curve(None))
    add("curve_presets", [], [[name, formula] for name, formula in F.CURVE_PRESETS])

    # ------------------------------------------------------------ 清洗
    clean_funnel_cases = [
        {},
        {"fill": "long", "change": "smooth", "follow": "curve", "wall": "past",
         "gate0": 0.125, "gate1": 0.5, "vary": True},
        {"fill": "x", "change": "x", "follow": "x", "wall": "x"},
        {"gate0": 0.25, "gate1": 0.125},
        {"gate0": 0.25, "gate1": 0.25},
        {"gate0": 0.25, "gate1": 0.125, "vary": False},
        {"gate0": 0.25, "gate1": 0.125, "vary": 1},
        {"gate0": 0.25, "gate1": 0.125, "vary": ""},
        {"gate0": -5, "gate1": 0.0},
        {"gate0": "0.5", "gate1": "0.25"},
        {"gate0": None},
        {"gate0": "abc"},
        5,
        [],
    ]
    for sh in clean_funnel_cases:
        add_call("clean_funnel", [sh], lambda sh=sh: F.clean_funnel(sh))

    clean_curve_cases = [
        None, [], {}, {"pts": []}, {"pts": [[0, 0], [1, 1]]},
        {"pts": {}}, {"pts": ""}, {"pts": 0},
        curve(C2), curve(C2, sharp=[1, 1, 5, -1, 0]), curve(C2, sharp=[2]),
        {"pts": C2, "sharp": {}}, {"pts": C2, "sharp": ""},
        {"pts": C2, "sharp": [1], "link": 3, "flip": True},
        {"pts": C2, "link": "2"}, {"pts": C2, "link": 2.7},
        {"pts": [[0, 0], [0.1, 0.1], [0.2, 0.2], [1, 2]]},
        {"pts": C2 + [[0.5, 0.5]]},
        [[0.75, 0.2]],
        [[0.75, 0.2], [0.2, 0.8]],
        [[0.2, 0.8], [0.75, 0.2], [0.5, 0.6]],
        [[[0.2, 0.8], [0.75, 0.2]] for _ in range(40)],
        [[0.2 + 0.001 * i, 0.8 - 0.001 * i] for i in range(40)],
        [[0.75, 0.2], [0.75001, 0.9]],
        "x", 5, True,
        {"pts": [[0, 0], [0.1, "bad"], [0.2, 0.2], [1, 1]]},
        {"pts": C2, "sharp": ["x"]},
        {"pts": C2, "sharp": 5},
        [["a", "b"]],
    ]
    for c in clean_curve_cases:
        add_call("clean_curve", [c], lambda c=c: F.clean_curve(c))

    clean_starts_cases = [
        (None, 3), ([], 3), ({}, 3), ("", 3),
        ([start(0, 0.5, [curve(C1), curve(C2)])], 3),
        ([start(2, 0.5, [curve(C1), None])], 3),
        ([start(0, -1.0, [curve(C1), None]), start(0, 2.0, [curve(C1), None])], 3),
        ([start(0, 0.5, [curve(C1), None]), start(0, 0.5, [curve(C1), None])], 3),
        ([start(4, 0.5, [curve(C1), curve(C2)])], 3),
        ([{"ends": [curve(C1), None]}], 3),
        ([{"line": 1, "at": 0.25, "ends": [curve(C1, link=4, flip=True), None]}], 3),
        ([{"line": 0, "at": 0.5, "ends": [[[0.75, 0.2]], None]}], 3),
        ([{"line": 0, "at": 0.5, "ends": [[[0.75, 0.2]], [[0.2, 0.8]]]}], 3),
        ([{"line": 0, "at": 0.5, "ends": [[[0.2, 0.8]]]},
          {"line": 1, "at": 0.25, "ends": [[[0.5, 0.5]], [[0.75, 0.2]]]}], 3),
        ([{"line": 0, "at": 0.5, "ends": "x"}], 3),
        ([{"line": 0, "at": 0.5, "ends": [[0.75, 0.2], None]}], 3),
        ([{"line": "x", "at": 0.5, "ends": [curve(C1), None]}], 3),
        ([{"line": 0, "at": 0.5, "ends": [{"pts": [[0, 0], [1, 1]]}, None]}], 3),
        ([5], 3),
        ([start(0, 0.5, [curve(C1), None])], 0),
    ]
    for starts, lines in clean_starts_cases:
        add_call("clean_starts", [starts, lines], lambda starts=starts, lines=lines: F.clean_starts(starts, lines))

    return cases


if __name__ == "__main__":
    data = gen()
    write("funnel", data)
