"""convert.py differential vectors (1.2.0).

Runs the original Python module (notes/convert.py) directly and writes the cases to
crates/spiderweb-core/tests/vectors/convert.json; the Rust test compares case by case.

Regenerate against 1.2.0:
    SPIDERWEB_SRC=/path/to/Spiderweb-1.2.0/scripts python3 tools/gen_convert_vectors.py
"""

import copy
import math

import numpy as np

from vec_common import write

from notes import convert as CV
from notes import custom as C
from notes import engine as E


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


def shape(kind, pts, **kw):
    """A raw shape dict -> clean_shape'd shape (like the shape of a project)."""
    d = {
        "kind": kind,
        "pts": [[float(b), float(p)] for b, p in pts],
        "vel0": kw.pop("vel0", 127.0),
        "vel1": kw.pop("vel1", 80.0),
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


def custom(strokes, pts=None, **kw):
    if pts is None:
        pts = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    kw.setdefault("name", "seed")
    return shape("custom", pts, strokes=copy.deepcopy(strokes), **kw)


def tumour(**kw):
    d = {"on": True, "shape": "triangle", "size": 3.0, "length": 0.125, "dist": 0.125, "side": "alt",
         "wrap": "simple", "start": 0.0, "end": 1.0, "ease": 0.0, "fit": False, "seed": 1, "mirror": False,
         "k": 0.25}
    d.update(kw)
    return d


def src(s, i):
    return dict(s, src=i)


SQUARE = [stroke("poly", [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]])]
TRIANGLE = [stroke("poly", [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0], [0.0, 0.0]])]
CURVE_ST = [curve([[0.0, 0.0], [0.3, 0.0], [0.7, 1.0], [1.0, 1.0]], sharp=[1])]
ARC_ST = [stroke("arc", [[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]], k=1.0)]
FREE_ST = [stroke("poly", [[0.0, 0.0], [0.2, 0.03], [0.35, -0.02], [0.5, 0.04], [0.65, -0.03],
                           [0.8, 0.02], [1.0, 0.0]], free=True, smooth=50, k=1.0)]


def gen():
    cases = []

    def add(fn, args, out):
        cases.append({"fn": fn, "args": tolist(args), "out": tolist(out)})

    def add_line_strokes(sh):
        add("line_strokes", [sh, E.cached_strokes(sh)], CV.line_strokes(sh, E.cached_strokes(sh)))

    # ------------------------------------------------------------ line_strokes
    add_line_strokes(shape("line", [[0.0, 60.0], [2.0, 64.0]]))
    add_line_strokes(shape("poly", [[0.0, 60.0], [1.0, 62.0], [2.0, 60.0]]))
    add_line_strokes(shape("free", [[0.0, 60.0], [0.3, 61.0], [0.6, 59.5], [1.0, 60.0]], smooth=40, k=1.0))
    add_line_strokes(shape("arc", [[0.0, 60.0], [1.0, 62.0], [2.0, 60.0]], k=1.0))
    add_line_strokes(shape("curve", [[0.0, 60.0], [0.5, 62.0], [1.5, 58.0], [2.0, 60.0]]))
    add_line_strokes(shape("curve", [[0.0, 60.0], [0.5, 62.0], [1.5, 58.0], [2.0, 60.0]], sharp=[1],
                            sym="mirror"))
    # Tumours: the bumps become points (only `paths` has the bumps).
    add_line_strokes(shape("line", [[0.0, 60.0], [2.0, 64.0]], tumour=tumour(size=2.0)))
    add_line_strokes(shape("poly", [[0.0, 60.0], [1.0, 62.0], [2.0, 60.0]], tumour=tumour(on=False)))
    # A joined curve (gaps): one curve stroke per piece.
    joined_pts = [[float(i), 60.0 + math.sin(i) * 2.0] for i in range(13)]
    joined = shape("curve", joined_pts)
    joined["gaps"] = [1]
    joined["sharp"] = [2]
    joined = E.clean_shape({k: v for k, v in joined.items()})
    assert joined is not None and joined.get("gaps") == [1], joined
    add_line_strokes(joined)

    # ------------------------------------------------------------ losses
    add("losses", [[shape("line", [[0.0, 60.0], [2.0, 64.0]])]], CV.losses([shape("line", [[0.0, 60.0], [2.0, 64.0]])]))
    add("losses", [[shape("line", [[0.0, 60.0], [2.0, 64.0]], tumour=tumour())]],
        CV.losses([shape("line", [[0.0, 60.0], [2.0, 64.0]], tumour=tumour())]))
    add("losses", [[shape("poly", [[0.0, 60.0], [2.0, 64.0]], end_dot=True)]],
        CV.losses([shape("poly", [[0.0, 60.0], [2.0, 64.0]], end_dot=True)]))
    both = shape("line", [[0.0, 60.0], [2.0, 64.0]], tumour=tumour(), end_dot=True)
    add("losses", [[both, shape("custom", [[0.0, 60.0], [2.0, 60.0], [0.0, 64.0]],
                                strokes=copy.deepcopy(SQUARE))]],
        CV.losses([both, shape("custom", [[0.0, 60.0], [2.0, 60.0], [0.0, 64.0]], strokes=copy.deepcopy(SQUARE))]))
    # A custom shape's tumour / last note doesn't count (only line kinds are in LINE_KINDS).
    other = shape("custom", [[0.0, 60.0], [2.0, 60.0], [0.0, 64.0]], strokes=copy.deepcopy(SQUARE),
                  end_dot=True)
    add("losses", [[other]], CV.losses([other]))

    # ------------------------------------------------------------ to_live
    DEFAULTS = {"kind": "line", "pts": [[0.0, 60.0]], "vel0": 100.0, "vel1": 90.0, "end_dot": False}
    CD = dict(C.CUSTOM_DEFAULTS)

    def add_to_live(shapes, cd=None, defaults=None):
        paths = [E.cached_strokes(sh) for sh in shapes]
        out = CV.to_live(shapes, paths, defaults or DEFAULTS, cd or CD)
        add("to_live", [shapes, paths, defaults or DEFAULTS, cd or CD], out)
        return out

    mixed = [
        shape("line", [[0.0, 60.0], [1.0, 62.0]]),
        shape("poly", [[1.0, 62.0], [2.0, 60.0], [3.0, 62.0]]),
        shape("free", [[3.0, 62.0], [3.5, 61.0], [4.0, 62.0]], smooth=30, k=1.0),
        shape("curve", [[4.0, 62.0], [4.5, 64.0], [5.5, 60.0], [6.0, 62.0]]),
        shape("arc", [[6.0, 62.0], [7.0, 64.0], [8.0, 62.0]], k=1.0),
    ]
    add_to_live(mixed)
    # Velocity envelopes: every joined part keeps its own velocity.
    with_env = [
        shape("line", [[0.0, 60.0], [1.0, 62.0]], vel0=30.0, vel1=60.0),
        shape("poly", [[1.0, 62.0], [2.0, 61.0], [3.0, 62.0]], vel0=60.0, vel1=127.0),
        shape("arc", [[3.0, 62.0], [4.0, 64.0], [5.0, 62.0]], k=1.0,
              vel_env=[[0.0, 50.0], [0.4, 110.0], [1.0, 70.0]]),
    ]
    add_to_live(with_env)
    # The first custom shape gives the name and fill settings; a later custom shape's src groups stay apart.
    first_custom = custom(copy.deepcopy(SQUARE), pts=[[2.0, 60.0], [4.0, 60.0], [2.0, 64.0]], name="box",
                          fill="spam", gate=0.125, align="centred", ends="keep", union=True, apart=True)
    second_custom = custom([src(stroke("poly", [[0.0, 0.0], [1.0, 1.0]]), 0),
                            src(stroke("poly", [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]), 1)],
                           pts=[[4.0, 60.0], [6.0, 60.0], [4.0, 62.0]])
    add_to_live([
        shape("line", [[0.0, 60.0], [2.0, 60.0]]),
        second_custom,
        first_custom,
    ])
    # Turning an already converted shape again: every source group stays apart (the ids mapping).
    converted = CV.to_live(mixed, [E.cached_strokes(sh) for sh in mixed], DEFAULTS, CD)
    add_to_live([
        shape("line", [[0.0, 56.0], [1.0, 58.0]]),
        converted,
    ])
    # Custom defaults (fill / gate / align / ends / union / apart).
    add_to_live([shape("line", [[0.0, 60.0], [2.0, 62.0]])], cd={
        "fill": "spam", "gate": 0.25, "align": "aligned", "ends": "stretch", "union": True, "apart": True})
    # Only custom shapes (name / settings come from the first one).
    add_to_live([copy.deepcopy(first_custom)])

    # ------------------------------------------------------------ originals
    def add_originals(sh):
        add("originals", [sh], CV.originals(sh))

    conv = CV.to_live(mixed, [E.cached_strokes(sh) for sh in mixed], DEFAULTS, CD)
    add_originals(conv)
    moved = copy.deepcopy(conv)
    moved["pts"] = [[b + 2.5, p - 1.0] for b, p in moved["pts"]]
    add_originals(moved)
    resized = copy.deepcopy(conv)
    resized["pts"][1] = [resized["pts"][1][0] + 0.5, resized["pts"][1][1]]
    add_originals(resized)
    turned = copy.deepcopy(conv)
    turned["pts"] = [[0.0, 60.0], [0.0, 62.0], [2.0, 60.0]]
    add_originals(turned)
    edited = copy.deepcopy(conv)
    edited["strokes"][0]["pts"][0][0] += 0.25
    add_originals(edited)
    # A custom shape without `from`.
    add_originals(custom(copy.deepcopy(SQUARE)))

    return cases


if __name__ == "__main__":
    write("convert", gen())
