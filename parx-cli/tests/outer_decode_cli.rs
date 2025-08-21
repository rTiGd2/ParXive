use assert_cmd::Command;
use assert_fs::fixture::PathChild;
use assert_fs::TempDir;
use std::fs;

#[test]
fn test_outer_decode() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.child("file.txt");

    fs::write(file.path(), "hello world").unwrap();

    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.arg("outer-decode").arg(file.path()).assert().success();

    tmp.close().unwrap();
}
