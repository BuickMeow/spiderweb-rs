"""Differential vectors for paths.py."""

import math
import random

import numpy as np

from vec_common import write

from notes import paths as P


def tolist(x):
    if isinstance(x, np.ndarray):
        return x.tolist()
    if isinstance(x, (list, tuple)):
        return [tolist(v) for v in x]
    return x


def gen():
    cases = []
    rnd = random.Random(7)

    def add(fn, args, out):
        cases.append({"fn": fn, "args": args, "out": tolist(out)})

    # dedupe
    add("dedupe", [[[0.0, 1.0], [0.0, 1.0], [1 / 3, 2.5], [1 / 3, 2.5], [1 / 3, 2.6]]],
        P.dedupe([[0.0, 1.0], [0.0, 1.0], [1 / 3, 2.5], [1 / 3, 2.5], [1 / 3, 2.6]]))

    # direction_changes
    add("direction_changes", [[60.0, 64.0, 62.0, 62.0, 70.0]], P.direction_changes([60.0, 64.0, 62.0, 62.0, 70.0]))
    add("direction_changes", [[1.0, 1.0, 1.0]], P.direction_changes([1.0, 1.0, 1.0]))

    # spans
    add("spans", [[0, 4], [2, 7], None], P.spans(np.array([0, 4]), np.array([2, 7])))
    add("spans", [[0, 4], [2, 7], [True, False]], P.spans(np.array([0, 4]), np.array([2, 7]), np.array([True, False])))

    # stretch_ends
    add("stretch_ends", [[[0.0, 60.0], [100.0, 64.0]], False], P.stretch_ends([[0.0, 60.0], [100.0, 64.0]], False))
    add("stretch_ends", [[[0.0, 64.0], [100.0, 60.0]], False], P.stretch_ends([[0.0, 64.0], [100.0, 60.0]], False))
    add("stretch_ends", [[[0.0, 60.0], [50.0, 64.0], [100.0, 58.0]], False],
        P.stretch_ends([[0.0, 60.0], [50.0, 64.0], [100.0, 58.0]], False))
    add("stretch_ends", [[[0.0, 60.0], [100.0, 64.0]], True], P.stretch_ends([[0.0, 60.0], [100.0, 64.0]], True))
    add("stretch_ends", [[[0.0, 60.3], [100.0, 63.8]], False], P.stretch_ends([[0.0, 60.3], [100.0, 63.8]], False))

    # keep_longest
    add("keep_longest", [[[10, 20, 60], [10, 15, 60], [30, 42, 61], [30, 42, 61], [5, -1, 60]]],
        P.keep_longest(np.array([[10, 20, 60], [10, 15, 60], [30, 42, 61], [30, 42, 61], [5, -1, 60]])))
    add("keep_longest", [[[10, 20, 60]]], P.keep_longest(np.array([[10, 20, 60]])))

    # loop_from_left
    add("loop_from_left", [[[20.0, 60.0], [10.0, 64.0], [0.0, 56.0], [20.0, 60.0]]],
        P.loop_from_left(np.array([[20.0, 60.0], [10.0, 64.0], [0.0, 56.0], [20.0, 60.0]])))

    # ends_forward
    add("ends_forward", [[[0.0, 60.0], [100.0, 64.0]]], P.ends_forward(np.array([[0.0, 60.0], [100.0, 64.0]])))
    add("ends_forward", [[[100.0, 60.0], [50.0, 62.0], [3.0, 64.0]]], P.ends_forward(np.array([[100.0, 60.0], [50.0, 62.0], [3.0, 64.0]])))
    add("ends_forward", [[[0.0, 60.0], [0.0, 64.0]]], P.ends_forward(np.array([[0.0, 60.0], [0.0, 64.0]])))

    # line_notes / path_notes: hand-picked cases + random ones
    hand_paths = [
        [[0.0, 60.0], [100.0, 64.0]],
        [[0.0, 64.0], [100.0, 60.0]],
        [[0.0, 60.0], [50.0, 64.0], [100.0, 58.0]],
        [[0.0, 60.0], [0.0, 64.0], [100.0, 64.0]],
        [[0.0, 60.0], [100.0, 64.0], [100.0, 55.0], [20.0, 55.0]],
        [[0.0, 60.0], [0.5, 61.0], [1.0, 62.0], [1.5, 63.0], [2.0, 60.0]],
        [[0.0, 60.0], [100.0, 66.0], [0.0, 60.0]],
        [[10.0, 60.0], [30.0, 62.0], [30.0, 70.0], [60.0, 70.1], [60.0, 61.0], [90.0, 61.0]],
        [[0.0, 60.3], [100.5, 64.7]],
        [[-10.0, 60.0], [40.0, 62.0]],
    ]
    for i, p in enumerate(hand_paths):
        add("line_notes", [p, False], P.line_notes(np.array(p), False))
        add("line_notes", [p, True], P.line_notes(np.array(p), True))
        add("path_notes", [p, False], P.path_notes(np.array(p), False))
        add("path_notes", [p, True], P.path_notes(np.array(p), True))

    for i in range(14):
        n = rnd.randint(2, 9)
        t = 0.0
        p = rnd.uniform(55, 70)
        pts = [[t, p]]
        for _ in range(n - 1):
            if rnd.random() < 0.25:
                t += 0.0  # vertical
            else:
                t += rnd.uniform(1, 60) * (1 if rnd.random() < 0.8 else -1)
            p += rnd.uniform(-6, 6)
            pts.append([round(t, 3), round(p, 3)])
        flag = rnd.random() < 0.5
        add("line_notes", [pts, flag], P.line_notes(np.array(pts), flag))
        add("path_notes", [pts, True], P.path_notes(np.array(pts), True))
        add("path_notes", [pts, False], P.path_notes(np.array(pts), False))

    # call parts_notes directly
    r = [[0.0, 60.0], [10.0, 60.0], [20.0, 64.0], [30.0, 64.0], [40.0, 62.0]]
    notes, per = P.parts_notes(np.array(r), np.array([0, 2]), np.array([False, True]), True)
    cases.append({"fn": "parts_notes", "args": [r, [0, 2], [False, True], True],
                  "out": [notes.tolist(), per.tolist()]})

    # dot_segment_notes: polyline
    add("dot_segment_notes", [[[0.0, 60.0], [100.0, 64.0], [50.0, 68.0]]],
        P.dot_segment_notes(np.array([[0.0, 60.0], [100.0, 64.0], [50.0, 68.0]])))
    add("dot_segment_notes", [[[0.0, 60.0], [40.0, 62.0], [80.0, 62.0], [120.0, 65.0]]],
        P.dot_segment_notes(np.array([[0.0, 60.0], [40.0, 62.0], [80.0, 62.0], [120.0, 65.0]])))
    add("dot_segment_notes", [[[30.0, 60.0], [10.0, 63.0], [10.0, 65.0]]],
        P.dot_segment_notes(np.array([[30.0, 60.0], [10.0, 63.0], [10.0, 65.0]])))

    return {"module": "paths", "cases": cases}


if __name__ == "__main__":
    data = gen()
    write("paths", data["cases"])
