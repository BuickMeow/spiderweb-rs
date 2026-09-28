//! files/safefile.py：原子写与临时文件清理。

use spiderweb_io::safefile;

#[test]
fn writes_and_replaces() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    safefile::write_text(&path, "hello").expect("第一次写");
    assert_eq!(std::fs::read_to_string(&path).expect("读"), "hello");
    assert!(!safefile::tmp_path(&path).exists(), "临时文件应当没了");

    safefile::write_text(&path, "world").expect("第二次写");
    assert_eq!(std::fs::read_to_string(&path).expect("读"), "world");
    assert!(!safefile::tmp_path(&path).exists(), "临时文件应当没了");

    safefile::write_bytes(&path, &[0, 1, 2, 255]).expect("写字节");
    assert_eq!(std::fs::read(&path).expect("读"), vec![0, 1, 2, 255]);
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
    safefile::write_text(&path, "中文名字").expect("写");
    assert_eq!(std::fs::read_to_string(&path).expect("读"), "中文名字");
}

#[test]
fn write_over_a_directory_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("sub");
    std::fs::create_dir(&target).expect("建目录");
    assert!(safefile::write_text(&target, "x").is_err());
}
