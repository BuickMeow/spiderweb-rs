"""生成基准工程（autosave / 工程 JSON），用于性能测试。

用法：
  python3 tools/gen_bench_project.py --notes 5000000 -o bench.json
  python3 tools/gen_bench_project.py --notes 500000 -o small.json

结构：一个覆盖 128 键的 spam 自定义形状（gate = 1/64 音符 = 0.0625 beat，
aligned），外加一条带肿瘤的斜线和一个小漏斗，模拟"同屏混合负载"。
音符数只由 PPQ 与形状几何决定，所以这里的 --notes 是近似目标值。
"""

import argparse
import json
import math

PPQ = 960
GATE = 0.0625  # 1/64 音符（beat 单位）
KEYS = 128


def spam_shape(name, notes, start_beat):
    """覆盖 128 键、总音符数约 notes 的 spam 矩形。"""
    per_key = max(1, math.ceil(notes / KEYS))
    span = per_key * GATE
    # 三个角：左下 (u=0,v=0)、右下 (u=1,v=0)、左上 (u=0,v=1)
    b0, p0 = start_beat, -0.5
    pts = [[b0, p0], [b0 + span, p0], [b0, p0 + KEYS]]
    box = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
    return {
        "vel0": 100,
        "vel1": 100,
        "end_dot": False,
        "kind": "custom",
        "pts": pts,
        "name": name,
        "strokes": [{"kind": "poly", "pts": box}],
        "fill": "spam",
        "gate": GATE,
        "align": "aligned",
    }, span


def tumour_line(start_beat, span):
    """一条带凸起的斜线（锯齿），覆盖 64 个键。"""
    return {
        "vel0": 90,
        "vel1": 120,
        "end_dot": False,
        "kind": "line",
        "pts": [[start_beat, 40.0], [start_beat + span, 104.0]],
        "tumour": {
            "on": True,
            "shape": "triangle",
            "size": 4.0,
            "length": 0.25,
            "dist": 0.25,
            "side": "alt",
            "wrap": "simple",
            "start": 0.0,
            "end": 1.0,
            "ease": 0.0,
            "fit": True,
            "seed": 7,
            "mirror": False,
            "k": 0.25,
        },
    }


def funnel_shape(start_beat, span):
    """一个 spam 漏斗：从 (start, 60) 张开到墙。"""
    return {
        "vel0": 80,
        "vel1": 110,
        "end_dot": False,
        "kind": "funnel",
        "pts": [
            [start_beat, 60.0],
            [start_beat + span, 60.0],
            [start_beat + span, 20.0],
            [start_beat + span, 100.0],
        ],
        "starts": [{"line": 0, "at": 0.0, "ends": [
            {"pts": [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]], "sharp": []},
            {"pts": [[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]], "sharp": []},
        ]}],
        "fill": "spam",
        "gate0": 0.0625,
        "gate1": 0.0625,
        "vary": False,
        "change": "steps",
        "follow": "time",
        "wall": "in",
    }


def project(shapes, ppq=PPQ):
    return {
        "version": 2,
        "app_version": "bench",
        "ppq": str(ppq),
        "bpm": "120",
        "beats": "4",
        "output": "",
        "channel_mode": "single",
        "channel_split": "key",
        "snap": "1/16",
        "defaults": {"vel0": 100.0, "vel1": 100.0, "end_dot": False},
        "custom_defaults": {"fill": "spam", "gate": GATE, "align": "aligned", "shape": "Circle"},
        "funnel_defaults": {"fill": "spam", "gate0": GATE, "gate1": GATE, "vary": False,
                            "change": "steps", "follow": "time", "wall": "in"},
        "text_defaults": {"text": "", "font": "Arial", "size": 24.0, "unit": "font", "weight": 400,
                          "italic": False, "tracking": 0.0, "leading": 100.0, "align": "left",
                          "threshold": 50.0, "grow": 0.0, "bbox": [0.0, 0.0, 1.0, 1.0], "cap": 0.7,
                          "k": 1.0, "holes": []},
        "free_smooth": 0,
        "shapes": shapes,
        "view": None,
        "playhead": 0.0,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--notes", type=int, default=1_000_000, help="spam 形状的近似音符总数")
    ap.add_argument("--mix", action="store_true", default=True, help="额外加肿瘤线与漏斗（默认开）")
    ap.add_argument("--out", "-o", default="bench.json")
    args = ap.parse_args()

    spam, span = spam_shape("Bench spam", args.notes, 0.0)
    shapes = [spam]
    if args.mix:
        shapes.append(tumour_line(0.0, span))
        shapes.append(funnel_shape(span * 0.2, span * 0.6))
    data = project(shapes)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False)
    print(f"wrote {args.out}: {len(shapes)} shapes, spam span {span:.0f} beats "
          f"({span / 4:.0f} bars), target ~{args.notes:,} notes")


if __name__ == "__main__":
    main()
