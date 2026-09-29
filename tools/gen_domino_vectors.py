"""Differential vectors for domino_clip.py.

Generated with the Python original: clip_data's input / raw / decompressed payload, and
read_notes' input raw / expected rows, saved to
crates/spiderweb-domino/tests/vectors/domino.json for the Rust tests to compare case by case.
The compressed bytes need not match Python bit for bit, so the Rust side only compares the
decompressed payload; read_notes reads the Python-generated raw directly.

When the worktree is in a temp dir, vec_common cannot derive the original script dir; add a
fallback path here.
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
    """The wrapper the original clip_data appends: MAGIC + decompressed size + zlib."""
    return D.MAGIC + struct.pack("<I", len(data)) + zlib.compress(data)


def note(tick, key, vel, gate):
    """One note item in the regular layout."""
    return D.item(2001, D.item(1001, struct.pack("<I", tick)) + D.item(2001, bytes([key])) +
                         D.item(2002, bytes([vel])) + D.item(2003, struct.pack("<I", gate)))


def payload(*tracks, ppq=96):
    """The full decompressed payload: SONG_START + PPQ + SONG_REST + each track + SONG_TAIL."""
    body = D.SONG_START
    if ppq is not None:
        body += D.item(1002, struct.pack("<H", ppq))
    body += D.SONG_REST + b"".join(D.item(1003, t) for t in tracks) + D.SONG_TAIL
    return body


def rows5(notes):
    return np.array(notes, dtype=np.int64).reshape(-1, 5)


# ------------------------------------------------------------------ clip cases

def clip_cases():
    cases = []

    def add(name, notes, ppq, bar, start="bar"):
        arr = rows5(notes)
        raw = D.clip_data(arr, ppq, bar, start)
        cases.append({"name": name, "notes": arr.tolist(), "ppq": ppq, "bar": bar, "start": start,
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
    # start="note": the first note is at tick 0, no empty lead (the length is not padded to bars)
    add("note_start_off", [[1000, 1300, 60, 100, 0], [1400, 1990, 61, 110, 0]], 480, 480, "note")
    add("note_multi_slot", [[500, 600, 60, 100, 1], [10, 20, 61, 101, 0],
                            [100, 150, 62, 102, 1], [30, 40, 63, 103, 0]], 96, 240, "note")
    add("note_negative_start", [[-5, -3, 60, 100, 0], [7, 3, 61, 100, 1]], 96, 10, "note")
    add("note_single_tick", [[5, 5, 60, 100, 0]], 96, 100, "note")
    add("note_zero_bar", [[10, 20, 60, 100, 0]], 96, 0, "note")  # the note start never uses bar
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
            raise AssertionError(f"{name}: the original unexpectedly did not error")
        cases.append({"name": name, "notes": rows5(notes).tolist(), "ppq": ppq, "bar": bar,
                      "start": "bar", "error": kind})
    # the note start: length is max_end - min_start (no bar padding); over u32 it errors too
    try:
        D.clip_data(rows5([[0, 1 << 33, 60, 100, 0]]), 96, 1, "note")
    except struct.error:
        cases.append({"name": "too_large_note", "notes": [[0, 1 << 33, 60, 100, 0]], "ppq": 96,
                      "bar": 1, "start": "note", "error": "too_large"})
    else:
        raise AssertionError("too_large_note: upstream did not raise")
    return cases


# ------------------------------------------------------------------ read cases

def read_case(name, raw):
    """Expected rows / ppq are both computed by the Python original; bad data records the error kind."""
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

    # A controller between regular notes: note_run stops early and parsing resumes after it.
    t = (D.TRACK_HEAD + note(10, 60, 100, 5) + note(20, 61, 101, 6) + D.item(2004, b"\x01\x02") +
         note(30, 62, 102, 7) + D.TRACK_TAIL)
    cases.append(read_case("controller_between", wrap(payload(t))))

    # Same length (34) but wrong field order: the fast path fails and parsing continues with the other layout.
    odd34 = D.item(2001, D.item(2001, bytes([70])) + D.item(2002, b"") +
                          D.item(2003, struct.pack("<I", 99)) + D.item(1001, struct.pack("<I", 500)))
    cases.append(read_case("odd_same_length", wrap(payload(D.TRACK_HEAD + odd34 + D.TRACK_TAIL))))

    # Missing 2002 (velocity defaults to 100), empty 2002 body, different lengths.
    odd_no_vel = D.item(2001, D.item(2001, bytes([71])) + D.item(2003, struct.pack("<I", 88)) +
                               D.item(1001, struct.pack("<I", 600)))
    odd_empty_vel = D.item(2001, D.item(2001, bytes([72])) + D.item(2002, b"") +
                                  D.item(2003, struct.pack("<I", 77)) + D.item(1001, struct.pack("<I", 700)))
    cases.append(read_case("odd_no_velocity", wrap(payload(D.TRACK_HEAD + odd_no_vel + odd_empty_vel + D.TRACK_TAIL))))

    # With duplicate inner tags the last one wins (2002 twice).
    odd_dup = D.item(2001, D.item(1001, struct.pack("<I", 800)) + D.item(2001, bytes([73])) +
                            D.item(2002, bytes([10])) + D.item(2002, bytes([44])) +
                            D.item(2003, struct.pack("<I", 66)))
    cases.append(read_case("odd_duplicate_tags", wrap(payload(D.TRACK_HEAD + odd_dup + D.TRACK_TAIL))))

    # regular note + odd34 + regular note: odd is placed after all runs.
    mixed = (D.TRACK_HEAD + note(11, 60, 100, 5) + odd34 + note(12, 61, 101, 6) + D.TRACK_TAIL)
    cases.append(read_case("odd_after_runs", wrap(payload(mixed))))

    # Two tracks: regular notes in the later track also come before the odd notes of the previous track.
    t0 = D.TRACK_HEAD + note(10, 60, 100, 5) + odd34 + note(20, 61, 101, 6) + D.TRACK_TAIL
    t1 = D.TRACK_HEAD + note(11, 62, 102, 7) + D.TRACK_TAIL
    cases.append(read_case("two_tracks_odd_order", wrap(payload(t0, t1))))

    # Other-layout notes with key > 127 are dropped.
    odd_high_key = D.item(2001, D.item(1001, struct.pack("<I", 900)) + D.item(2001, bytes([200])) +
                                  D.item(2003, struct.pack("<I", 55)) + D.item(1001, struct.pack("<I", 7)))
    cases.append(read_case("odd_high_key", wrap(payload(D.TRACK_HEAD + odd_high_key + D.TRACK_TAIL))))

    # The last item in a track body has a length beyond the track: stop right there.
    truncated_track = D.TRACK_HEAD + note(1, 2, 3, 4) + struct.pack("<HI", 9999, 1000) + b"x"
    cases.append(read_case("truncated_track", wrap(payload(truncated_track))))

    # A top-level item has a length beyond the payload: nothing after it is read (PPQ is already captured).
    cases.append(read_case("truncated_top", wrap(D.SONG_START + D.item(1002, struct.pack("<H", 96)) +
                                                 struct.pack("<HI", 1003, 1000) + b"x")))

    # No PPQ item; a PPQ item with the wrong length also counts as none.
    cases.append(read_case("no_ppq", wrap(payload(D.TRACK_HEAD + note(1, 2, 3, 4) + D.TRACK_TAIL, ppq=None))))
    cases.append(read_case("wrong_ppq_len", wrap(D.SONG_START + D.item(1002, b"\x01") + D.SONG_TAIL)))

    # Two PPQ items: the last one wins.
    cases.append(read_case("last_ppq_wins", wrap(D.SONG_START + D.item(1002, struct.pack("<H", 96)) +
                                                 D.item(1002, struct.pack("<H", 480)) + D.SONG_TAIL)))

    # Empty track, no tracks.
    cases.append(read_case("empty_track", wrap(payload(D.TRACK_HEAD + D.TRACK_TAIL))))
    cases.append(read_case("no_tracks", wrap(payload())))

    # Items without a single note are all skipped.
    cases.append(read_case("only_settings", wrap(payload(D.TRACK_HEAD + D.item(2001, b"\x01") + D.TRACK_TAIL))))

    # Garbage after the compressed data: Python ignores it.
    trailing = wrap(payload(D.TRACK_HEAD + note(1, 2, 3, 4) + D.TRACK_TAIL)) + b"JUNK"
    cases.append(read_case("trailing_garbage", trailing))

    # Bad data.
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
