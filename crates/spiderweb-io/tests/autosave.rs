//! Rotation semantics of autosave and autosave-backup (files/project.py load_autosave / restore_window).

use serde_json::Value;
use spiderweb_io::project::{
    AutosaveOutcome, Project, backup_path, load_autosave, read_autosave_window,
};

fn project(name: &str) -> Project {
    let mut p = Project::default();
    p.shapes.push(
        spiderweb_io::compat::shape_from_json(&serde_json::json!({
            "kind": "line", "pts": [[0, 60], [4, 72]], "vel0": 99
        }))
        .expect("形状解析")
        .expect("形状"),
    );
    p.output = name.to_string();
    p
}

#[test]
fn backup_path_names() {
    use std::path::Path;
    for (path, want) in [
        ("autosave.json", "autosave-backup.json"),
        ("a.b.json", "a.b-backup.json"),
        ("noext", "noext-backup.json"),
        (".hidden", ".hidden-backup.json"),
        ("a.", "a-backup.json"),
        ("dir/x.json", "dir/x-backup.json"),
        ("dir/.x.y", "dir/.x-backup.json"),
    ] {
        assert_eq!(
            backup_path(Path::new(path)),
            Path::new(want),
            "backup_path({path:?})"
        );
    }
}

#[test]
fn missing_autosave() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    assert!(matches!(
        load_autosave(&path, "stamp"),
        AutosaveOutcome::Missing
    ));
}

#[test]
fn good_autosave_becomes_the_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    let p = project("first");
    p.write(&path, None).expect("写 autosave");

    match load_autosave(&path, "stamp") {
        AutosaveOutcome::Opened(loaded) => assert_eq!(*loaded, p),
        other => panic!("应当打开: {other:?}"),
    }
    // this launch's autosave was copied to the backup
    let backup = backup_path(&path);
    assert!(backup.exists(), "备份应当存在");
    assert_eq!(Project::load(&backup).expect("读备份"), p);
    // autosave itself was left alone
    assert_eq!(Project::load(&path).expect("读 autosave"), p);
}

#[test]
fn damaged_autosave_is_kept_aside_and_the_backup_opens() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    let backup = backup_path(&path);
    let p = project("backup");
    p.write(&backup, None).expect("写备份");
    std::fs::write(&path, "{ 坏掉的 JSON").expect("写坏 autosave");

    match load_autosave(&path, "autosave-broken-20240928-120000") {
        AutosaveOutcome::Damaged {
            renamed_to,
            backup: got,
        } => {
            let renamed = renamed_to.expect("应当改名留档");
            assert!(renamed.exists(), "留档文件应当存在");
            assert!(
                renamed
                    .to_string_lossy()
                    .contains("autosave-broken-20240928-120000")
            );
            assert!(!path.exists(), "坏 autosave 应当被移走");
            assert_eq!(*got.expect("备份应当能打开"), p);
        }
        other => panic!("应当是损坏流程: {other:?}"),
    }

    // Corrupt it again with the same timestamp: the previous file must not be overwritten
    std::fs::write(&path, "又坏了").expect("写坏 autosave");
    match load_autosave(&path, "autosave-broken-20240928-120000") {
        AutosaveOutcome::Damaged { renamed_to, .. } => {
            let renamed = renamed_to.expect("应当改名留档");
            assert!(
                renamed.to_string_lossy().ends_with("-2.json"),
                "第二份要带 -2: {renamed:?}"
            );
        }
        other => panic!("应当是损坏流程: {other:?}"),
    }
}

#[test]
fn damaged_autosave_without_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    std::fs::write(&path, "坏").expect("写坏 autosave");
    match load_autosave(&path, "stamp") {
        AutosaveOutcome::Damaged { renamed_to, backup } => {
            assert!(renamed_to.is_some());
            assert!(backup.is_none());
        }
        other => panic!("应当是损坏流程: {other:?}"),
    }
}

#[test]
fn window_state_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    let text = r#"{
        "window": {"geometry": "1200x800+10+20", "maximized": true, "velocity": true,
                   "velocity_height": 175.5, "midi_device": "IAC Driver", "live": false,
                   "welcome_tip": true},
        "shapes": []
    }"#;
    std::fs::write(&path, text).expect("写");
    let win = read_autosave_window(&path).expect("应当有窗口");
    assert_eq!(win.geometry, "1200x800+10+20");
    assert!(win.maximized && win.velocity);
    assert_eq!(win.velocity_height, Some(175.5));
    assert_eq!(win.midi_device, "IAC Driver");
    assert!(!win.live);
    assert_eq!(win.rest.get("welcome_tip"), Some(&Value::Bool(true)));

    // when the file cannot be read, fall back to the backup
    std::fs::write(&path, "坏").expect("写坏");
    let backup = backup_path(&path);
    let backup_project = Project::default();
    backup_project.write(&backup, None).expect("写备份");
    assert!(read_autosave_window(&path).is_none(), "备份里没有 window");
}
