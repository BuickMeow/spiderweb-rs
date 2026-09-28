//! paths.py 对照测试（向量由 tools/gen_paths_vectors.py 生成）。

mod common;

use common::*;
use spiderweb_core::paths as P;

#[test]
fn paths_vectors() {
    let mut checked = 0;
    for (idx, case) in cases("paths").iter().enumerate() {
        let fname = case["fn"].as_str().unwrap();
        let args = case["args"].as_array().unwrap();
        let out = &case["out"];
        let ctx = format!("#{idx} {fname}");
        match fname {
            "dedupe" => {
                let g = P::dedupe(&pts(&args[0]));
                assert_pts_eq(&g, out, &ctx);
            }
            "direction_changes" => {
                let g = P::direction_changes(&floats(&args[0]));
                let w: Vec<usize> = out
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                assert_eq!(g, w, "{ctx}");
            }
            "spans" => {
                let lo: Vec<usize> = args[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                let hi: Vec<usize> = args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                let rev: Option<Vec<bool>> = if args[2].is_null() {
                    None
                } else {
                    Some(args[2].as_array().unwrap().iter().map(b).collect())
                };
                let g = P::spans(&lo, &hi, rev.as_deref());
                let w: Vec<usize> = out
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                assert_eq!(g, w, "{ctx}");
            }
            "stretch_ends" => {
                let g = P::stretch_ends(&pts(&args[0]), b(&args[1]));
                assert_pts_eq(&g, out, &ctx);
            }
            "keep_longest" => {
                let g = P::keep_longest(&rows3(&args[0]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "loop_from_left" => {
                let g = P::loop_from_left(&pts(&args[0]));
                assert_pts_eq(&g, out, &ctx);
            }
            "ends_forward" => {
                let g = P::ends_forward(&pts(&args[0]));
                assert_eq!(g, b(out), "{ctx}");
            }
            "line_notes" => {
                let g = P::line_notes(&pts(&args[0]), b(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "parts_notes" => {
                let first: Vec<usize> = args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                let tails: Vec<bool> = args[2].as_array().unwrap().iter().map(b).collect();
                let (notes, per) = P::parts_notes(&pts(&args[0]), &first, &tails, b(&args[3]));
                assert_rows3_eq(&notes, &out[0], &ctx);
                let wper: Vec<usize> = out[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                assert_eq!(per, wper, "{ctx} per");
            }
            "path_notes" => {
                let g = P::path_notes(&pts(&args[0]), b(&args[1]));
                assert_rows3_eq(&g, out, &ctx);
            }
            "dot_segment_notes" => {
                let g = P::dot_segment_notes(&pts(&args[0]));
                assert_rows3_eq(&g, out, &ctx);
            }
            other => panic!("未知用例 {other}"),
        }
        checked += 1;
    }
    assert!(checked > 30, "用例太少：{checked}");
}
