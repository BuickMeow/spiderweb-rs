"""arc.py 的对照向量。

所有输入的浮点数先量化到 15 位有效数字再交给 Python 原版计算：
这样 serde_json 的默认浮点解析（无 float_roundtrip 特性）也能精确还原，
从而保证 Rust 与原版在同一批 double 上运算。
"""

import math
import os
import sys

from vec_common import write

# 在 worktree 里跑时 vec_common 推不出原版脚本目录，这里补一个回退路径
_SCRIPTS = (os.environ.get("SPIDERWEB_SCRIPTS") or os.environ.get("SPIDERWEB_SRC")
            or "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts")
if os.path.isdir(_SCRIPTS) and _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)

from notes import arc as A


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


def add(cases, fn, args, call):
    a = rjson(args)
    check(a)
    cases.append({"fn": fn, "args": a, "out": call(*a)})


def arc_k_one(k):
    return A.arc_k({"k": k})


def gen():
    cases = []

    # ---- circle：一般三点 / 共线 / 重合 / 近似共线
    triples = [
        ([0.0, 0.0], [1.0, 1.0], [2.0, 0.0]),
        ([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]),           # 共线
        ([0.0, 0.0], [0.0, 0.0], [1.0, 2.0]),           # 前两点重合
        ([1.0, 1.0], [2.0, 2.0], [1.0, 1.0]),           # 首尾重合
        ([0.0, 0.0], [3.0, 4.0], [6.0, 8.0000000001]),  # 近似共线
        ([-2.5, 3.25], [0.0, 7.0], [4.0, -1.0]),
        ([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]),
        ([5.0, 5.0], [5.0, 5.0], [5.0, 5.0]),           # 全重合
    ]
    for triple in triples:
        add(cases, "circle", list(triple), A.circle)

    # ---- full_circle
    whole = [
        ([0.0, 0.0], [1.0, 0.0], [0.0, 0.0]),
        ([1.0, 1.0], [2.0, 2.0], [1.0, 1.0]),
        ([0.0, 0.0], [1.0, 0.0], [1.0, 0.0]),  # 首尾不同
        ([0.0, 0.0], [0.0, 0.0], [0.0, 0.0]),  # 全同
        ([-1.0, 2.0], [3.0, -0.5], [-1.0, 2.0]),
    ]
    for triple in whole:
        add(cases, "full_circle", list(triple), A.full_circle)

    # ---- arc_circle：一般三点 / k / 整圆 / 共线（没有圆弧时 None）
    for p, k in (
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 2.0),
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0),
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0),
    ):
        add(cases, "arc_circle", [p, k], A.arc_circle)

    # ---- arc_points：k / step / 共线 / 整圆 / 超半圆 / 点不足
    arc_pts = [
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0, A.STEP),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5, A.STEP),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 2.0, A.STEP),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 1.0, A.STEP),    # 超半圆
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0, 0.5),         # 整圆（大 step）
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0, A.STEP),      # 整圆（默认 step）
        ([[0.0, 1.0], [1.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # 恰转过 0
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0, A.STEP),      # 共线
        ([[0.0, 0.0], [0.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # 中间点等于起点
        ([[0.0, 0.0], [2.0, 0.0], [2.0, 0.0]], 1.0, A.STEP),      # 中间点等于终点
        ([[0.0, 0.0], [1.0, 2.0]], 1.0, A.STEP),                  # 只有两点
        ([[3.0, 4.0]], 1.0, A.STEP),                              # 只有一点
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0, math.pi / 8),
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 0.25, math.pi / 4),
    ]
    for p, k, step in arc_pts:
        add(cases, "arc_points", [p, k, step], A.arc_points)

    # ---- arc_bezier：四分之一分段 / k / 整圆 / 顺逆时针 / 共线
    arc_bez = [
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.0),        # 半圆 -> 2 段
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 0.5),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 2.0),
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 1.7),        # k 非整数倍
        ([[0.0, 0.0], [0.0, 1.0], [-1.0, 0.0]], 1.0),       # 超半圆 -> 3 段
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 1.0),        # 整圆 -> 4 段
        ([[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]], 0.5),
        ([[0.0, 1.0], [1.0, 0.0], [2.0, 1.0]], 1.0),        # 顺时针
        ([[2.0, 3.0], [2.0, 5.0], [2.0, 7.0]], 1.0),        # 共线 -> 直线
        ([[0.0, 0.0], [0.0, 0.0], [2.0, 0.0]], 1.0),
        ([[0.0, 0.0], [3.0, 0.0], [0.0, 0.0]], 2.5),        # 压扁的整圆
        ([[0.0, 0.0], [1.0, 0.001], [2.0, 0.0]], 1.0),      # 很平的弧
        ([[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], 10.0),       # 极扁（近直线）
    ]
    for p, k in arc_bez:
        add(cases, "arc_bezier", [p, k], A.arc_bezier)

    # ---- ellipse_bezier / line_bezier
    for box in ([0.0, 0.0, 4.0, 2.0], [1.0, 2.0, 1.0, 5.0], [-3.0, -2.0, -1.0, 0.0], [2.0, 2.0, 2.0, 2.0]):
        add(cases, "ellipse_bezier", [box], A.ellipse_bezier)

    for a, c in (([0.0, 0.0], [6.0, 3.0]), ([2.0, 5.0], [-2.0, -5.0]), ([1.0, 1.0], [1.0, 1.0])):
        add(cases, "line_bezier", [a, c], A.line_bezier)

    # ---- clean_k（Python 侧为 arc_k({"k": ...})）：边界、负数、inf / nan
    ks = [1.0, 0.5, 2.0, 1e-9, 1.0001e-9, 1e9, 999999999.0, 1e10, -1.0, 0.0, "inf", "-inf", "nan"]
    for k in ks:
        add(cases, "clean_k", [k], arc_k_one)

    return {"module": "arc", "cases": cases}


if __name__ == "__main__":
    data = gen()
    # 自检：所有输出里的浮点也要能被 Rust 侧按 1e-9 容差比较即可，无需量化
    write("arc", data["cases"])
