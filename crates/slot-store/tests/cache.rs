use slot_store::{refresh, scan_fast};

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Games/GBA")).unwrap();
    std::fs::create_dir_all(root.path().join("Labels/GBA")).unwrap();
    std::fs::create_dir_all(root.path().join("System")).unwrap();
    root
}

fn write_rom(root: &tempfile::TempDir, stem: &str, title: &str) {
    let mut bytes = vec![0; 0x100];
    bytes[0xa0..0xa0 + title.len()].copy_from_slice(title.as_bytes());
    std::fs::write(root.path().join(format!("Games/GBA/{stem}.gba")), bytes).unwrap();
}

#[test]
fn a_second_boot_reads_the_last_complete_library() {
    let root = root();
    write_rom(&root, "Alpha", "FIRST");
    let first = scan_fast(root.path()).unwrap();
    assert!(!first.from_cache);
    assert_eq!(first.carts[0].title, "FIRST");

    write_rom(&root, "Alpha", "SECOND");
    let cached = scan_fast(root.path()).unwrap();
    assert!(cached.from_cache);
    assert_eq!(cached.carts[0].title, "FIRST");

    let refreshed = refresh(root.path()).unwrap();
    assert_eq!(refreshed[0].title, "SECOND");
    assert_eq!(scan_fast(root.path()).unwrap().carts[0].title, "SECOND");
}

#[test]
fn refresh_reconciles_added_and_removed_roms() {
    let root = root();
    write_rom(&root, "Alpha", "ALPHA");
    scan_fast(root.path()).unwrap();

    std::fs::remove_file(root.path().join("Games/GBA/Alpha.gba")).unwrap();
    write_rom(&root, "Bravo", "BRAVO");
    let refreshed = refresh(root.path()).unwrap();
    assert_eq!(
        refreshed
            .iter()
            .map(|c| c.stem.as_str())
            .collect::<Vec<_>>(),
        ["Bravo"]
    );
}

#[test]
fn a_damaged_cache_falls_back_to_a_full_scan() {
    let root = root();
    write_rom(&root, "Alpha", "ALPHA");
    std::fs::write(root.path().join("System/library-cache.bin"), b"broken").unwrap();

    let scanned = scan_fast(root.path()).unwrap();
    assert!(!scanned.from_cache);
    assert_eq!(scanned.carts[0].stem, "Alpha");
}
