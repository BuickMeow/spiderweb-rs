"""domino_clip.py 的对照向量。

用 Python 原版生成：clip_data 的输入 / raw / 解压负载，以及 read_notes 的输入 raw /
期望行，存到 crates/spiderweb-domino/tests/vectors/domino.json，Rust 测试逐用例对照。
压缩字节不要求与 Python 逐位相同，所以 Rust 侧只对照解压负载；read_notes 直接读 Python
生成的 raw。

worktree 在临时目录时 vec_common 推不出原版脚本目录，这里补一个回退路径。
"""

import json
import os
import struct
import sys
import zlib

from vec_common import REPO, SCRIPTS

_FALLBACK = "/Users/jieneng/Documents/GitHub/Spiderweb-main/scripts"
if not os.path.isdir(SCRIPTS) and os.path.isdir(_FALLBACK):
    SCRIPTS = _FALLBACK
if SCRIPTS not in sys.path:
    sys.path.insert(0, SCRIPTS)

import numpy as np  # noqa: E402

from files import domino_clip as D  # noqa: E402

OUT = os.path.join(REPO, "crates", "spiderweb-domino", "tests", "vectors", "domino.json")


def wrap(data):
    """原版 clip_data 尾部的包装：MAGIC + 解压后大小 + zlib。"""
    return D.MAGIC + struct.pack("<I", len(data)) + zlib.compress(data)


def note(tick, key, vel, gate):
    """常规布局的单条音符项。"""
    return D.item(2001, D.item(1001, struct.pack("<I", tick)) + D.item(2001, bytes([key])) +
                         D.item(2002, bytes([vel])) + D.item(2003, struct.pack("<I", gate)))


def payload(*tracks, ppq=96):
    """SONG_START + PPQ + SONG_REST + 各轨 + SONG_TAIL 的完整解压负载。"""
    body = D.SONG_START
    if ppq is not None:
        body += D.item(1002, struct.pack("<H", ppq))
    body += D.SONG_REST + b"".join(D.item(1003, t) for t in tracks) + D.SONG_TAIL
    return body


def rows5(notes):
    return np.array(notes, dtype=np.int64).reshape(-1, 5)


# ------------------------------------------------------------------ clip 用例

def clip_cases():
    cases = []

    def add(name, notes, ppq, bar):
        arr = rows5(notes)
        raw = D.clip_data(arr, ppq, bar)
        cases.append({"name": name, "notes": arr.tolist(), "ppq": ppq, "bar": bar,
                      "raw": raw.hex(), "payload": zlib.decompress(raw[len(D.MAGIC) + 4:]).hex()})

    add("single_on_bar", [[960, 1200, 60, 100, 0]], 480, 480)
    add("start_off_bar", [[1000, 1300, 60, 100, 0], [1400, 1990, 61, 110, 0]], 480, 480)
    add("multi_slot_unordered", [[500, 600, 60, 100, 1], [10, 20, 61, 101, 0],
                                 [100, 150, 62, 102, 1], [30, 40, 63, 103, 0]], 96, 240)
    add("multi_bar_multi_slot", [[100, 9000, 36, 90, 2], [3840, 4000, 60, 100, 0],
                                 [9000, 9120, 70, 20, 2], [0, 1, 127, 127, 1]], 480, 3840)
    add("negative_start", [[-5, -3, 60, 100, 0], [7, 3, 61, 100, 1]], 96, 10)
    add("stable_ties", [[5, 9, 60, 100, 0], [5, 4, 60, 101, 0], [5, 6, 59, 102, 0]], 96, 100)
    add("clamp_and_filter", [[0, 5, 200, 0, 0], [3, 9, 60, 200, 0], [7, 7, 61, 100, 0]], 96, 100)
    add("ppq_max", [[0, 240, 60, 100, 0]], 65535, 240)
    add("long_run", [[i * 10, i * 10 + 5, 60 + i % 12, 1 + i % 127, 0] for i in range(130)], 480, 3840)
    return cases


def clip_error_cases():
    cases = []
    trials = [
        ("empty_notes", [], 96, 100),
        ("zero_bar", [[10, 20, 60, 100, 0]], 96, 0),
        ("too_large", [[0, 1 << 33, 60, 100, 0]], 96, 1),
    ]
    for name, notes, ppq, bar in trials:
        try:
            D.clip_data(rows5(notes), ppq, bar)
        except struct.error:
            kind = "too_large"
        except ZeroDivisionError:
            kind = "bad_bar"
        except ValueError:
            kind = "empty"
        else:
            raise AssertionError(f"{name} 原版竟然没报错")
        cases.append({"name": name, "notes": rows5(notes).tolist(), "ppq": ppq, "bar": bar, "error": kind})
    return cases


# ------------------------------------------------------------------ read 用例

def read_case(name, raw):
    """期望行 / ppq 都由 Python 原版算；坏数据记下错误种类。"""
    try:
        rows, ppq = D.read_notes(raw)
    except ValueError as e:
        if "not Domino" in str(e):
            return {"name": name, "raw": raw.hex(), "error": "not_domino"}
        return {"name": name, "raw": raw.hex(), "error": "damaged"}
    return {"name": name, "raw": raw.hex(), "rows": rows.tolist(), "ppq": ppq}


def read_cases(clip):
    cases = []
    for case in clip:
        rows, ppq = D.read_notes(bytes.fromhex(case["raw"]))
        cases.append({"name": "roundtrip_" + case["name"], "raw": case["raw"],
                      "rows": rows.tolist(), "ppq": ppq})

    # 控制器夹在常规音符之间：note_run 提前停，再接着解析。
    t = (D.TRACK_HEAD + note(10, 60, 100, 5) + note(20, 61, 101, 6) + D.item(2004, b"\x01\x02") +
         note(30, 62, 102, 7) + D.TRACK_TAIL)
    cases.append(read_case("controller_between", wrap(payload(t))))

    # 同一长度(34)但字段顺序不对：快路径失败后走其它布局解析。
    odd34 = D.item(2001, D.item(2001, bytes([70])) + D.item(2002, b"") +
                          D.item(2003, struct.pack("<I", 99)) + D.item(1001, struct.pack("<I", 500)))
    cases.append(read_case("odd_same_length", wrap(payload(D.TRACK_HEAD + odd34 + D.TRACK_TAIL))))

    # 缺 2002（力度默认 100）、2002 空 body、不同长度。
    odd_no_vel = D.item(2001, D.item(2001, bytes([71])) + D.item(2003, struct.pack("<I", 88)) +
                               D.item(1001, struct.pack("<I", 600)))
    odd_empty_vel = D.item(2001, D.item(2001, bytes([72])) + D.item(2002, b"") +
                                  D.item(2003, struct.pack("<I", 77)) + D.item(1001, struct.pack("<I", 700)))
    cases.append(read_case("odd_no_velocity", wrap(payload(D.TRACK_HEAD + odd_no_vel + odd_empty_vel + D.TRACK_TAIL))))

    # 内部 tag 重复时最后一个生效（2002 两次）。
    odd_dup = D.item(2001, D.item(1001, struct.pack("<I", 800)) + D.item(2001, bytes([73])) +
                            D.item(2002, bytes([10])) + D.item(2002, bytes([44])) +
                            D.item(2003, struct.pack("<I", 66)))
    cases.append(read_case("odd_duplicate_tags", wrap(payload(D.TRACK_HEAD + odd_dup + D.TRACK_TAIL))))

    # 常规音符 + odd34 + 常规音符：odd 排在所有 runs 后面。
    mixed = (D.TRACK_HEAD + note(11, 60, 100, 5) + odd34 + note(12, 61, 101, 6) + D.TRACK_TAIL)
    cases.append(read_case("odd_after_runs", wrap(payload(mixed))))

    # 两条轨：后面的轨里的常规音符也排在前一轨的 odd 音符前面。
    t0 = D.TRACK_HEAD + note(10, 60, 100, 5) + odd34 + note(20, 61, 101, 6) + D.TRACK_TAIL
    t1 = D.TRACK_HEAD + note(11, 62, 102, 7) + D.TRACK_TAIL
    cases.append(read_case("two_tracks_odd_order", wrap(payload(t0, t1))))

    # key > 127 的其它布局音符被丢掉。
    odd_high_key = D.item(2001, D.item(1001, struct.pack("<I", 900)) + D.item(2001, bytes([200])) +
                                  D.item(2003, struct.pack("<I", 55)) + D.item(1001, struct.pack("<I", 7)))
    cases.append(read_case("odd_high_key", wrap(payload(D.TRACK_HEAD + odd_high_key + D.TRACK_TAIL))))

    # 轨道体里最后一项长度超出轨道范围：直接停。
    truncated_track = D.TRACK_HEAD + note(1, 2, 3, 4) + struct.pack("<HI", 9999, 1000) + b"x"
    cases.append(read_case("truncated_track", wrap(payload(truncated_track))))

    # 顶层项长度超出负载：后面的都不看了（PPQ 已经拿到）。
    cases.append(read_case("truncated_top", wrap(D.SONG_START + D.item(1002, struct.pack("<H", 96)) +
                                                 struct.pack("<HI", 1003, 1000) + b"x")))

    # 没有 PPQ 项；PPQ 项长度不对也当没有。
    cases.append(read_case("no_ppq", wrap(payload(D.TRACK_HEAD + note(1, 2, 3, 4) + D.TRACK_TAIL, ppq=None))))
    cases.append(read_case("wrong_ppq_len", wrap(D.SONG_START + D.item(1002, b"\x01") + D.SONG_TAIL)))

    # 有两个 PPQ 项：最后一个生效。
    cases.append(read_case("last_ppq_wins", wrap(D.SONG_START + D.item(1002, struct.pack("<H", 96)) +
                                                 D.item(1002, struct.pack("<H", 480)) + D.SONG_TAIL)))

    # 空轨、没有轨。
    cases.append(read_case("empty_track", wrap(payload(D.TRACK_HEAD + D.TRACK_TAIL))))
    cases.append(read_case("no_tracks", wrap(payload())))

    # 一个音符都没有的项都被跳过。
    cases.append(read_case("only_settings", wrap(payload(D.TRACK_HEAD + D.item(2001, b"\x01") + D.TRACK_TAIL))))

    # 压缩数据后面还有垃圾：Python 忽略。
    trailing = wrap(payload(D.TRACK_HEAD + note(1, 2, 3, 4) + D.TRACK_TAIL)) + b"JUNK"
    cases.append(read_case("trailing_garbage", trailing))

    # 坏数据。
    cases.append(read_case("empty", b""))
    cases.append(read_case("short", D.MAGIC[:10]))
    cases.append(read_case("magic_only", D.MAGIC))
    cases.append(read_case("not_domino", b"hello world"))
    cases.append(read_case("damaged_junk", D.MAGIC + struct.pack("<I", 5) + b"junk"))
    cases.append(read_case("damaged_empty", D.MAGIC + struct.pack("<I", 0)))
    cases.append(read_case("damaged_truncated", D.MAGIC + struct.pack("<I", 100) +
                           zlib.compress(payload(D.TRACK_HEAD + note(1, 2, 3, 4) + D.TRACK_TAIL))[:-4]))
    return cases


def gen():
    clip = clip_cases()
    return {"module": "domino", "clip": clip, "clip_errors": clip_error_cases(),
            "read": read_cases(clip)}


if __name__ == "__main__":
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    data = gen()
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False)
    print(f"domino: clip={len(data['clip'])} clip_errors={len(data['clip_errors'])} "
          f"read={len(data['read'])} -> {OUT}")
