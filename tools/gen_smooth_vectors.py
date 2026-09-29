"""Differential vectors for smooth.py.

Every input float is first quantised to 15 significant digits before being handed to the Python
original: that way serde_json's default float parsing (without the float_roundtrip feature) can
reproduce it exactly, guaranteeing Rust and the original operate on the same doubles. The output
keeps the full precision the original computes.
"""

import math
import os
import random
import sys

from vec_common import write

# when run in the worktree, vec_common cannot derive the original script dir; add a fallback path
_SCRIPTS = os.environ.get("SPIDERWEB_SCRIPTS", "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from notes import smooth as S


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


# ---------------------------------------------------------------- input strokes

def noisy_line(seed, n=26, noise=0.35):
    r = random.Random(seed)
    return [[100.0 * i / (n - 1) + r.uniform(-noise, noise),
             60.0 + r.uniform(-noise, noise)] for i in range(n)]


def cornered(seed, noise=0.22, per=14):
    r = random.Random(seed)
    base = [[0.0, 60.0], [38.0, 60.0], [38.0, 65.0], [76.0, 65.0], [76.0, 56.0], [100.0, 56.0]]
    out = []
    for k in range(len(base) - 1):
        ax, ay = base[k]
        bx, by = base[k + 1]
        for i in range(per):
            t = i / per
            out.append([ax + (bx - ax) * t + r.uniform(-noise, noise),
                        ay + (by - ay) * t + r.uniform(-noise, noise)])
    out.append(base[-1])
    return out


def wave(seed, n=64, amp=4.0, noise=0.2):
    r = random.Random(seed)
    return [[100.0 * i / (n - 1), 60.0 + amp * math.sin(i / 5.5) + r.uniform(-noise, noise)]
            for i in range(n)]


def s_shape(seed, n=72, noise=0.25):
    r = random.Random(seed)
    return [[100.0 * i / (n - 1),
             60.0 + 5.0 * math.sin(2 * math.pi * i / (n - 1)) + r.uniform(-noise, noise)]
            for i in range(n)]


def circle_ring(seed, n=40, rad=38.0, noise=0.5, offset=(0.0, 0.0)):
    r = random.Random(seed)
    pts = []
    for i in range(n):
        a = 2 * math.pi * i / n
        rr = rad + r.uniform(-noise, noise)
        pts.append([rr * math.cos(a) + r.uniform(-0.2, 0.2),
                    60.0 + rr * math.sin(a) + r.uniform(-0.2, 0.2)])
    return pts + [[pts[0][0] + offset[0], pts[0][1] + offset[1]]]


def ellipse_ring(seed, n=44, a=54.0, b=26.0, tilt=0.5, noise=0.5, offset=(0.0, 0.0)):
    r = random.Random(seed)
    ca, sa = math.cos(tilt), math.sin(tilt)
    pts = []
    for i in range(n):
        t = 2 * math.pi * i / n
        aa = a + r.uniform(-noise, noise)
        bb = b + r.uniform(-noise, noise)
        x = aa * math.cos(t)
        y = bb * math.sin(t)
        pts.append([x * ca - y * sa + r.uniform(-0.2, 0.2),
                    60.0 + x * sa + y * ca + r.uniform(-0.2, 0.2)])
    return pts + [[pts[0][0] + offset[0], pts[0][1] + offset[1]]]


def box_ring(seed, hw=30.0, hh=24.0, per=14, noise=0.45):
    r = random.Random(seed)
    cs = [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]]
    pts = []
    for k in range(4):
        ax, ay = cs[k]
        bx, by = cs[(k + 1) % 4]
        for i in range(per):
            t = i / per
            pts.append([ax + (bx - ax) * t + r.uniform(-noise, noise),
                        60.0 + ay + (by - ay) * t + r.uniform(-noise, noise)])
    return pts + [list(pts[0])]


def polygon_ring(seed, corners_n=4, rad=34.0, per=12, noise=0.45):
    r = random.Random(seed)
    cs = [[rad * math.cos(2 * math.pi * k / corners_n + 0.3),
           rad * math.sin(2 * math.pi * k / corners_n + 0.3)] for k in range(corners_n)]
    pts = []
    for k in range(corners_n):
        ax, ay = cs[k]
        bx, by = cs[(k + 1) % corners_n]
        for i in range(per):
            t = i / per
            pts.append([ax + (bx - ax) * t + r.uniform(-noise, noise),
                        60.0 + ay + (by - ay) * t + r.uniform(-noise, noise)])
    return pts + [list(pts[0])]


def random_loop(seed, n=26, offset=(0.0, 0.0)):
    r = random.Random(seed)
    x, y = 0.0, 0.0
    pts = [[0.0, 60.0]]
    for _ in range(n):
        x += r.uniform(2.0, 10.0)
        y += r.uniform(-7.0, 7.0)
        pts.append([x, 60.0 + y])
    pts.append([offset[0], 60.0 + offset[1]])
    return pts


def scribble(seed, n=90):
    r = random.Random(seed)
    pts = [[0.0, 60.0]]
    x = 0.0
    for _ in range(n - 1):
        x += r.uniform(0.5, 2.5)
        pts.append([x, 60.0 + 6.0 * math.sin(x / 7.0) + r.uniform(-1.2, 1.2)])
    return pts


def gen():
    cases = []

    # ---- tolerance: level bounds / size bounds
    for level in (0.0, 0.5, 1.0, 12.5, 30.0, 50.0, 60.0, 99.9, 100.0, 100.5, 150.0, -5.0, 33.3):
        for size in (0.0, 1.0, 12.75, 123.456):
            add(cases, "tolerance", [level, size], S.tolerance)

    # ---- clean_level: rounding (.5 to even), clamping, extreme values
    for v in (0.0, 1.0, 7.0, 60.0, 100.0, 150.0, -20.0, -0.5, 0.5, 1.5, 2.5, 33.5, 44.5, 45.5,
              99.4, 99.6, 100.4, 1e6, 1e-6, 1e300, -1e300):
        add(cases, "clean_level", [v], S.clean_level)

    # ---- smooth_path: fixed strokes × four levels
    P = {
        "line": noisy_line(11),
        "line_flat": noisy_line(12, noise=0.08),
        "corner": cornered(13),
        "wave": wave(14),
        "s": s_shape(15),
        "circle": circle_ring(16),
        "ellipse": ellipse_ring(17),
        "box": box_ring(18),
        "triangle": polygon_ring(19, 3),
        "pentagon": polygon_ring(20, 5),
        "hexagon": polygon_ring(21, 6),
        "scribble": scribble(22),
    }
    for pts in P.values():
        for level in (0, 30, 60, 100):
            add(cases, "smooth_path", [pts, float(level), 1.0], S.smooth_path)

    # ---- k != 1: x is squeezed / stretched on screen
    for name, k in (("line", 2.0), ("line", 0.5), ("wave", 3.0), ("circle", 0.5),
                    ("ellipse", 2.0), ("box", 3.0), ("triangle", 0.25), ("s", 1.5)):
        for level in (30, 60, 100):
            add(cases, "smooth_path", [P[name], float(level), k], S.smooth_path)

    # ---- random-walk closed loops (12 seeds; the last 4 close almost but not quite)
    for seed in range(12):
        offset = (0.0, 0.0) if seed < 8 else (2.5, 1.5)
        pts = random_loop(100 + seed, offset=offset)
        for level in (30, 55, 80, 100):
            add(cases, "smooth_path", [pts, float(level), 1.0], S.smooth_path)

    # ---- ring gaps near the LOOP / closed boundary
    for off in ((1.0, 0.5), (4.0, 2.0), (8.0, 0.0), (20.0, 0.0)):
        add(cases, "smooth_path", [circle_ring(60, offset=off), 60.0, 1.0], S.smooth_path)

    # ---- degenerate: fewer than 3 points, k <= 0, level <= 0, coincident points
    add(cases, "smooth_path", [[[0.0, 60.0], [5.0, 61.0]], 60.0, 1.0], S.smooth_path)
    add(cases, "smooth_path", [[[0.0, 60.0], [5.0, 61.0], [10.0, 60.0]], 60.0, 0.0], S.smooth_path)
    add(cases, "smooth_path", [[[0.0, 60.0], [5.0, 61.0], [10.0, 60.0]], 60.0, -2.0], S.smooth_path)
    add(cases, "smooth_path", [[[0.0, 60.0], [0.0, 60.0], [0.0, 60.0], [0.0, 60.0]], 60.0, 1.0],
        S.smooth_path)
    dup = [[0.0, 60.0], [10.0, 60.0], [10.0, 60.0], [20.0, 63.0], [20.0, 63.0], [30.0, 60.0]]
    add(cases, "smooth_path", [dup, 60.0, 1.0], S.smooth_path)
    add(cases, "smooth_path", [P["line"], -30.0, 1.0], S.smooth_path)
    add(cases, "smooth_path", [P["line"], 55.7, 1.0], S.smooth_path)

    return {"module": "smooth", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("smooth", data["cases"])
