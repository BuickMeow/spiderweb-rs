"""Differential vectors for spiderweb-io: shape JSON / project files / MIDI / math expressions.

The Python originals (files/ and notes/engine.clean_shape etc.) are run directly and the output
goes to crates/spiderweb-io/tests/vectors/<module>.json for the Rust tests to compare case by
case. The MIDI vectors give the full file hex from Python write_midi and require byte-identical
output.
"""

import copy
import json
import math
import os
import random
import sys
import tempfile

import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Upstream source dir: Spiderweb-main (the 1.1.0 reference copy) by default; SPIDERWEB_SRC points at another version (1.2.0)
SCRIPTS = os.environ.get("SPIDERWEB_SRC") or os.path.join(os.path.dirname(REPO), "Spiderweb-main", "scripts")
if not os.path.isdir(SCRIPTS):
    SCRIPTS = "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts"
OUT_DIR = os.path.join(REPO, "crates", "spiderweb-io", "tests", "vectors")
if SCRIPTS not in sys.path:
    sys.path.insert(0, SCRIPTS)

from files.mathexpr import calc, calc_int, fmt, formula  # noqa: E402
from files.midi_out import write_midi  # noqa: E402
from files.project import backup_path, project_json, short_num, short_env, short_shape  # noqa: E402
from files.snap import (DEFAULT_SNAP, DOTS, SNAP_LIST, SNAPS, clean_snap, custom_parts,  # noqa: E402
                        custom_snap, snap_beats, whole_notes)
from notes import custom as C  # noqa: E402
from notes import engine as E  # noqa: E402
from notes import funnel as F  # noqa: E402
from notes import smooth as S  # noqa: E402
from notes import text as T  # noqa: E402
from notes import tumour as TU  # noqa: E402

APP_VERSION = "1.1.0"  # this port's version (Rust VERSION; upstream 1.2.0's about.VERSION is "1.2.0")


def write(module, cases):
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, f"{module}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"module": module, "cases": cases}, f, ensure_ascii=False, allow_nan=True)
    print(f"{module}: {len(cases)} cases -> {path}")


def tolist(x):
    if isinstance(x, dict):
        return {str(k): tolist(v) for k, v in x.items()}
    if isinstance(x, (list, tuple, set)):
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


# ---------------------------------------------------------------- shapes

def shape(kind, pts, **extra):
    d = {"kind": kind, "pts": [[float(b), float(p)] for b, p in pts]}
    d.update(extra)
    return d


def poly_stroke(pts, free=False, smooth=0, k=1.0):
    d = {"kind": "poly", "pts": [[float(u), float(v)] for u, v in pts]}
    if free:
        d.update(free=True, smooth=smooth, k=k)
    return d


def curve_stroke(pts, sharp=None, sym=None):
    d = {"kind": "curve", "pts": [[float(u), float(v)] for u, v in pts]}
    if sharp is not None:
        d["sharp"] = sharp
    if sym is not None:
        d["sym"] = sym
    return d


def compat_shapes():
    cases = []
    cases.append(shape("line", [(0, 60), (4, 72)], vel0=100.5, vel1=20, end_dot=True,
                       vel_env=[[0.0, 127.0], [0.5, 63.25], [1.0, 30.5]]))
    cases.append(shape("line", [(0, 60), (4, 72)], vel_env=[[0.0, 900], [1.0, -5]]))
    cases.append(shape("poly", [(-1, 30), (0.5, 64), (7.25, 100)],
                       tumour={"on": 1, "shape": "square", "size": 5.5, "length": 0.25, "side": "left",
                               "wrap": "wrap", "seed": 7, "mirror": True, "k": 0.5}))
    cases.append(shape("free", [(0, 0), (1, 1), (2, 0.5)], smooth=42.7, k=0.5))
    cases.append(shape("free", [(0, 0), (1, 1), (2, 0.5)]) )
    cases.append(shape("arc", [(0, 60), (2, 70), (4, 62)], k=2.5))
    cases.append(shape("arc", [(0, 60), (2, 70), (4, 62)]))
    cases.append(shape("arc", [(0, 60), (2, 70)]) )  # wrong point count -> None
    cases.append(shape("curve", [(0, 0), (1, 0), (2, 1), (3, 1), (4, 0.5), (5, 0.5), (6, 0.25)],
                       sharp=[1, 2, 2, 5], sym="mirror"))
    cases.append(shape("curve", [(0, 0), (1, 0), (2, 1), (3, 1)]))
    cases.append(shape("curve", [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)]))  # 5 points -> truncated to 4
    cases.append(shape("curve", [(0, 0), (1, 0), (2, 1)], sharp=[1], sym="turn"))
    cases.append(shape("curve", [(0, 0), (1, 0), (2, 1), (3, 1), (4, 0.5), (5, 0.5), (6, 0.25)],
                       sharp=[0, 1, 9], sym="turn"))  # last=2 is even, sharp keeps only 1
    custom = shape("custom", [(0, 0), (1, 0), (0, 1)],
                   strokes=[poly_stroke([(0.1, 0.1), (0.9, 0.1), (0.9, 0.9)], free=True, smooth=33.3, k=1.5),
                            curve_stroke([(0, 0), (0.3, 0.1), (0.6, 0.1), (1, 1), (1.2, 1.1), (1.5, 1.2), (2, 2)],
                                         sharp=[1], sym="mirror"),
                            {"kind": "arc", "pts": [[0.2, 0.3], [0.5, 0.6], [0.8, 0.3]], "k": 0.75},
                            {"kind": "ellipse", "box": [0.25, 0.25, 0.75, 0.85]}],
                   fill="spam", gate=0.125, align="aligned", name="中文名字")
    cases.append(custom)
    cases.append(shape("custom", [(0, 0), (1, 0), (0, 1)],
                       strokes=[{"kind": "ellipse", "box": [0, 0, 1, 1]}], fill="outline_spam",
                       gate=0.0625, align="auto", text=T.TEXT_DEFAULTS | {
                           "text": "Hi", "bbox": [-0.1, -0.2, 0.5, 0.75], "cap": 0.72, "k": 1.5,
                           "holes": {2, 0, 2}}))
    notes = [[0, 120, 60, 100, 0], [240, 360, 62, 90, 1], [500, 700, 59, 80, 0]]
    cases.append(E.clean_shape({"kind": "custom", "pts": [[0, 0], [1, 0], [0, 1]],
                                "strokes": [{"kind": "ellipse", "box": [0, 0, 1, 1]}], "fill": "empty",
                                "align": "auto", "gate": 0.0625,
                                "notes": C.pack_notes(notes), "own_vel": True}))
    cases.append(shape("custom", [(0, 0), (1, 0)], strokes=[poly_stroke([(0, 0), (1, 1)])]))
    cases.append(shape("custom", [(0, 0), (1, 0), (0, 1)], strokes=[], fill="empty"))
    cases.append(shape("custom", [(0, 0), (1, 0), (0, 1)],
                       strokes=[{"kind": "poly", "pts": [[0, 0], [1, 1]], "free": 1, "smooth": -4, "k": 999}],
                       fill="nope", gate=2.5, align="nope", name=42))
    cases.append(shape("funnel", [(0, 60), (4, 60), (4, 72), (4, 52)],
                       starts=[{"line": 0, "at": 0.25,
                                "ends": [{"pts": [[0, 0], [0.7, 0.06], [0.94, 0.3], [1, 1]], "sharp": []},
                                         {"pts": [[0, 0], [1, 0], [2, 1], [3, 1]], "sharp": [1], "link": 3,
                                          "flip": True}]}],
                       fill="long", gate0=0.5, gate1=0.125, change="smooth", follow="curve", wall="past"))
    cases.append(shape("funnel", [(0, 60), (4, 60), (4, 72), (4, 52)],
                       starts=[{"line": 1, "at": 2.0, "ends": [None, None]},
                               {"line": 0, "at": 0.5, "ends": [None, None]}],
                       fill="spam", vary=False))
    cases.append(shape("funnel", [(0, 60), (4, 60), (4, 72), (4, 52)]))  # no starts
    cases.append(shape("funnel", [(0, 60), (4, 60), (4, 72)]))  # point count is not even -> None
    cases.append(shape("blob", [(0, 0), (1, 1)]))
    cases.append(shape("line", [(0, 0), (1, 1)], vel0="abc"))
    cases.append({"kind": "line", "pts": [[0, 0], [1]]})
    cases.append({"kind": "line"})
    cases.append(shape("custom", [(0, 0), (1, 0), (0, 1)], strokes=[poly_stroke([(0, 0), (1, 1)])],
                       gate="bad"))
    cases.append(shape("custom", [(0, 0), (1, 0), (0, 1)],
                       strokes=[{"kind": "poly", "pts": [[0, 0], [1, 1]]}], notes="not packed"))
    cases.append(shape("line", [(0, 0), (1, 1)], vel_env=[[0, 60]]))
    cases.append(shape("line", [(0, 0), (1, 1)], vel_env=[[0, 60], [1, 70, 80]]))

    out = []
    for i, sh in enumerate(cases):
        try:
            cleaned = E.clean_shape(sh)
            case = {"name": f"shape{i}", "input": tolist(sh), "clean": tolist(cleaned)}
            if cleaned is not None:
                try:  # Python does not convert vel0/vel1 and leaves bad values as they are; Rust's Shape uses f64, so it can only error
                    float(cleaned["vel0"]), float(cleaned["vel1"])
                except (TypeError, ValueError):
                    case["non_numeric_vel"] = True
            out.append(case)
        except Exception as e:  # noqa: BLE001 - in Python such bad fields make load_file fail
            out.append({"name": f"shape{i}", "input": tolist(sh), "error": type(e).__name__})
    return out


# ---------------------------------------------------------------- project

def expected_project(data):
    """The data project_data writes after load_file (without the UI parts)."""
    shapes = [s for s in (E.clean_shape(sh) for sh in data.get("shapes", [])) if s]
    defaults = dict(E.SHAPE_DEFAULTS)
    defaults.update({k: type(E.SHAPE_DEFAULTS[k])(v)
                     for k, v in data.get("defaults", {}).items() if k in E.SHAPE_DEFAULTS})
    mode = data.get("channel_mode", "auto" if data.get("auto_channels") else "single")
    mode = mode if mode in E.CHANNEL_MODES else "single"
    split = data.get("channel_split")
    split = split if split in E.SPLITS else "key"
    # 1.2.0 snap: load_file runs clean_snap (old values migrate, bad ones fall back)
    snap = clean_snap(data["snap"]) if isinstance(data.get("snap"), str) else DEFAULT_SNAP
    # 1.2.0's 256-key setting (project_data writes keys; on load anything other than 256 is 128)
    keys = 256 if data.get("keys") == 256 else 128
    # 1.2.0 domino_start: only a known value moves the dropdown, else the initial "note"
    domino_start = data["domino_start"] if data.get("domino_start") in ("note", "bar") else "note"

    custom_defaults = dict(C.CUSTOM_DEFAULTS)
    custom_shape = "Circle"
    custom = data.get("custom_defaults") or {}
    if isinstance(custom, dict):
        if custom.get("fill") in C.FILLS:
            custom_defaults["fill"] = custom["fill"]
        if custom.get("align") in C.ALIGNS:
            custom_defaults["align"] = custom["align"]
        try:
            custom_defaults["gate"] = max(1e-6, float(custom.get("gate", C.CUSTOM_DEFAULTS["gate"])))
        except (TypeError, ValueError):
            pass
        if custom.get("shape"):
            custom_shape = str(custom["shape"])

    funnel_defaults = dict(F.FUNNEL_DEFAULTS)
    funnel = data.get("funnel_defaults")
    if isinstance(funnel, dict):
        try:
            funnel_defaults = {k: v for k, v in F.clean_funnel(funnel).items() if k in F.FUNNEL_DEFAULTS}
        except (TypeError, ValueError):
            pass

    text_defaults = dict(T.TEXT_DEFAULTS)
    if isinstance(data.get("text_defaults"), dict):
        tx = T.clean_text(dict(data["text_defaults"], bbox=[0, 0, 1, 1]))
        if tx:
            text_defaults = {k: tx[k] for k in T.TEXT_DEFAULTS}

    view = None
    try:
        v = data.get("view")
        view = {k: float(v[k]) for k in ("t", "top", "sx", "sy")}
    except (TypeError, KeyError, ValueError):
        view = None
    try:
        playhead = max(0.0, float(data.get("playhead", 0)))
    except (TypeError, ValueError):
        playhead = 0.0

    return {
        "version": 2,
        "app_version": APP_VERSION,
        "ppq": str(data["ppq"]) if "ppq" in data else "960",
        "bpm": str(data["bpm"]) if "bpm" in data else "120",
        "beats": str(data["beats"]) if "beats" in data else "4",
        "output": str(data["output"]) if "output" in data else "",
        "channel_mode": mode,
        "channel_split": split,
        "keys": keys,
        "domino_start": domino_start,
        "snap": snap,
        "defaults": defaults,
        "custom_defaults": dict(custom_defaults, shape=custom_shape),
        "funnel_defaults": funnel_defaults,
        "text_defaults": text_defaults,
        "free_smooth": S.clean_level(data.get("free_smooth", S.SMOOTH_DEFAULT)),
        "shapes": tolist(shapes),
        "view": view,
        "playhead": playhead,
    }


def project_cases():
    custom_notes = [[0, 120, 60, 100, 0], [240, 360, 62, 90, 1]]
    text_shape = E.clean_shape(shape("custom", [(0, 0), (1, 0), (0, 1)],
                                     strokes=[{"kind": "ellipse", "box": [0, 0, 1, 1]}], fill="empty",
                                     align="auto", gate=0.0625, name="Text",
                                     text=T.TEXT_DEFAULTS | {"text": "Hello", "bbox": [-0.1, -0.2, 0.5, 0.75],
                                                             "cap": 0.72, "k": 1.5, "holes": [1]}))
    packed = E.clean_shape({"kind": "custom", "pts": [[0, 0], [1, 0], [0, 1]],
                            "strokes": [{"kind": "ellipse", "box": [0, 0, 1, 1]}], "fill": "empty",
                            "align": "auto", "gate": 0.0625, "notes": C.pack_notes(custom_notes),
                            "own_vel": True})
    base_shapes = [
        E.clean_shape(shape("line", [(0, 60), (4, 72)], vel0=100.5, vel1=20, end_dot=True,
                            vel_env=[[0.0, 127.0], [0.1000000000000001, 63.25], [0.1, 30.5]])),
        E.clean_shape(shape("curve", [(0, 0), (1, 0), (2, 1), (3, 1)], sharp=[1], sym="mirror")),
        text_shape,
        packed,
        E.clean_shape(shape("funnel", [(0, 60), (4, 60), (4, 72), (4, 52)],
                            starts=[{"line": 0, "at": 0.25,
                                     "ends": [{"pts": [[0, 0], [0.7, 0.06], [0.94, 0.3], [1, 1]], "sharp": []},
                                              None]}],
                            fill="long", gate0=0.5, gate1=0.125, change="smooth", follow="curve", wall="past")),
        E.clean_shape(shape("free", [(0, 0), (1, 1), (2, 0.5)], smooth=42.7, k=0.5)),
    ]

    projects = []
    d = {
        "version": 2, "app_version": "0.9.0",
        "ppq": "960", "bpm": "120", "beats": "4", "output": "out/spiderweb.mid",
        "channel_mode": "auto", "channel_split": "time", "keys": 256, "domino_start": "bar",
        "snap": "1/32",
        "defaults": {"vel0": 100.5, "vel1": 30, "end_dot": True},
        "custom_defaults": {"fill": "spam", "gate": 0.3333333333333, "align": "aligned", "shape": "Circle"},
        "funnel_defaults": {"fill": "long", "gate0": 0.5, "gate1": 0.125, "change": "smooth",
                            "follow": "curve", "wall": "past"},
        "text_defaults": {"font": "Arial", "size": 48.0, "unit": "rows", "weight": 700, "italic": True,
                          "tracking": 12.5, "leading": 90.0, "align": "center", "threshold": 33.3, "grow": 2.5},
        "free_smooth": 55.5,
        "shapes": base_shapes,
        "view": {"t": 0.0, "top": 127.5, "sx": 50.0, "sy": 10.0},
        "playhead": 3.5,
    }
    projects.append(("full", d))

    d2 = dict(d)
    # a 1.1.0 project: no keys and no domino_start
    d2.pop("defaults"), d2.pop("view"), d2.pop("keys"), d2.pop("domino_start")
    d2["auto_channels"] = True
    d2.pop("channel_mode")
    d2["snap"] = "nope"
    d2["channel_split"] = "nope"
    d2["output"] = ""
    projects.append(("old", d2))

    projects.append(("empty", {}))
    d3 = dict(d)
    d3["shapes"] = []
    d3["defaults"] = {"vel0": 1, "unknown": 5}
    d3["custom_defaults"] = None
    d3["funnel_defaults"] = {"gate0": "bad", "gate1": 2}
    d3["text_defaults"] = None
    d3["free_smooth"] = "50"
    d3["playhead"] = -5
    d3["view"] = {"t": 1}
    d3["keys"] = "256"  # a string does not count: it stays 128
    projects.append(("partial", d3))

    # an old 1.1.0 project: snap 1/64 / 1/128 / Off, and no domino_start (-> note)
    d4 = dict(d)
    d4["snap"] = "1/64"
    d4["domino_start"] = "nope"
    projects.append(("legacy", d4))

    cases = []
    for name, data in projects:
        try:
            saved = project_json(data)
            loaded = json.loads(saved)
            expected = expected_project(loaded)
            cases.append({"name": name, "saved": saved, "expected": tolist(expected)})
        except Exception as e:  # noqa: BLE001
            cases.append({"name": name, "saved": project_json(data), "error": type(e).__name__})
    return cases


# ---------------------------------------------------------------- short_shape

def short_cases():
    """files/project.py short_shape: the number rounding the project writer does (tumour graphs too)."""
    noisy = 0.1000000000000001
    tm = {"on": 1, "shape": "parabola", "size": 3.2999999999999998, "length": 0.12500000000000003,
          "dist": 0.1000000000000001, "side": "right", "wrap": "bent", "start": 0.0, "end": 1.0,
          "ease": 0.30000000000000004, "rot": -0.12500000000000003, "slant": 0.20000000000000004,
          "graphs": {"size": [[0.0, noisy], [0.5, 2.5000000000000004], [1.0, 1]],
                     "dist": [[0.0, 0.20000000000000004], [1.0, 0.30000000000000004]]},
          "fit": 0, "seed": 7, "mirror": 1, "k": 0.24999999999999997}
    cases = [
        ("tumour", {"kind": "poly", "pts": [[0.0, 60.0], [4.0, 62.5]], "tumour": tm}),
        ("tumour_no_graphs", {"kind": "line", "pts": [[0.0, 60.0], [1.0, 60.0]],
                              "tumour": {"on": True, "size": noisy, "seed": 3}}),
        ("joined_tumours", {"kind": "curve", "pts": [[0.0, 0.0], [1.0, 0.0], [2.0, 1.0], [3.0, 1.0]],
                            "tumours": [tm, None, {"on": True, "size": 2.9999999999999996}],
                            "splits": [1]}),
        ("from_passthrough", {"kind": "custom", "pts": [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                              "strokes": [{"kind": "poly", "pts": [[0.1, 0.1], [0.9, 0.1]]}],
                              "from": {"shapes": [{"kind": "line", "pts": [[0.1, 0.1], [0.9, 0.1]]}],
                                       "strokes": [{"kind": "poly", "pts": [[0.1, 0.1], [0.9, 0.1]]}],
                                       "pts": [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]}}),
        ("strokes_starts_text", {"kind": "funnel", "pts": [[0.0, 60.0], [4.0, 60.0], [4.0, 72.0], [4.0, 52.0]],
                                 "starts": [{"line": 0, "at": 0.1000000000000001,
                                             "ends": [{"pts": [[0.0, noisy], [1.0, 0.0]], "sharp": [0]}, None]}],
                                 "text": {"bbox": [noisy, 0.0, 1.0, 1.0], "size": 23.999999999999996},
                                 "strokes": [{"kind": "ellipse", "box": [noisy, 0.0, 1.0, 1.0]}],
                                 "gate": noisy, "gate0": 0.06249999999999999, "k": 1.0000000000000002}),
    ]
    return [{"name": name, "input": tolist(sh), "saved": short_shape(tolist(sh))} for name, sh in cases]


# ---------------------------------------------------------------- snap

def snap_cases():
    """files/snap.py: parsing / spelling / lengths / steps / old-value migration."""
    samples = [
        "off", "bar", "1/1", "3/4", "1/3", "1/12", "1/16", "1/48",
        "c:", "c:3/16/1", "c:/64/1", "c:./8/3", "c:../8/3", "c:5/16/1", "c:007/16/1",
        "c: 5 /16/1", "c:1_0/16/1", "c:3/16/1/2", "c:3/16", "c:2/16/1", "c:101/16/1",
        "c:0/16/1", "c:-1/16/1", "c:abc/16/1", "c:3.0/16/1", "c:3/0/1", "c:3/129/1",
        "c:3/16/0", "c:3/16/101", "c:/16/1", "c://1", "c:3/1_6/1", "c:3/16/1 ",
        "c:./64/1", "1/64", "Off", "1/128", "nope", "",
    ]
    cases = []
    for snap in samples:
        parts = custom_parts(snap)
        w = whole_notes(snap)
        cases.append({
            "fn": "snap",
            "snap": snap,
            "parts": None if parts is None else list(parts),
            "whole": None if w is None else {"num": w.numerator, "den": w.denominator},
            "beats": snap_beats(snap, 4),
            "clean": clean_snap(snap),
        })
    for count, note, div in [("", 16, 1), (".", 8, 3), ("..", 4, 2), ("5", 16, 1), ("100", 1, 1)]:
        cases.append({"fn": "custom_snap", "count": count, "note": note, "div": div,
                      "snap": custom_snap(count, note, div)})
    for snap, beats in [("bar", 4), ("bar", 7), ("1/16", 4), ("1/3", 4), ("c:./8/3", 4), ("off", 4)]:
        cases.append({"fn": "snap_beats", "snap": snap, "beats_per_bar": beats,
                      "beats": snap_beats(snap, beats)})
    cases.append({
        "fn": "list",
        "snaps": list(SNAPS),
        "pictures": [None if what is None else list(what) for _, what in SNAP_LIST],
        "default": DEFAULT_SNAP,
        "dots": list(DOTS),
    })
    return cases


# ---------------------------------------------------------------- MIDI

def midi_cases():
    cases = []
    midi_data = [
        ("simple", 960, 120.0, 4,
         [[0, 480, 60, 100, 0, 0], [240, 720, 64, 90, 1, 0], [480, 960, 67, 80, 0, 0]]),
        ("order", 960, 120.0, 4,
         [[0, 480, 60, 100, 0, 0], [0, 480, 62, 90, 0, 0], [480, 960, 64, 80, 0, 0]]),
        ("vlq", 960, 100.0, 3,
         [[0, 1, 0, 1, 0, 0], [127, 128, 127, 64, 0, 0], [16383, 16384, 60, 127, 0, 0],
          [2097151, 2097152, 61, 1, 0, 0], [268435455, 268435456, 62, 2, 0, 0]]),
        ("empty", 960, 120.0, 4, []),
        ("sparse", 480, 60.0, 5, [[0, 240, 48, 77, 3, 0]]),
        ("slots", 960, 120.0, 4,
         [[0, 100, 40, 10, 0, 0], [10, 110, 41, 20, 14, 1], [20, 120, 42, 30, 29, 2]]),
        ("weird", 3840, 95.5, 7, [[0, 1, 0, 0, 0, 0], [7, 8, 127, 127, 0, 0]]),
        # 256 keys: a key >127 is written into the key byte as-is (1.2.0 write_midi does not check; 200 == 0xC8)
        ("high_keys", 960, 120.0, 4, [[0, 480, 200, 100, 0, 0], [0, 480, 128, 90, 0, 0]]),
    ]
    for name, ppq, bpm, beats, notes in midi_data:
        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            path = f.name
        try:
            write_midi(path, ppq, bpm, beats, np.array(notes, np.int64))
            with open(path, "rb") as f:
                raw = f.read()
        finally:
            os.unlink(path)
        cases.append({"name": name, "ppq": ppq, "bpm": bpm, "beats": beats, "notes": notes,
                      "hex": raw.hex()})
    return cases


# ---------------------------------------------------------------- mathexpr

def calc_case(expr):
    try:
        v = calc(expr)
        if isinstance(v, complex):
            return {"expr": expr, "ok": True, "kind": "complex", "re": v.real, "im": v.imag}
        if isinstance(v, bool):
            return {"expr": expr, "ok": True, "kind": "int", "value": int(v)}
        if isinstance(v, int):
            return {"expr": expr, "ok": True, "kind": "int", "value": v}
        return {"expr": expr, "ok": True, "kind": "float", "value": float(v)}
    except ValueError as e:
        return {"expr": expr, "ok": False, "error": str(e)}


def calc_int_case(expr, lo=None, hi=None):
    try:
        return {"expr": expr, "lo": lo, "hi": hi, "ok": True, "value": calc_int(expr, lo, hi)}
    except ValueError as e:
        return {"expr": expr, "lo": lo, "hi": hi, "ok": False, "error": str(e)}


def formula_case(text, xs):
    try:
        fn = formula(text)
    except ValueError as e:
        return {"text": text, "compile_error": str(e)}
    outs = []
    for x in xs:
        try:
            outs.append({"x": x, "ok": True, "value": float(fn(x))})
        except ValueError as e:
            outs.append({"x": x, "ok": False, "error": str(e)})
        except OverflowError as e:
            outs.append({"x": x, "ok": False, "error": str(e)})
        except ZeroDivisionError:
            outs.append({"x": x, "ok": False, "error": "division by zero"})
        except TypeError:
            outs.append({"x": x, "ok": False, "error": "type"})
    return {"text": text, "evals": outs}


def mathexpr_cases():
    fixed = [
        "960*4", "(60+4)*16", "1 + 2 * 3", "2**10", "2^3", "2x3", "1_000 + 0.5",
        "7//2", "-7//2", "7%-3", "-7%3", "1/0", "1.0/0", "1%0", "2**65", "(-8)**0.5",
        "2**3**2", "-2**2", "2**-1", "1e3", "1E-3", ".5", "1.", "1.e3", "0x10", "0o17", "0b101",
        "1__0", "1_", "1x2", "True + True", "None", "1 if 2 else 3", "sin(0)", "2 3", "", "x",
        "1 +", "--3", "+4", "1_0.5", "1e+2", "1e", "1..2", "0x", "0xZ", "  42  ", "2***3", "~3",
        # float // and % use CPython's divmod algorithm ((x/y).floor() is off by 1)
        "(2.//1E-3)", "(127//0.1)", "-(2./(-(127//1E-3)%0.1))", "((1%1.e2)%10)",
    ]
    random.seed(20240928)
    exprs = list(fixed)
    ops = ["+", "-", "*", "/", "//", "%", "**"]
    big = 1 << 63  # integers beyond i64 / JSON number precision are no longer compared
    tries = 0
    while len(exprs) < len(fixed) + 200 and tries < 20000:
        tries += 1
        a, b, c = (random.choice([0, 1, 2, 3, 7, 10, 64, 127]) for _ in range(3))
        op1, op2 = random.choice(ops), random.choice(ops)
        expr = f"({a}{op1}{b}){op2}{c}"
        try:  # integers beyond i128 fall back to floats on the Rust side, so they are not compared
            v = calc(expr)
            if isinstance(v, int) and abs(v) >= big:
                continue
        except ValueError:
            pass
        exprs.append(expr)
    while len(exprs) < len(fixed) + 260:
        a = round(random.uniform(-100, 100), 3)
        b = round(random.uniform(-5, 5), 3)
        exprs.append(f"{a}*{b}+{random.randint(0, 9)}")
    cases = [calc_case(e) for e in exprs]

    int_cases = [
        calc_int_case("960"),
        calc_int_case("960*4", 1, 65535),
        calc_int_case("2.5", 0, 10),
        calc_int_case("2.0", 0, 10),
        calc_int_case("100", 1, 32),
        calc_int_case("33", 1, 32),
        calc_int_case("-1", 0, 10),
        calc_int_case("1/0"),
        calc_int_case("abc"),
        calc_int_case("7//2"),
    ]

    formulas = [
        formula_case("x^2", [-2, 0, 0.5, 3]),
        formula_case("sin(x*pi/2)", [-1, 0, 1, 2]),
        formula_case("1-(1-x)^2", [-1, 0, 0.25, 1, 2]),
        formula_case("log10(x)+ln(x)+log2(x)", [0.5, 1, 10]),
        formula_case("sqrt(x)", [-1, 0, 4]),
        formula_case("asin(x)", [-2, 0, 1]),
        formula_case("abs(x)-floor(x)+ceil(x)-round(x)", [-2.5, 2.5, 3.7]),
        formula_case("min(x, 2, 3) + max(x, 2, 3)", [-1, 2.5, 5]),
        formula_case("pow(x, 2)", [-3, 0, 2]),
        formula_case("1/x", [0, 2]),
        formula_case("x**100", [2]),
        formula_case("exp(x)", [0, 1, 1000]),
        formula_case("cosh(x)", [1000]),
        formula_case("sinh(x)", [10, 1000]),
        formula_case("exp(x)", [1000]),
        formula_case("tan(x)", [math.pi / 2]),
        formula_case("x % 3", [-4, 0, 4]),
        formula_case("x // 2", [-5, 5]),
        formula_case("e^x", [1]),
        formula_case("", []),
        formula_case("x +", []),
        formula_case("foo(x)", []),
        formula_case("foo", []),
        formula_case("sin(x, 2)", [1]),
        formula_case("None", []),
        formula_case("True", []),
        formula_case("1 +_ 2", []),
        formula_case("sin * x", [0]),
    ]

    fmts = [0.0, 960.0, 1.5, -2.25, 0.3333333333, 1e-7, 123456.789, -0.0]
    return {"calc": cases, "calc_int": int_cases, "formula": formulas,
            "fmt": [{"x": x, "text": fmt(x)} for x in fmts]}


def main():
    write("compat", compat_shapes())
    write("project", project_cases())
    write("short", short_cases())
    write("snap", snap_cases())
    write("midi", midi_cases())
    write("mathexpr", [mathexpr_cases()])


if __name__ == "__main__":
    main()
