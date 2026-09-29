"""Reference vectors for joined.py.

Generated against the 1.2.0 sources:
    SPIDERWEB_SRC=/path/to/Spiderweb-1.2.0/scripts python3 tools/gen_joined_vectors.py

Input floats are quantized to 15 significant digits first (as in gen_bezier_vectors) so the default
serde_json float parsing loses nothing; outputs keep the original full precision.
"""

import copy
import math
import os
import sys

from vec_common import write

_SCRIPTS = (os.environ.get("SPIDERWEB_SCRIPTS") or os.environ.get("SPIDERWEB_SRC")
            or "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from notes import joined as J
from notes import tumour as T


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


def sh(kind, pts, **kw):
    d = {"kind": kind, "pts": [list(p) for p in pts]}
    d.update(kw)
    return d


def tm(**kw):
    d = {"on": True}
    d.update(kw)
    return T.clean_tumour(d)  # all settings present, so tumour_path works


def touch(tol):
    return lambda p, q: math.hypot(p[0] - q[0], p[1] - q[1]) <= tol


# ---- inputs

LINE1 = sh("line", [[0.0, 60.0], [4.0, 60.0]])
LINE2 = sh("line", [[4.0, 60.0], [8.0, 64.0]])
LINE_FAR = sh("line", [[6.0, 60.0], [10.0, 60.0]])
POLY1 = sh("poly", [[0.0, 60.0], [2.0, 62.0], [4.0, 60.0]])
CURVE1 = sh("curve", [[0.0, 60.0], [1.0, 61.0], [2.0, 61.0], [3.0, 60.0]],
            sharp=[1])
ARC1 = sh("arc", [[4.0, 60.0], [6.0, 64.0], [8.0, 60.0]], k=1.0)
FREE1 = sh("free", [[0.0, 60.0], [1.0, 60.5], [2.0, 60.0], [3.0, 60.5], [4.0, 60.0]],
           smooth=0, k=1.0)
FREE_SMOOTH = sh("free", [[0.0, 60.0], [1.0, 62.0], [2.0, 58.0], [3.0, 62.0], [4.0, 60.0]],
                 smooth=50, k=1.0)
TOUCH_TM = tm(size=3.0, length=0.5, dist=0.5, side="alt", k=1.0)


def joined(shapes, k=1.0, tol=1e-9):
    got = J.join_shapes(copy.deepcopy(shapes), k, touch(tol))
    return got


def gen():
    cases = []

    # ---- to_bezier
    for shape, k in ((LINE1, 1.0), (POLY1, 1.0), (CURVE1, 1.0), (ARC1, 1.0),
                     (FREE1, 1.0), (FREE1, 0.5), (FREE_SMOOTH, 1.0),
                     (sh("poly", [[0.0, 60.0], [1.0, 61.0], [2.0, 60.0]]), 2.0)):
        add(cases, "to_bezier", [shape, k], lambda s, kk: {
            "pts": [[p[0], p[1]] for p in J.to_bezier(s, kk)[0]],
            "sharp": list(J.to_bezier(s, kk)[1]),
        })

    # ---- reversed_bezier / reversed_tumour
    for pts, sharp in (
        (CURVE1["pts"], [1]),
        (CURVE1["pts"], []),
        ([[0.0, 60.0], [1.0, 61.0], [2.0, 60.0], [3.0, 59.0]], [1],),
        ([[0.0, 60.0], [1.0, 61.0], [2.0, 60.0], [3.0, 59.0], [4.0, 60.0],
          [5.0, 61.0], [6.0, 60.0]], [0, 1, 2]),
    ):
        add(cases, "reversed_bezier", [pts, sharp],
            lambda p, s: [J.reversed_bezier(p, s)[0], J.reversed_bezier(p, s)[1]])
    for t in (tm(), tm(mirror=True, start=0.25, end=0.75), tm(start=0.1, end=0.9, side="left"),
              tm(fit=True, size=-2.0, dist=0.25, k=0.5)):
        add(cases, "reversed_tumour", [t], J.reversed_tumour)

    # ---- join_shapes
    add(cases, "join_shapes", [[LINE1, LINE2], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[LINE1, LINE_FAR], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[LINE2, LINE1], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[LINE1, LINE2, POLY1], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[ARC1, LINE1], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[FREE1, LINE2], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [["notashape", LINE1, LINE2], 1.0, 1e-9],
        lambda s, k, tol: joined([x for x in s if isinstance(x, dict)], k, tol))
    a = sh("line", [[0.0, 60.0], [4.0, 60.0]], tumour=TOUCH_TM)
    b = sh("line", [[4.0, 60.0], [8.0, 60.0]])
    add(cases, "join_shapes", [[a, b], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    a = sh("line", [[0.0, 60.0], [4.0, 60.0]], tumour=tm(size=2.0, side="left", k=1.0))
    b = sh("line", [[8.0, 60.0], [4.0, 60.0]], tumour=tm(size=2.0, side="left", k=1.0))
    add(cases, "join_shapes", [[a, b], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))
    # a far end that only counts as touching with a loose tolerance
    add(cases, "join_shapes", [[LINE1, sh("line", [[4.5, 60.0], [8.0, 60.0]])], 1.0, 1.0],
        lambda s, k, tol: joined(s, k, tol))
    add(cases, "join_shapes", [[LINE1], 1.0, 1e-9], lambda s, k, tol: joined(s, k, tol))

    # joined curve dicts to feed the read-side functions
    JOINED_GAP = joined([LINE1, LINE_FAR])
    JOINED_TOUCH = joined([LINE1, LINE2])
    JOINED_TM = joined([sh("line", [[0.0, 60.0], [4.0, 60.0]], tumour=tm(size=2.0, k=1.0)),
                        sh("line", [[6.0, 60.0], [10.0, 60.0]], tumour=tm(size=3.0, side="left", k=1.0))])
    JOINED_THREE = joined([LINE1, LINE_FAR, POLY1])
    # a joined curve whose pieces kept the same tumours: one setting for the whole curve
    JOINED_SAME_TM = joined([sh("line", [[0.0, 60.0], [4.0, 60.0]], tumour=tm(size=2.0, k=1.0)),
                             sh("line", [[6.0, 60.0], [10.0, 60.0]], tumour=tm(size=2.0, k=1.0))])
    # an explicit joined curve with a section split inside a piece
    SPLIT_JOINED = sh(
        "curve",
        [[0.0, 60.0], [1.0, 60.0], [2.0, 60.0], [3.0, 61.0], [4.0, 62.0], [5.0, 63.0],
         [6.0, 64.0], [7.0, 65.0], [8.0, 66.0], [9.0, 66.0]],
        tool=None)
    SPLIT_JOINED.pop("tool", None)
    SPLIT_JOINED["gaps"] = [2]
    SPLIT_JOINED["splits"] = [1]
    SPLIT_JOINED["tumours"] = [tm(size=2.0, k=1.0), tm(size=4.0, side="left", k=1.0),
                               tm(size=6.0, k=1.0)]

    # ---- sources for reverse / channels: joined_paths
    for shape in (JOINED_GAP, JOINED_TOUCH, JOINED_TM, JOINED_THREE, JOINED_SAME_TM):
        add(cases, "joined_paths", [shape],
            lambda s: [[[p[0], p[1]] for p in path] for path in J.joined_paths(s, T.tumour_path)])

    # ---- is_joined / all_tumours / shown_tumour / unify_tumours
    for shape in (JOINED_GAP, JOINED_TOUCH, JOINED_TM, JOINED_SAME_TM,
                  sh("line", [[0.0, 60.0], [1.0, 60.0]]), CURVE1,
                  sh("curve", CURVE1["pts"])):
        add(cases, "is_joined", [shape], J.is_joined)
    for shape in (JOINED_TM, JOINED_SAME_TM, sh("line", [[0.0, 60.0], [1.0, 60.0]],
                                                tumour=tm(size=2.0))):
        add(cases, "all_tumours", [shape], lambda s: J.all_tumours(s))
        add(cases, "shown_tumour", [shape], lambda s: J.shown_tumour(s))
    for shape in (JOINED_TM, JOINED_SAME_TM):
        add(cases, "unify_tumours", [shape],
            lambda s: (lambda c: (J.unify_tumours(c), {
                "tumour": c.get("tumour"), "tumours": c.get("tumours"),
                "splits": c.get("splits"),
            })[1])(copy.deepcopy(s)))

    # ---- sections / split_pieces / split_at
    for shape in (JOINED_GAP, JOINED_TOUCH, JOINED_TM, JOINED_THREE, JOINED_SAME_TM,
                  SPLIT_JOINED):
        add(cases, "sections", [shape],
            lambda s: [[a, b, tmc] for a, b, tmc in J.sections(s)])
        add(cases, "split_pieces", [shape], lambda s: J.split_pieces(s))
    for shape, a in ((SPLIT_JOINED, 1), (SPLIT_JOINED, 2), (SPLIT_JOINED, 3),
                     (JOINED_GAP, 1), (JOINED_TOUCH, 1), (JOINED_GAP, 0),
                     (JOINED_GAP, 2), (JOINED_TM, 1)):
        add(cases, "split_at", [shape, a],
            lambda s, aa: (list(J.split_at(s, aa)) if J.split_at(s, aa) else None))

    # ---- clean_joined / is_joined from a file
    base = sh("curve", JOINED_GAP["pts"], sharp=list(JOINED_GAP.get("sharp", [])))
    for extra in (
        {"gaps": [1], "splits": [2], "tumours": [None, None]},
        {"gaps": [2], "splits": [1], "tumours": [tm(size=2.0), tm()]},
        {"gaps": [1, 3, 1, 999, -1], "splits": [0, 2, 2, 99], "tumours": [None, tm(), None, None]},
        {"gaps": "12", "splits": "1", "tumours": [None, tm(), None]},
        {"gaps": None, "splits": [1], "tumours": [None, tm()]},
        {"gaps": 3, "splits": [1], "tumours": [None, tm()]},
        {"gaps": [1], "splits": ["x"], "tumours": [None, tm()]},
        {"gaps": [1], "splits": [2], "tumours": [tm()]},
        {"gaps": [1], "splits": [2]},
    ):
        def clean_call(shp, ex):
            out = copy.deepcopy(shp)
            J.clean_joined(ex, out)
            return {
                "pts": [[p[0], p[1]] for p in out["pts"]],
                "sharp": list(out.get("sharp", [])),
                "gaps": list(out.get("gaps", [])),
                "splits": list(out.get("splits", [])),
                "tumour": out.get("tumour"),
                "tumours": out.get("tumours"),
            }
        add(cases, "clean_joined", [base, extra], clean_call)

    # ---- velocities
    old = sh("curve", LINE1["pts"], vel0=40.0, vel1=100.0)
    new_pts = [[0.0, 60.0], [1.0, 60.0], [2.0, 60.0], [3.0, 60.0]]
    add(cases, "piece_velocity", [sh("curve", new_pts), old, [0.0, 2.0], [0.0, 4.0]],
        lambda n, o, ns, os_: (J.piece_velocity(n, o, tuple(ns), tuple(os_)), n)[1])
    env_old = sh("curve", LINE1["pts"], vel0=10.0, vel1=90.0,
                 vel_env=[[0.0, 10.0], [0.5, 90.0], [1.0, 30.0]])
    add(cases, "piece_velocity", [sh("curve", new_pts), env_old, [1.0, 3.0], [0.0, 4.0]],
        lambda n, o, ns, os_: (J.piece_velocity(n, o, tuple(ns), tuple(os_)), n)[1])
    add(cases, "piece_velocity", [sh("curve", new_pts), old, [2.0, 2.0], [0.0, 4.0]],
        lambda n, o, ns, os_: (J.piece_velocity(n, o, tuple(ns), tuple(os_)), n)[1])
    add(cases, "join_velocity",
        [[sh("line", [[0.0, 60.0], [2.0, 60.0]], vel0=30.0, vel1=70.0),
          sh("line", [[3.0, 60.0], [5.0, 60.0]], vel0=80.0, vel1=120.0)],
         [[0.0, 2.0], [3.0, 5.0]], [0.0, 5.0]],
        lambda olds, spans, ns: (lambda n: (J.join_velocity(n, copy.deepcopy(olds), [tuple(s) for s in spans],
                                                           tuple(ns)), n)[1])({}))
    add(cases, "join_velocity",
        [[sh("line", [[0.0, 60.0], [2.0, 60.0]], vel0=10.0, vel1=10.0,
             vel_env=[[0.0, 10.0], [0.5, 100.0], [1.0, 50.0]]),
          sh("line", [[2.0, 60.0], [4.0, 60.0]], vel0=60.0, vel1=60.0)],
         [[0.0, 2.0], [2.0, 4.0]], [0.0, 4.0]],
        lambda olds, spans, ns: (lambda n: (J.join_velocity(n, copy.deepcopy(olds), [tuple(s) for s in spans],
                                                           tuple(ns)), n)[1])({}))
    add(cases, "join_velocity",
        [[sh("line", [[0.0, 60.0], [2.0, 60.0]], vel0=10.0, vel1=10.0),
          sh("line", [[5.0, 60.0], [7.0, 60.0]], vel0=50.0, vel1=90.0)],
         [[0.0, 2.0], [5.0, 7.0]], [0.0, 7.0]],
        lambda olds, spans, ns: (lambda n: (J.join_velocity(n, copy.deepcopy(olds), [tuple(s) for s in spans],
                                                           tuple(ns)), n)[1])({}))

    # ---- custom_groups / split_custom
    custom_joined = sh(
        "custom",
        [[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]],
        strokes=[
            {"kind": "poly", "pts": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]},
            {"kind": "poly", "pts": [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0]]},
            {"kind": "poly", "pts": [[5.0, 5.0], [6.0, 5.0], [6.0, 6.0]]},
        ],
        fill="empty", gate=60.0, align="auto", name="x")
    custom_apart = sh(
        "custom",
        [[0.0, 60.0], [2.0, 60.0], [0.0, 62.0]],
        strokes=[
            {"kind": "poly", "pts": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]},
            {"kind": "poly", "pts": [[1.0, 1.0], [2.0, 1.0]]},
            {"kind": "poly", "pts": [[3.0, 3.0], [4.0, 3.0]]},
        ],
        fill="empty", gate=60.0, align="auto", name="y")
    for shape in (custom_joined, custom_apart):
        add(cases, "custom_groups", [shape], J.custom_groups)
        add(cases, "split_custom", [shape],
            lambda s: [{"pts": c["pts"], "strokes": c["strokes"]} for c in J.split_custom(copy.deepcopy(s))])

    return {"module": "joined", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("joined", data["cases"])
