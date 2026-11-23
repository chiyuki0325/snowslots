mod common;

use slot_store::{find_file_by_stem, scan, Core, Platform, StateRing};

#[test]
fn utf8_names_are_displayed_without_changing_the_rom_path() {
    let root = common::tmp_root();
    let rom = root.path().join("Games/GBA/宝可梦.gba");
    std::fs::write(&rom, b"rom").unwrap();
    let carts = scan(root.path()).unwrap();
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].stem, "宝可梦");
    assert_eq!(carts[0].rom, rom);
}

#[test]
fn latin1_mojibake_is_recovered_only_for_chinese_names() {
    let root = common::tmp_root();
    let mangled: String = "宝可梦"
        .as_bytes()
        .iter()
        .map(|byte| char::from(*byte))
        .collect();
    std::fs::write(root.path().join(format!("Games/GBA/{mangled}.gba")), b"rom").unwrap();
    std::fs::write(root.path().join("Games/GBA/Pokémon.gba"), b"rom").unwrap();
    let carts = scan(root.path()).unwrap();
    assert!(carts.iter().any(|cart| cart.stem == "宝可梦"));
    assert!(carts.iter().any(|cart| cart.stem == "Pokémon"));
}

#[cfg(unix)]
fn legacy_name(extension: &str) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    let mut bytes = vec![0xb1, 0xa6, 0xbf, 0xc9, 0xc3, 0xce];
    bytes.extend_from_slice(extension.as_bytes());
    std::ffi::OsString::from_vec(bytes)
}

#[cfg(unix)]
#[test]
fn gb18030_roms_and_labels_keep_their_original_paths() {
    let root = common::tmp_root();
    for (folder, extension) in [("GBA", ".GBA"), ("GB", ".gb"), ("GBC", ".gbc")] {
        let rom = root
            .path()
            .join("Games")
            .join(folder)
            .join(legacy_name(extension));
        let label = root
            .path()
            .join("Labels")
            .join(folder)
            .join(legacy_name(".PNG"));
        std::fs::create_dir_all(label.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"rom").unwrap();
        std::fs::write(&label, b"png").unwrap();
        let carts = scan(root.path()).unwrap();
        let cart = carts.iter().find(|cart| cart.rom == rom).unwrap();
        assert_eq!(cart.stem, "宝可梦");
        assert_eq!(cart.label.as_ref(), Some(&label));
    }
}

#[cfg(unix)]
#[test]
fn gb18030_save_files_are_found_by_the_displayed_name() {
    let root = common::tmp_root();
    let dir = root.path().join("Saves/GBA");
    std::fs::create_dir_all(&dir).unwrap();
    for extension in [".sav", ".srm"] {
        let path = dir.join(legacy_name(extension));
        std::fs::write(&path, b"battery").unwrap();
        assert_eq!(
            find_file_by_stem(&dir, "宝可梦", &extension[1..]),
            Some(path)
        );
    }
}

#[cfg(unix)]
#[test]
fn resume_reads_and_writes_reuse_the_gb18030_directory() {
    let root = common::tmp_root();
    let dir = root.path().join("States/GBA/mgba").join(legacy_name(""));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("resume.state"), b"old").unwrap();
    let ring = StateRing::new(root.path(), Platform::Gba, Core::Mgba, "宝可梦");
    assert_eq!(ring.read_resume().unwrap().as_deref(), Some(&b"old"[..]));
    ring.write_resume(b"new").unwrap();
    assert_eq!(std::fs::read(dir.join("resume.state")).unwrap(), b"new");
    assert!(!root.path().join("States/GBA/mgba/宝可梦").exists());
}

#[cfg(unix)]
#[test]
fn hidden_gb18030_sidecars_are_not_games() {
    use std::os::unix::ffi::OsStringExt;
    let root = common::tmp_root();
    let mut bytes = b"._".to_vec();
    bytes.extend_from_slice(&legacy_name(".gba").into_vec());
    std::fs::write(
        root.path()
            .join("Games/GBA")
            .join(std::ffi::OsString::from_vec(bytes)),
        b"metadata",
    )
    .unwrap();
    assert!(scan(root.path()).unwrap().is_empty());
}
