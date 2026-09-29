"""engine.py 的对照向量。

Python 原版（notes/engine.py）直接跑：形状 dict 先过一遍 clean_shape（和实际工程一样），
输出写到 crates/spiderweb-core/tests/vectors/engine.json，Rust 测试逐用例对照。
"""

import copy
import math

import numpy as np

from vec_common import write

from notes import custom as C
from notes import engine as E

# ---------------------------------------------------------------- 小工具


def to_json(x):
    """转成 JSON 能序列化的形式（递归处理 numpy 与元组）。"""
    if isinstance(x, dict):
        return {str(k): to_json(v) for k, v in x.items()}
    if isinstance(x, np.ndarray):
        return x.tolist()
    if isinstance(x, np.integer):
        return int(x)
    if isinstance(x, np.floating):
        return float(x)
    if isinstance(x, np.bool_):
        return bool(x)
    if isinstance(x, (list, tuple)):
        return [to_json(v) for v in x]
    return x


def shape(kind, pts, **kw):
    """一个原始形状 dict -> clean_shape 后的有效形状（和工程的形状一样）。"""
    d = {
        "kind": kind,
        "pts": [[float(b), float(p)] for b, p in pts],
        "vel0": kw.pop("vel0", 127.0),
        "vel1": kw.pop("vel1", 127.0),
        "end_dot": kw.pop("end_dot", False),
    }
    d.update(kw)
    out = E.clean_shape(d)
    assert out is not None, d
    return out


def stroke(kind, pts, **kw):
    d = {"kind": kind, "pts": [[float(u), float(v)] for u, v in pts]}
    d.update(kw)
    return d


def curve(pts, sharp=None, sym=None):
    d = stroke("curve", pts)
    if sharp is not None:
        d["sharp"] = [int(a) for a in sharp]
    if sym is not None:
        d["sym"] = sym
    return d


def start(line, at, ends):
    return {"line": int(line), "at": float(at), "ends": copy.deepcopy(ends)}


def tumour(**kw):
    d = {"on": True, "shape": "triangle", "size": 3.0, "length": 0.25, "dist": 0.125, "side": "alt",
         "wrap": "simple", "start": 0.0, "end": 1.0, "ease": 0.0, "fit": False, "seed": 1, "mirror": False,
         "k": 0.25}
    d.update(kw)
    return d


def notes_rows(rows):
    return C.pack_notes(np.asarray(rows, np.int64))


# ---------------------------------------------------------------- 常用形状

C1 = [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]]
C2 = [[0.0, 0.0], [0.1, 0.05], [0.2, 0.1], [0.5, 0.5], [0.6, 0.7], [0.8, 0.9], [1.0, 1.0]]

LINE_PTS = [[0.0, 60.0], [8.0, 64.0]]
WALL_PTS = [[8.0, 66.0], [8.0, 62.0]]
FN_STARTS = [start(0, 0.25, [curve(C1), curve(C2, sharp=[1])])]

BOX = [[0.0, 60.0], [4.0, 60.0], [0.0, 64.0]]
SQUARE = [stroke("poly", [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]])]
TRIANGLE = [stroke("poly", [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.0, 0.0]])]
ELLIPSE = [stroke("ellipse", [], box=[0.2, 0.2, 0.8, 0.8])]
CURVE_ST = [curve([[0.0, 0.0], [0.3, 0.0], [0.7, 1.0], [1.0, 1.0]])]
ARC_ST = [stroke("arc", [[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]], k=1.0)]
FREE_ST = [stroke("poly", [[0.0, 0.0], [0.2, 0.03], [0.35, -0.02], [0.5, 0.04], [0.65, -0.03], [0.8, 0.02],
                           [1.0, 0.0]], free=True, smooth=50, k=1.0)]

NOTE_ROWS = [[0, 480, 0, 100, 0], [0, 240, 4, 90, 1], [240, 720, 2, 80, 0], [480, 1440, 5, 70, 2],
             [720, 960, 0, 127, 1]]
NOTE_ROWS_DUP = [[0, 240, 0, 100, 0], [0, 240, 0, 100, 0], [0, 240, 0, 90, 1], [240, 480, 2, 100, 1]]


def custom(strokes, pts=BOX, **kw):
    kw.setdefault("name", "seed")
    return shape("custom", pts, strokes=copy.deepcopy(strokes), **kw)


def funnel_opts(fill="spam", fill_strokes=True, extra=False, reverse=False, wall="in"):
    """常用漏斗设置。"""
    if reverse:
        pts = [[8.0, 60.0], [2.0, 64.0], [2.0, 66.0], [2.0, 62.0]]
    else:
        pts = copy.deepcopy(LINE_PTS + WALL_PTS)
    if extra:
        pts = pts + [[1.0, 58.0], [7.0, 60.0]]
    starts = copy.deepcopy(FN_STARTS) if fill_strokes else []
    return shape("funnel", pts, starts=starts, fill=fill, wall=wall)


# ---------------------------------------------------------------- 用例生成

def gen():
    cases = []

    def add(fn, args, out):
        cases.append({"fn": fn, "args": to_json(args), "out": to_json(out)})

    # ------------------------------------------------------------ make_shape
    defaults = {"vel0": 100.0, "vel1": 80.0, "end_dot": True}
    ms_pts = [[0.0, 60.0], [4.0, 64.0], [8.0, 62.0]]
    for kind in ("line", "poly", "free", "curve", "arc", "custom", "funnel"):
        add("make_shape", [kind, ms_pts, defaults], E.make_shape(kind, ms_pts, defaults))
    add("make_shape", ["line", [[1.0, 60.0]], defaults], E.make_shape("line", [[1.0, 60.0]], defaults))
    add("make_shape", ["curve", [[0.0, 60.0], [4.0, 65.0]], defaults],
        E.make_shape("curve", [[0.0, 60.0], [4.0, 65.0]], defaults))

    # ------------------------------------------------------------ point_names
    add("point_names", [shape("curve", [[0.0, 60.0], [1.0, 62.0], [3.0, 62.0], [4.0, 60.0]])],
        E.point_names(shape("curve", [[0.0, 60.0], [1.0, 62.0], [3.0, 62.0], [4.0, 60.0]])))
    cp7 = [[0.0, 60.0], [1.0, 62.0], [2.0, 61.0], [3.0, 60.0], [4.0, 58.0], [5.0, 59.0], [6.0, 60.0]]
    add("point_names", [shape("curve", cp7)], E.point_names(shape("curve", cp7)))
    cp10 = [[float(i), 60.0 + (i % 3)] for i in range(10)]
    add("point_names", [shape("curve", cp10)], E.point_names(shape("curve", cp10)))
    cp5 = [[0.0, 60.0], [1.0, 62.0], [2.0, 61.0], [3.0, 60.0], [4.0, 58.0]]
    add("point_names", [shape("curve", cp5)], E.point_names(shape("curve", cp5)))
    add("point_names", [shape("funnel", LINE_PTS + WALL_PTS, starts=[])],
        E.point_names(shape("funnel", LINE_PTS + WALL_PTS, starts=[])))
    add("point_names", [funnel_opts(extra=True)], E.point_names(funnel_opts(extra=True)))
    add("point_names", [shape("funnel", LINE_PTS + WALL_PTS + [[1.0, 58.0], [7.0, 60.0], [2.0, 62.0], [6.0, 63.0]],
                            starts=[])],
        E.point_names(shape("funnel", LINE_PTS + WALL_PTS + [[1.0, 58.0], [7.0, 60.0], [2.0, 62.0], [6.0, 63.0]],
                            starts=[])))
    add("point_names", [shape("line", [[0.0, 60.0], [4.0, 64.0]])],
        E.point_names(shape("line", [[0.0, 60.0], [4.0, 64.0]])))
    add("point_names", [shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=1.0)],
        E.point_names(shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=1.0)))
    add("point_names", [shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]])],
        E.point_names(shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]])))
    add("point_names", [shape("free", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]])],
        E.point_names(shape("free", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]])))
    add("point_names", [custom(SQUARE)], E.point_names(custom(SQUARE)))

    # ------------------------------------------------------------ shape_path / shape_strokes
    for tag, sh in (
        ("line", shape("line", [[0.0, 60.0], [4.0, 64.0]])),
        ("poly", shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]])),
        ("curve", shape("curve", [[0.0, 60.0], [1.0, 63.0], [3.0, 57.0], [4.0, 60.0]])),
        ("arc", shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=1.0)),
        ("free-smooth", shape("free", [[0.0, 60.0], [1.0, 60.4], [2.0, 59.6], [3.0, 60.3], [4.0, 60.0]],
                              smooth=60, k=1.0)),
        ("tumour", shape("line", [[0.0, 60.0], [4.0, 64.0]], tumour=tumour(size=3.0))),
        ("custom", custom(SQUARE)),
        ("funnel", funnel_opts(fill_strokes=False)),
    ):
        add("shape_path", [sh], E.shape_path(sh))
        add("shape_strokes", [sh], E.shape_strokes(sh))
    add("shape_strokes", [custom([stroke("poly", [[0.0, 0.0], [1.0, 1.0]]), TRIANGLE[0]])],
        E.shape_strokes(custom([stroke("poly", [[0.0, 0.0], [1.0, 1.0]]), TRIANGLE[0]])))

    # ------------------------------------------------------------ shape_notes：line / poly / free
    def add_notes(sh, ppq=960, keys=128):
        add("shape_notes", [sh, ppq, keys], E.shape_notes(sh, ppq, keys))

    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]]))
    add_notes(shape("line", [[4.0, 64.0], [0.0, 60.0]]))  # 反向
    add_notes(shape("line", [[0.0, 60.0], [4.0, 60.0]]))  # 水平
    add_notes(shape("line", [[0.0, 60.0], [0.0, 64.0]]))  # 竖直
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], end_dot=True))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], end_dot=True, vel0=90.0, vel1=60.0))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], vel_env=[[0.0, 30.0], [0.4, 110.0], [0.4, 40.0],
                                                                  [1.0, 100.0]]))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], vel_env=[[0.0, 100.5], [1.0, 101.5]]))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], vel0=100.0, vel1=100.0))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], vel_env=[[0.0, 64.0], [0.5, 64.0], [1.0, 64.0]]))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], vel0=1.0, vel1=127.0))
    add_notes(shape("line", [[-0.5, 60.0], [2.0, 64.0]]))  # 起点夹到 0
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]]), 480)
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], tumour=tumour(size=2.0)))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], tumour=tumour(size=2.0, shape="square", dist=0.25)))
    add_notes(shape("line", [[0.0, 60.0], [4.0, 64.0]], tumour=tumour(on=False, size=2.0)))
    add_notes(shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 62.0]]))
    add_notes(shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 62.0]], end_dot=True))
    add_notes(shape("poly", [[4.0, 62.0], [2.0, 64.0], [0.0, 60.0]]))
    add_notes(shape("poly", [[0.0, 60.0], [4.0, 60.0]]))
    add_notes(shape("poly", [[0.0, 60.0], [2.0, 64.0], [4.0, 62.0]], tumour=tumour(size=1.5)))
    add_notes(shape("poly", [[0.0, 60.0], [1.0, 62.0], [2.0, 61.0], [3.0, 64.0], [4.0, 60.0]]))
    add_notes(shape("free", [[0.0, 60.0], [1.0, 62.0], [2.0, 61.0], [3.0, 63.0], [4.0, 60.5]]))
    add_notes(shape("free", [[0.0, 60.0], [1.0, 60.6], [2.0, 59.4], [3.0, 60.7], [4.0, 60.0]], smooth=40, k=1.0))
    add_notes(shape("free", [[0.0, 60.0], [1.0, 60.6], [2.0, 59.4], [3.0, 60.7], [4.0, 60.0]], smooth=100, k=1.0))
    add_notes(shape("free", [[0.0, 60.0], [2.0, 60.5], [4.0, 59.5], [6.0, 60.4], [8.0, 60.0]], smooth=30, k=2.0))
    add_notes(shape("free", [[0.0, 60.0], [1.0, 65.0], [2.0, 55.0], [3.0, 64.0], [4.0, 56.0],
                             [5.0, 63.0], [6.0, 57.0], [7.0, 62.0], [8.0, 60.0]], smooth=70, k=1.0))

    # ------------------------------------------------------------ shape_notes：curve / arc
    add_notes(shape("curve", [[0.0, 60.0], [1.0, 63.0], [3.0, 57.0], [4.0, 60.0]]))
    add_notes(shape("curve", [[4.0, 60.0], [3.0, 57.0], [1.0, 63.0], [0.0, 60.0]]))
    add_notes(shape("curve", [[0.0, 60.0], [1.0, 64.0], [2.0, 64.0], [3.0, 60.0], [4.0, 56.0], [5.0, 56.0],
                              [6.0, 60.0]]))
    add_notes(shape("curve", [[float(i), 60.0 + math.sin(i) * 3.0] for i in range(10)]))
    add_notes(shape("curve", [[0.0, 60.0], [1.0, 63.0], [3.0, 57.0], [4.0, 60.0]], end_dot=True))
    add_notes(shape("curve", [[0.0, 60.0], [1.0, 63.0], [3.0, 57.0], [4.0, 60.0]], vel_env=[[0.0, 20.0], [1.0, 120.0]]))
    add_notes(shape("curve", [[0.0, 60.0], [1.0, 64.0], [3.0, 56.0], [4.0, 60.0]], tumour=tumour(size=1.0)))
    add_notes(shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=1.0))
    add_notes(shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=2.0))
    add_notes(shape("arc", [[4.0, 60.0], [2.0, 64.0], [0.0, 60.0]], k=1.0))
    add_notes(shape("arc", [[0.0, 60.0], [2.0, 60.0], [4.0, 60.0]], k=1.0))  # 共线
    add_notes(shape("arc", [[0.0, 60.0], [2.0, 64.0], [4.0, 60.0]], k=1.0, end_dot=True))

    # ------------------------------------------------------------ shape_notes：custom
    add_notes(custom(SQUARE, fill="empty"))
    add_notes(custom(SQUARE, fill="fill"))
    add_notes(custom(SQUARE, fill="spam"))
    add_notes(custom(SQUARE, fill="spam", align="aligned"))
    add_notes(custom(SQUARE, fill="spam", gate=0.125))
    add_notes(custom(SQUARE, fill="outline_spam"))
    add_notes(custom(TRIANGLE, fill="fill"))
    add_notes(custom(TRIANGLE, fill="spam"))
    add_notes(custom(ELLIPSE, fill="fill"))
    add_notes(custom(ELLIPSE, fill="spam"))
    add_notes(custom(CURVE_ST, fill="empty"))
    # （开放笔画 + spam：1.2.0 由 fill_plan 用直线补缺口，属 custom.py 的 Fills/Ends 移植，另有 agent；
    # 这条用例先不生成，免得把它的行为算进 256 键的向量里）
    add_notes(custom([stroke("poly", [[0.0, 0.0], [1.0, 1.0], [0.5, 0.5]])], fill="fill"))
    add_notes(custom(ARC_ST, fill="fill"))
    add_notes(custom(FREE_ST, fill="empty"))
    add_notes(custom([stroke("poly", [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]),
                      stroke("poly", [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8], [0.2, 0.2]])], fill="fill"))
    add_notes(custom(SQUARE, notes=notes_rows(NOTE_ROWS)), )
    add_notes(custom(SQUARE, notes=notes_rows(NOTE_ROWS), own_vel=True))
    add_notes(custom(SQUARE, notes=notes_rows(NOTE_ROWS_DUP), own_vel=True))
    add_notes(custom(SQUARE, notes=notes_rows(NOTE_ROWS_DUP)), )
    add_notes(custom(SQUARE, notes=notes_rows(NOTE_ROWS), own_vel=True, vel_env=[[0.0, 30.0], [1.0, 90.0]]))
    add_notes(custom(SQUARE, fill="fill", vel0=40.0, vel1=120.0))
    add_notes(custom(SQUARE, pts=BOX, fill="spam", gate=0.03125))
    add_notes(custom(SQUARE, pts=BOX, fill="spam", align="aligned"), 480)

    # ------------------------------------------------------------ shape_notes：funnel
    add_notes(funnel_opts(fill="spam"))
    add_notes(funnel_opts(fill="long"))
    add_notes(funnel_opts(fill="spam", fill_strokes=False))
    add_notes(funnel_opts(fill="spam", reverse=True))
    add_notes(funnel_opts(fill="long", reverse=True))
    add_notes(funnel_opts(fill="spam", extra=True))
    add_notes(funnel_opts(fill="long", extra=True))
    add_notes(funnel_opts(fill="spam", wall="past"))
    add_notes(funnel_opts(fill="long", wall="past"))
    add_notes(funnel_opts(fill="spam"), 480)
    add_notes(funnel_opts(fill="spam"), 96)

    # ------------------------------------------------------------ shape_notes：256 键
    add_notes(shape("line", [[0.0, 120.0], [4.0, 140.0]]), 960, 256)
    add_notes(shape("line", [[0.0, 120.0], [4.0, 140.0]]), 960, 128)  # 同样的形状：128 键滤掉 >127
    add_notes(shape("line", [[0.0, 124.0], [0.0, 130.0]]), 960, 256)
    add_notes(shape("line", [[0.0, 200.0], [0.0, 200.0]]), 960, 256)
    add_notes(shape("line", [[0.0, 300.0], [0.0, 300.0]]), 960, 256)  # 超过 255：滤掉
    add_notes(shape("poly", [[0.0, 126.0], [4.0, 134.0]], end_dot=True), 960, 256)
    add_notes(shape("custom", [[0.0, 124.0], [4.0, 124.0], [0.0, 130.0]],
                    strokes=[stroke("poly", [[0.0, 0.0], [1.0, 0.0]])]), 960, 256)
    add_notes(custom(SQUARE, pts=[[0.0, 124.0], [4.0, 124.0], [0.0, 130.0]], fill="spam"), 960, 256)
    pasted_high = custom(SQUARE, pts=[[0.0, 124.0], [4.0, 124.0], [0.0, 130.0]],
                         notes=notes_rows([[0, 480, 130, 100, 0], [0, 240, 126, 90, 1]]))
    add_notes(pasted_high, 960, 256)
    add_notes(pasted_high, 960, 128)

    # ------------------------------------------------------------ shape_notes_tracks
    def add_tracks(sh, ppq=960, keys=128):
        add("shape_notes_tracks", [sh, ppq, keys], E.shape_notes_tracks(sh, ppq, keys))

    # Strokes from different shapes (convert.py's "src"): every note carries its source group.
    def src_custom(strokes, **kw):
        return custom([dict(s, src=i % 2) for i, s in enumerate(strokes)], **kw)

    src_line = src_custom([SQUARE[0], TRIANGLE[0]], fill="empty")
    add_tracks(src_line)
    add_notes(src_line)
    add_notes(dict(src_line, fill="outline_spam", gate=0.25))
    # Overlapping sources: the same note from two groups stays twice (they can get channels apart).
    overlap_src = src_custom([SQUARE[0], SQUARE[0]], fill="empty")
    add_tracks(overlap_src)

    pasted = custom(SQUARE, notes=notes_rows(NOTE_ROWS))
    add_tracks(pasted)
    pasted_own = custom(SQUARE, notes=notes_rows(NOTE_ROWS), own_vel=True)
    add_tracks(pasted_own)
    pasted_dup = custom(SQUARE, notes=notes_rows(NOTE_ROWS_DUP))
    add_tracks(pasted_dup)
    pasted_dup_own = custom(SQUARE, notes=notes_rows(NOTE_ROWS_DUP), own_vel=True)
    add_tracks(pasted_dup_own)
    line_sh = shape("line", [[0.0, 60.0], [4.0, 64.0]])
    add_tracks(line_sh)
    fun_sh = funnel_opts(fill="spam")
    add_tracks(fun_sh)
    pasted_high_own = custom(SQUARE, pts=[[0.0, 124.0], [4.0, 124.0], [0.0, 130.0]],
                             notes=notes_rows([[0, 480, 130, 100, 0], [0, 240, 126, 90, 1]]), own_vel=True)
    add_tracks(pasted_high_own, 960, 256)
    add_tracks(pasted_high_own, 960, 128)

    # ------------------------------------------------------------ assign_slots
    def add_slots(lists, split, apart=()):
        args = [np.asarray(a, np.int64).reshape(-1, 4) for a in lists]
        add("assign_slots", [lists, split, [list(g) for g in apart]],
            E.assign_slots(args, split, apart))

    add_slots([[[0, 200, 60, 100]], [[100, 300, 60, 100]]], "key")
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]]], "key")
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]]], "time")
    add_slots([[[0, 100, 60, 100], [100, 200, 60, 100], [200, 300, 60, 100]], [[150, 250, 60, 100]]], "key")
    add_slots([[[0, 300, 60, 100]], [[100, 200, 60, 100]]], "key")
    add_slots([[[0, 100, 60, 100]], [[50, 150, 60, 100]], [[120, 200, 60, 100]]], "key")
    add_slots([[[0, 100, 60, 100]], [[50, 150, 64, 100]], [[120, 200, 60, 100]]], "time")
    add_slots([[], [[0, 100, 60, 100]]], "key")
    add_slots([[[0, 0, 60, 100]], [[0, 100, 60, 100]]], "key")
    add_slots([[[10, 20, 60, 100], [20, 30, 60, 100]], [[15, 25, 60, 100]], [[5, 8, 60, 100]]], "key")
    add_slots([[[0, 10, 60, 100], [10, 20, 60, 100], [30, 40, 60, 100]], [[15, 35, 60, 100]]], "key")
    add_slots([[[0, 100, 60, 100]], [[0, 100, 60, 100]], [[0, 100, 60, 100]]], "key")
    # apart: the lists of one group (a custom shape's outline/inside, convert.py's sources) never share a slot
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]]], "key", apart=[[0, 1]])
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]]], "time", apart=[[0, 1]])
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]], [[50, 150, 62, 100]]], "key", apart=[[0, 2]])
    add_slots([[[0, 200, 60, 100]], [[100, 300, 64, 100]], [[50, 150, 62, 100]]], "key", apart=[[0, 1, 2]])
    # apart splits them even when they don't overlap
    add_slots([[[0, 100, 60, 100]], [[200, 300, 60, 100]]], "key", apart=[[0, 1]])

    # ------------------------------------------------------------ resolve_overlaps
    def add_resolve(rows):
        add("resolve_overlaps", [rows], E.resolve_overlaps(np.asarray(rows, np.int64)))

    add_resolve([])
    add_resolve([[0, 100, 60, 50, 0, 0], [0, 200, 60, 100, 0, 0]])
    add_resolve([[0, 200, 60, 80, 0, 0], [0, 100, 60, 80, 0, 0]])
    add_resolve([[0, 200, 60, 50, 0, 0], [100, 150, 60, 50, 0, 0]])
    add_resolve([[0, 100, 60, 50, 0, 0], [100, 200, 60, 50, 0, 0]])
    add_resolve([[0, 100, 60, 50, 0, 0], [150, 200, 60, 50, 0, 0]])
    add_resolve([[0, 100, 60, 50, 0, 0], [50, 80, 60, 60, 0, 0], [70, 200, 60, 40, 0, 0]])
    add_resolve([[100, 200, 60, 50, 1, 0], [0, 100, 60, 50, 0, 0], [150, 250, 60, 50, 1, 0],
                 [50, 150, 60, 50, 0, 0]])
    add_resolve([[0, 100, 60, 50, 0, 0], [0, 100, 60, 50, 1, 0], [40, 120, 60, 50, 0, 1]])
    add_resolve([[0, 50, 60, 50, 0, 0], [0, 0, 62, 50, 0, 0], [0, 80, 64, 50, 0, 0]])
    add_resolve([[0, 0, 60, 50, 0, 0], [0, 0, 62, 50, 0, 0]])
    add_resolve([[0, 100, 60, 50, 0, 0], [20, 30, 60, 90, 0, 0], [25, 150, 60, 70, 0, 0]])
    # 256 键：slot 1 的 key 2 不能和 slot 0 的 key 130 算同一组（slot * 256 + key）
    add_resolve([[0, 100, 130, 50, 0, 0], [50, 80, 2, 60, 1, 0], [0, 200, 130, 50, 0, 0]])

    # ------------------------------------------------------------ render
    def add_render(lists, mode, split, tracks=None, apart=None, ppq=960):
        args_lists = [np.asarray(a, np.int64).reshape(-1, 4) for a in lists]
        args_tracks = None if tracks is None else [None if t is None else np.asarray(t, np.int64) for t in tracks]
        add("render", [lists, mode, split, tracks, apart], E.render(args_lists, mode, split, args_tracks, apart))

    A = [[0, 200, 60, 100]]
    B = [[100, 300, 60, 90]]
    add_render([A, B], "raw", "key")
    add_render([A, B], "single", "key")
    add_render([A, B], "auto", "key")
    add_render([A, B], "auto", "time")
    C_ = [[100, 300, 64, 90]]
    add_render([A, C_], "auto", "key")
    add_render([A, C_], "auto", "time")
    add_render([A, B, [[250, 400, 60, 80]]], "auto", "key")
    add_render([A, B], "raw", "time", tracks=[None, None])
    add_render([A, B], "single", "time", tracks=[None, None])
    add_render([[[0, 100, 60, 100], [200, 300, 60, 100]]], "auto", "key", tracks=[[0, 1]])
    add_render([[[0, 100, 60, 100], [200, 300, 60, 100]]], "auto", "key", tracks=[[1, 1]])
    add_render([[[0, 100, 60, 100], [200, 300, 60, 100]], A], "auto", "key", tracks=[[0, 1], None])
    add_render([[[0, 100, 60, 100]], A], "auto", "key", tracks=[[2], None])
    add_render([[], A], "auto", "key", tracks=[[], None])
    add_render([[], A], "auto", "key", tracks=[[5], None])
    add_render([[], []], "auto", "key")
    add_render([[], []], "single", "key")
    add_render([[], []], "raw", "key")
    add_render([], "auto", "key")
    add_render([], "single", "key")
    add_render([[[0, 100, 60, 100], [100, 200, 60, 100]], [[50, 150, 60, 100]]], "single", "key")
    add_render([[[0, 100, 60, 100], [100, 200, 60, 100]], [[50, 150, 60, 100]]], "raw", "key")
    add_render([[[0, 100, 60, 100], [0, 100, 60, 100]]], "single", "key")
    add_render([[[0, 100, 60, 100], [50, 150, 64, 80]]], "single", "key")

    # convert.py's source groups (tracks) + Fill / Spam "Outline" (apart)
    src_notes, src_tracks = E.shape_notes_tracks(src_line, 960)
    add_render([src_notes.tolist(), B], "auto", "key",
               tracks=[src_tracks.tolist(), None], apart=[False, False])
    add_render([src_notes.tolist(), B], "auto", "time",
               tracks=[src_tracks.tolist(), None], apart=[False, False])
    apart_shape = custom(SQUARE, fill="fill", apart=True)
    ap_notes, ap_tracks = E.shape_notes_tracks(apart_shape, 960)
    add_render([ap_notes.tolist()], "auto", "key", tracks=[ap_tracks.tolist()], apart=[True])
    spam_apart = custom(SQUARE, fill="spam", apart=True, gate=0.125)
    sp_notes, sp_tracks = E.shape_notes_tracks(spam_apart, 960)
    add_render([sp_notes.tolist()], "auto", "key", tracks=[sp_tracks.tolist()], apart=[True])
    add_render([sp_notes.tolist(), A], "auto", "key",
               tracks=[sp_tracks.tolist(), None], apart=[True, False])
    add_render([sp_notes.tolist(), A], "auto", "time",
               tracks=[sp_tracks.tolist(), None], apart=[True, False])

    # ------------------------------------------------------------ slot_track_channel
    for slot in (0, 1, 8, 9, 10, 14, 15, 16, 30, 45):
        add("slot_track_channel", [slot], E.slot_track_channel(slot))

    return cases


if __name__ == "__main__":
    data = gen()
    write("engine", data)
