mod common;

use slot_store::{
    library_cache_path, load_library, read_library_cache, scan, scan_report, write_library_cache,
};

fn game(root: &std::path::Path, name: &str) {
    std::fs::write(root.join("Games/GBA").join(name), [0; 256]).unwrap();
}

#[test]
fn cached_listing_matches_a_fresh_scan_including_labels_and_tags() {
    let root = common::tmp_root();
    game(root.path(), "游戏 (USA).gba");
    game(root.path(), "游戏 (Japan).gba");
    std::fs::create_dir_all(root.path().join("Labels/GBA")).unwrap();
    std::fs::write(root.path().join("Labels/GBA/游戏 (USA).png"), []).unwrap();
    let carts = scan(root.path()).unwrap();
    write_library_cache(root.path(), &carts).unwrap();
    assert_eq!(read_library_cache(root.path()).unwrap(), Some(carts));
}

#[test]
fn a_cache_hit_does_not_rescan_the_game_directory() {
    let root = common::tmp_root();
    game(root.path(), "Old.gba");
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    std::fs::remove_file(root.path().join("Games/GBA/Old.gba")).unwrap();
    game(root.path(), "New.gba");
    let loaded = load_library(root.path());
    assert!(loaded.from_cache);
    assert_eq!(loaded.carts[0].stem, "Old");
    assert_eq!(scan(root.path()).unwrap()[0].stem, "New");
}

#[test]
fn a_cache_miss_returns_the_complete_listing_without_writing_on_the_caller() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    let loaded = load_library(root.path());
    assert!(!loaded.from_cache);
    assert!(loaded.complete);
    assert_eq!(loaded.carts.len(), 1);
    assert!(!library_cache_path(root.path()).exists());
}

#[test]
fn a_cache_hit_skips_the_legacy_name_sweep() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let loaded = slot_store::load_library_with(root.path(), || panic!("swept cached names"));
    assert!(loaded.from_cache);
}

#[test]
fn a_broken_cache_is_deleted_before_scanning() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    std::fs::create_dir_all(root.path().join("Cache")).unwrap();
    std::fs::write(library_cache_path(root.path()), b"broken").unwrap();
    let loaded = load_library(root.path());
    assert!(!loaded.from_cache);
    assert_eq!(loaded.carts.len(), 1);
    assert!(!library_cache_path(root.path()).exists());
}

#[test]
fn the_old_cache_is_deleted_without_touching_saves() {
    let root = common::tmp_root();
    let old = root.path().join("System/library-cache.bin");
    let save = root.path().join("Saves/Game.sav");
    std::fs::write(&old, b"old cache").unwrap();
    std::fs::write(&save, b"save").unwrap();
    load_library(root.path());
    assert!(!old.exists());
    assert_eq!(std::fs::read(save).unwrap(), b"save");
}

#[test]
fn unknown_versions_and_trailing_data_are_discarded() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    let carts = scan(root.path()).unwrap();
    for change_version in [true, false] {
        write_library_cache(root.path(), &carts).unwrap();
        let mut bytes = std::fs::read(library_cache_path(root.path())).unwrap();
        if change_version {
            bytes[7] = 255;
        } else {
            bytes.push(0);
        }
        std::fs::write(library_cache_path(root.path()), bytes).unwrap();
        assert!(!load_library(root.path()).from_cache);
        assert!(!library_cache_path(root.path()).exists());
    }
}

#[test]
fn a_bad_folder_returns_readable_games_but_is_not_a_complete_scan() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    std::fs::remove_dir(root.path().join("Games/GB")).unwrap();
    std::fs::write(root.path().join("Games/GB"), b"not a directory").unwrap();
    let scanned = scan_report(root.path());
    assert!(!scanned.complete);
    assert_eq!(scanned.carts.len(), 1);
    assert!(!load_library(root.path()).complete);
}

#[test]
fn a_bad_labels_folder_also_prevents_caching_a_partial_scan() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    std::fs::write(root.path().join("Labels/GBA"), b"not a directory").unwrap();
    let scanned = scan_report(root.path());
    assert!(!scanned.complete);
    assert_eq!(scanned.carts.len(), 1);
}

#[test]
fn cached_games_use_current_shell_settings() {
    let root = common::tmp_root();
    game(root.path(), "Game.gba");
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    std::fs::write(
        root.path().join(slot_store::CART_SHELL_FILE),
        "Game = rounded 112233 clear\n",
    )
    .unwrap();
    let carts = read_library_cache(root.path()).unwrap().unwrap();
    assert_eq!(carts, scan(root.path()).unwrap());
    assert_eq!(
        carts[0].shell,
        slot_store::ShellChoice::parse("rounded 112233 clear")
    );
}

#[cfg(unix)]
#[test]
fn raw_gb18030_filenames_survive_the_cache_round_trip() {
    use std::os::unix::ffi::OsStringExt;
    let root = common::tmp_root();
    let (encoded, _, _) = encoding_rs::GB18030.encode("中文游戏.gba");
    let name = std::ffi::OsString::from_vec(encoded.into_owned());
    std::fs::write(root.path().join("Games/GBA").join(&name), []).unwrap();
    let carts = scan(root.path()).unwrap();
    write_library_cache(root.path(), &carts).unwrap();
    let loaded = read_library_cache(root.path()).unwrap().unwrap();
    assert_eq!(loaded, carts);
    assert_eq!(loaded[0].rom.file_name(), Some(name.as_os_str()));
    assert_eq!(loaded[0].stem, "中文游戏");
}

#[test]
fn a_cache_cannot_escape_the_game_directory() {
    let root = common::tmp_root();
    game(root.path(), "abc.gba");
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let path = library_cache_path(root.path());
    let mut bytes = std::fs::read(&path).unwrap();
    let offset = bytes
        .windows(7)
        .position(|part| part == b"abc.gba")
        .unwrap();
    bytes[offset..offset + 7].copy_from_slice(b"../.gba");
    std::fs::write(&path, bytes).unwrap();
    assert!(read_library_cache(root.path()).is_err());
    assert!(!load_library(root.path()).from_cache);
    assert!(!path.exists());
}
