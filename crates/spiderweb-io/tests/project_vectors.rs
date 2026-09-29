//! files/project.py 对照测试（向量由 tools/gen_io_vectors.py 生成）。
//!
//! 向量是 Python `project_json(project_data)` 存出的工程文本与按 `load_file` 语义算出的
//! 期望字段。Rust 侧：解析 -> 逐字段深比较；再序列化 -> Python 版式仍是同一份数据。

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::project::Project;

#[test]
fn project_load_vectors() {
    let cases = cases("project", "cases");
    for case in &cases {
        let name = s(&case["name"]);
        if let Some(err) = case.get("error").and_then(Value::as_str) {
            panic!("向量 {name} 生成失败: {err}");
        }
        let saved = s(&case["saved"]);
        let data: Value = serde_json::from_str(saved).expect("saved 不是 JSON");
        let project =
            Project::from_json(&data).unwrap_or_else(|e| panic!("{name}: 工程解析失败: {e}"));
        let got = project.to_json();
        assert_json_eq(
            &got,
            &project_want(&case["expected"]),
            &format!("{name}: load"),
        );

        // Python 版式 -> 合法 JSON -> 与 to_json 语义一致（Python 能读回）
        let text = project.to_project_json();
        let reparsed: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name}: to_project_json 不是 JSON: {e}\n{text}"));
        assert_json_eq(&reparsed, &got, &format!("{name}: to_project_json"));

        // 原子写盘再读回完全一致
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("project.json");
        project.write(&path, None).expect("写工程");
        let loaded = Project::load(&path).expect("读工程");
        assert_eq!(loaded, project, "{name}: 写盘再读回");
    }
}

#[test]
fn keys_round_trip() {
    // 1.2.0 的 256 键设置：256 存得进读得出；没有 / 别的值一律 128
    let p = Project::from_json(&serde_json::json!({"keys": 256})).expect("读工程");
    assert_eq!(p.keys, 256);
    assert_eq!(p.to_json()["keys"], serde_json::json!(256));
    let text = p.to_project_json();
    let again = Project::from_json(&serde_json::from_str(&text).expect("JSON")).expect("读回");
    assert_eq!(again.keys, 256);
    assert_eq!(
        Project::from_json(&serde_json::json!({}))
            .expect("读工程")
            .keys,
        128
    );
    assert_eq!(
        Project::from_json(&serde_json::json!({"keys": "256"}))
            .expect("读工程")
            .keys,
        128
    );
}

#[test]
fn read_project_settings() {
    // PPQ / BPM / 拍数是文本框，可以是算式（window/app.py read_project）
    let mut p = Project {
        ppq: "960*2".to_string(),
        bpm: "60+60".to_string(),
        beats: "3+1".to_string(),
        ..Project::default()
    };
    assert_eq!(p.read_project().expect("求值"), (1920, 120.0, 4));
    p.ppq = "0".to_string();
    assert_eq!(
        p.read_project().expect_err("PPQ 越界").to_string(),
        "PPQ must be a whole number from 1 to 65535"
    );
    p.ppq = "960".to_string();
    p.bpm = "1".to_string();
    assert_eq!(
        p.read_project().expect_err("BPM 太小").to_string(),
        "BPM must be a number, at least 4"
    );
    p.bpm = "120".to_string();
    p.beats = "33".to_string();
    assert_eq!(
        p.read_project().expect_err("拍数越界").to_string(),
        "Beats per bar must be a whole number from 1 to 32"
    );
}

#[test]
fn project_error_vectors() {
    // 坏 shapes / defaults 让整个工程读不开（Python load_file 返回 False）
    for bad in [
        serde_json::json!({"shapes": null}),
        serde_json::json!({"shapes": 5}),
        serde_json::json!({"defaults": [1]}),
        serde_json::json!({"defaults": {"vel0": "abc"}}),
        serde_json::json!({"shapes": [{"kind": "line", "pts": [[0]]}]}),
    ] {
        assert!(Project::from_json(&bad).is_err(), "坏工程应当报错: {bad}");
    }
    // 形状种类不支持只是跳过，不是读不开
    let p = Project::from_json(&serde_json::json!({
        "shapes": [{"kind": "blob", "pts": [[0, 0], [1, 1]]}, {"kind": "line", "pts": [[0, 0], [1, 1]]}]
    }))
    .expect("应当读开");
    assert_eq!(p.shapes.len(), 1);
}
