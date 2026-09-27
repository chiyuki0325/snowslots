use slot_store::{
    read_play_history, scan_fast, write_last_played, write_play_history, Platform, PlayHistory,
};

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Games/GBA")).unwrap();
    std::fs::create_dir_all(root.path().join("Labels/GBA")).unwrap();
    std::fs::create_dir_all(root.path().join("System")).unwrap();
    root
}

fn rom(root: &tempfile::TempDir, stem: &str) {
    let mut bytes = vec![0; 0x100];
    bytes[0xa0..0xa4].copy_from_slice(b"GAME");
    std::fs::write(root.path().join(format!("Games/GBA/{stem}.gba")), bytes).unwrap();
}

#[test]
fn play_history_round_trips_all_carts_in_one_file() {
    let root = root();
    let mut history = PlayHistory::new();
    history.insert((Platform::Gba, "中文 = game".into()), 1234);
    history.insert((Platform::Gba, "Other".into()), 5678);
    write_play_history(root.path(), &history).unwrap();

    assert_eq!(read_play_history(root.path()).unwrap(), history);
}

#[test]
fn updating_one_cart_preserves_the_other_records() {
    let root = root();
    write_last_played(root.path(), Platform::Gba, "Alpha", 10).unwrap();
    write_last_played(root.path(), Platform::Gba, "Bravo", 20).unwrap();
    write_last_played(root.path(), Platform::Gba, "Alpha", 30).unwrap();

    let history = read_play_history(root.path()).unwrap();
    assert_eq!(history[&(Platform::Gba, "Alpha".into())], 30);
    assert_eq!(history[&(Platform::Gba, "Bravo".into())], 20);
}

#[test]
fn scans_merge_history_into_the_cached_library() {
    let root = root();
    rom(&root, "Alpha");
    let first = scan_fast(root.path()).unwrap();
    assert!(!first.from_cache);
    assert_eq!(first.carts[0].last_launched, None);

    write_last_played(root.path(), Platform::Gba, "Alpha", 42).unwrap();
    let cached = scan_fast(root.path()).unwrap();
    assert!(cached.from_cache);
    assert_eq!(cached.carts[0].last_launched, Some(42));
}

#[test]
fn corrupt_history_is_rejected_without_breaking_the_library() {
    let root = root();
    rom(&root, "Alpha");
    std::fs::write(root.path().join("System/play-history.bin"), b"broken").unwrap();

    assert!(read_play_history(root.path()).is_err());
    assert_eq!(scan_fast(root.path()).unwrap().carts.len(), 1);
}
