mod common;

use slot_store::{find_file_by_stem, scan, Core, Platform, StateRing};

#[test]
fn chinese_and_latin_names_share_pinyin_order() {
    let mut names = [
        "Zelda",
        "宝可梦",
        "阿童木",
        "Batman",
        "1943",
        "[BIOS]",
        "马里奥",
    ];
    names.sort_by_cached_key(|name| slot_store::sort_key(name));
    assert_eq!(
        names,
        [
            "1943",
            "阿童木",
            "宝可梦",
            "Batman",
            "马里奥",
            "Zelda",
            "[BIOS]"
        ]
    );
    assert_eq!(slot_store::initial(" 宝可梦"), 'B');
    assert_eq!(slot_store::initial("马里奥"), 'M');
    assert_eq!(slot_store::initial(""), '#');
}

#[test]
fn homophones_have_a_stable_order_and_leading_spaces_are_ignored() {
    assert_ne!(slot_store::sort_key("宝"), slot_store::sort_key("保"));
    assert!(slot_store::sort_key("  阿童木") < slot_store::sort_key("宝可梦"));
}

#[test]
fn cached_and_scanned_games_use_the_same_pinyin_order() {
    let root = common::tmp_root();
    for name in ["马里奥", "宝可梦", "阿童木", "Zelda"] {
        std::fs::write(root.path().join(format!("Games/GBA/{name}.gba")), b"rom").unwrap();
    }
    let carts = scan(root.path()).unwrap();
    assert_eq!(
        carts
            .iter()
            .map(|cart| cart.stem.as_str())
            .collect::<Vec<_>>(),
        ["阿童木", "宝可梦", "马里奥", "Zelda"]
    );
    let mut reversed = carts.clone();
    reversed.reverse();
    slot_store::write_library_cache(root.path(), &reversed).unwrap();
    assert_eq!(slot_store::load_library(root.path()).carts, carts);
}

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
