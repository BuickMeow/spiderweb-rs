//! files/safefile.py: atomic writes and temp file cleanup.

use spiderweb_io::safefile;

#[test]
fn writes_and_replaces() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    safefile::write_text(&path, "hello").expect("first write");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "hello");
    assert!(
        !safefile::tmp_path(&path).exists(),
        "temporary file should be gone"
    );

    safefile::write_text(&path, "world").expect("second write");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "world");
    assert!(
        !safefile::tmp_path(&path).exists(),
        "temporary file should be gone"
    );

    safefile::write_bytes(&path, &[0, 1, 2, 255]).expect("write bytes");
    assert_eq!(std::fs::read(&path).expect("read"), vec![0, 1, 2, 255]);
}

#[test]
fn missing_directory_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nope").join("file.txt");
    assert!(safefile::write_text(&path, "x").is_err());
    assert!(!path.exists());
}

#[test]
fn utf8_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cn.txt");
    safefile::write_text(&path, "中文名字").expect("write");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "中文名字");
}

#[test]
fn write_over_a_directory_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("sub");
    std::fs::create_dir(&target).expect("create dir");
    assert!(safefile::write_text(&target, "x").is_err());
}
