#![cfg(unix)]

use slot::persist;
use slot_store::{Core, Platform};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

fn legacy_name(extension: &str) -> OsString {
    let mut bytes = vec![0xb1, 0xa6, 0xbf, 0xc9, 0xc3, 0xce];
    bytes.extend_from_slice(extension.as_bytes());
    OsString::from_vec(bytes)
}

#[test]
fn battery_reads_and_writes_reuse_the_original_gb18030_save() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Saves/GBA");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(legacy_name(".sav"));
    std::fs::write(&file, b"old battery").unwrap();
    assert_eq!(
        persist::read_sav(root.path(), Platform::Gba, "宝可梦").unwrap(),
        Some(b"old battery".to_vec())
    );
    assert!(persist::write_sav(root.path(), Platform::Gba, "宝可梦", b"new battery").unwrap());
    assert_eq!(std::fs::read(file).unwrap(), b"new battery");
    assert!(!dir.join("宝可梦.sav").exists());
}

#[test]
fn a_gb18030_srm_is_loaded_when_there_is_no_sav() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Saves/GBC");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(legacy_name(".srm")), b"battery").unwrap();
    assert_eq!(
        persist::read_sav(root.path(), Platform::Gbc, "宝可梦").unwrap(),
        Some(b"battery".to_vec())
    );
}

#[test]
fn flushing_reuses_the_original_gb18030_resume_directory() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("States/GBA/mgba").join(legacy_name(""));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("resume.state"), b"old state").unwrap();
    assert_eq!(
        persist::read_resume(root.path(), Platform::Gba, Core::Mgba, "宝可梦").unwrap(),
        Some(b"old state".to_vec())
    );
    persist::flush(
        root.path(),
        Platform::Gba,
        Core::Mgba,
        "宝可梦",
        Some(b"new state"),
        None,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(dir.join("resume.state")).unwrap(),
        b"new state"
    );
    assert!(!root.path().join("States/GBA/mgba/宝可梦").exists());
}
