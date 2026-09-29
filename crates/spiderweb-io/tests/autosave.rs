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
        .expect("parse shape")
        .expect("shape"),
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
    p.write(&path, None).expect("write autosave");

    match load_autosave(&path, "stamp") {
        AutosaveOutcome::Opened(loaded) => assert_eq!(*loaded, p),
        other => panic!("should open: {other:?}"),
    }
    // this launch's autosave was copied to the backup
    let backup = backup_path(&path);
    assert!(backup.exists(), "backup should exist");
    assert_eq!(Project::load(&backup).expect("read backup"), p);
    // autosave itself was left alone
    assert_eq!(Project::load(&path).expect("read autosave"), p);
}

#[test]
fn damaged_autosave_is_kept_aside_and_the_backup_opens() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    let backup = backup_path(&path);
    let p = project("backup");
    p.write(&backup, None).expect("write backup");
    std::fs::write(&path, "{ broken JSON").expect("write broken autosave");

    match load_autosave(&path, "autosave-broken-20240928-120000") {
        AutosaveOutcome::Damaged {
            renamed_to,
            backup: got,
        } => {
            let renamed = renamed_to.expect("should be renamed for the record");
            assert!(renamed.exists(), "broken file should exist");
            assert!(
                renamed
                    .to_string_lossy()
                    .contains("autosave-broken-20240928-120000")
            );
            assert!(!path.exists(), "broken autosave should be moved away");
            assert_eq!(*got.expect("backup should open"), p);
        }
        other => panic!("should be the damaged path: {other:?}"),
    }

    // Corrupt it again with the same timestamp: the previous file must not be overwritten
    std::fs::write(&path, "broken again").expect("write broken autosave");
    match load_autosave(&path, "autosave-broken-20240928-120000") {
        AutosaveOutcome::Damaged { renamed_to, .. } => {
            let renamed = renamed_to.expect("should be renamed for the record");
            assert!(
                renamed.to_string_lossy().ends_with("-2.json"),
                "second copy should have -2: {renamed:?}"
            );
        }
        other => panic!("should be the damaged path: {other:?}"),
    }
}

#[test]
fn damaged_autosave_without_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("autosave.json");
    std::fs::write(&path, "broken").expect("write broken autosave");
    match load_autosave(&path, "stamp") {
        AutosaveOutcome::Damaged { renamed_to, backup } => {
            assert!(renamed_to.is_some());
            assert!(backup.is_none());
        }
        other => panic!("should be the damaged path: {other:?}"),
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
    std::fs::write(&path, text).expect("write");
    let win = read_autosave_window(&path).expect("should have a window");
    assert_eq!(win.geometry, "1200x800+10+20");
    assert!(win.maximized && win.velocity);
    assert_eq!(win.velocity_height, Some(175.5));
    assert_eq!(win.midi_device, "IAC Driver");
    assert!(!win.live);
    assert_eq!(win.rest.get("welcome_tip"), Some(&Value::Bool(true)));

    // when the file cannot be read, fall back to the backup
    std::fs::write(&path, "broken").expect("write broken");
    let backup = backup_path(&path);
    let backup_project = Project::default();
    backup_project.write(&backup, None).expect("write backup");
    assert!(
        read_autosave_window(&path).is_none(),
        "backup has no window"
    );
}
