//! Differential tests against files/snap.py (vectors from tools/gen_io_vectors.py).

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::snap::{
    CustomParts, DEFAULT_SNAP, SNAP_LIST, SNAPS, clean_snap, custom_parts, custom_snap, snap_beats,
    whole_notes,
};

fn parts(v: &Value) -> Option<CustomParts> {
    let a = v.as_array()?;
    Some(CustomParts {
        count: s(&a[0]).to_string(),
        note: i(&a[1]),
        div: i(&a[2]),
    })
}

#[test]
fn snap_vectors() {
    let cases = cases("snap", "cases");
    for case in &cases {
        match s(&case["fn"]) {
            "snap" => {
                let snap = s(&case["snap"]);
                let got_parts = custom_parts(snap);
                let got_whole = whole_notes(snap);
                assert_eq!(
                    got_parts.as_ref(),
                    parts(&case["parts"]).as_ref(),
                    "{snap:?}: custom_parts"
                );
                let want_whole = if case["whole"].is_null() {
                    None
                } else {
                    Some((i(&case["whole"]["num"]), i(&case["whole"]["den"])))
                };
                let got_whole = got_whole.map(|w| (w.num as i64, w.den as i64));
                assert_eq!(got_whole, want_whole, "{snap:?}: whole_notes");
                let want_beats = case["beats"].as_f64();
                assert_eq!(
                    spiderweb_io::snap::snap_beats(snap, 4.0),
                    want_beats,
                    "{snap:?}: snap_beats"
                );
                assert_eq!(clean_snap(snap), s(&case["clean"]), "{snap:?}: clean_snap");
            }
            "custom_snap" => {
                let got = custom_snap(s(&case["count"]), i(&case["note"]), i(&case["div"]));
                assert_eq!(got, s(&case["snap"]), "custom_snap {}", case["count"]);
            }
            "snap_beats" => {
                let got = snap_beats(s(&case["snap"]), f(&case["beats_per_bar"]));
                assert_eq!(
                    got,
                    case["beats"].as_f64(),
                    "snap_beats {} / {}",
                    s(&case["snap"]),
                    f(&case["beats_per_bar"])
                );
            }
            "list" => {
                let wants: Vec<&str> = case["snaps"]
                    .as_array()
                    .expect("snaps")
                    .iter()
                    .map(s)
                    .collect();
                assert_eq!(wants, SNAPS, "SNAPS");
                assert_eq!(s(&case["default"]), DEFAULT_SNAP, "DEFAULT_SNAP");
                let pics = case["pictures"].as_array().expect("pictures");
                assert_eq!(pics.len(), SNAP_LIST.len(), "SNAP_LIST length");
                for (k, (snap, what)) in SNAP_LIST.iter().enumerate() {
                    assert_eq!(*snap, wants[k], "SNAP_LIST[{k}] name");
                    let want = &pics[k];
                    match what {
                        None => assert!(want.is_null(), "{snap}: should have no picture"),
                        Some(n) => {
                            assert_eq!(i(&want[0]), n.note, "{snap}: note");
                            assert_eq!(i(&want[1]), n.dots, "{snap}: dots");
                            assert_eq!(want[2].as_bool(), Some(n.triplet), "{snap}: triplet");
                        }
                    }
                }
            }
            other => panic!("unknown vector fn {other}"),
        }
    }
    assert!(cases.len() >= 50, "too few snap cases: {}", cases.len());
}
