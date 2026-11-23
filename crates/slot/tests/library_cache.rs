mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use slot::app::{App, Phase};
use slot_input::Action;
use slot_store::{scan, write_library_cache, write_slot_state, SlotState};

fn boot(root: &Path) -> App {
    write_slot_state(
        root,
        &SlotState {
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let app = App::boot(root);
    assert!(matches!(app.phase(), Phase::Shelf));
    app
}

fn finish_refresh(app: &mut App) {
    let until = Instant::now() + Duration::from_secs(5);
    while app.library_refresh_pending() {
        assert!(Instant::now() < until, "library refresh did not finish");
        app.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_first_boot_scans_before_showing_the_shelf_and_saves_after_presentation() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut app = boot(root.path());
    assert_eq!(app.carts().count(), 2);
    let path = slot_store::library_cache_path(root.path());
    assert!(!path.exists());
    app.library_presented();
    let until = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < until, "the first cache was not written");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        slot_store::read_library_cache(root.path())
            .unwrap()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn the_cached_shelf_is_shown_before_refresh_and_keeps_its_selected_game() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    std::fs::write(root.path().join("Games/GBA/Added.gba"), [0; 256]).unwrap();
    let mut app = boot(root.path());
    let selected = app.selected_stem().unwrap().to_owned();
    for _ in 0..10 {
        app.update(1.0 / 60.0);
    }
    assert_eq!(app.carts().count(), 2);
    assert!(app.library_refresh_pending());
    assert!(!app.take_rescanned());
    app.library_presented();
    finish_refresh(&mut app);
    assert_eq!(app.carts().count(), 3);
    assert_eq!(app.selected_stem(), Some(selected.as_str()));
    assert!(app.take_rescanned());
}

#[test]
fn an_unchanged_library_does_not_rebuild_the_shelf() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let mut app = boot(root.path());
    app.library_presented();
    finish_refresh(&mut app);
    assert!(!app.take_rescanned());
}

#[test]
fn name_sweep_keeps_working_for_both_cached_and_uncached_libraries() {
    for cached in [false, true] {
        let root = common::tmp_root_with_carts(&[" Emerald ", "Fusion"]);
        if cached {
            write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
        }
        let app = boot(root.path());
        assert!(app.carts().any(|cart| cart.stem == "Emerald"));
        assert!(root.path().join("Games/GBA/Emerald.gba").is_file());
        assert!(!root.path().join("Games/GBA/ Emerald .gba").exists());
    }
}

#[test]
fn a_game_in_progress_delays_refresh_until_the_shelf_returns() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    std::fs::write(root.path().join("Games/GBA/Added.gba"), [0; 256]).unwrap();
    let mut app = boot(root.path());
    app.apply(Action::Insert);
    app.library_presented();
    for _ in 0..100 {
        app.update(1.0 / 60.0);
    }
    assert_eq!(app.carts().count(), 2);
    assert!(app.library_refresh_pending());
    assert!(!app.take_rescanned());
    app.apply(Action::Eject);
    finish_refresh(&mut app);
    assert!(matches!(app.phase(), Phase::Shelf));
    assert_eq!(app.carts().count(), 3);
}

#[test]
fn a_bad_scan_keeps_the_current_shelf_and_deletes_the_cache() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let mut app = boot(root.path());
    std::fs::create_dir_all(root.path().join("Games/GB")).unwrap();
    std::fs::remove_dir(root.path().join("Games/GB")).unwrap();
    std::fs::write(root.path().join("Games/GB"), b"not a directory").unwrap();
    app.library_presented();
    finish_refresh(&mut app);
    assert_eq!(app.carts().count(), 2);
    assert!(!app.take_rescanned());
    assert!(!slot_store::library_cache_path(root.path()).exists());
}

#[test]
fn a_deleted_cached_game_is_refused_and_then_removed_from_the_shelf() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let mut app = boot(root.path());
    let selected = app.selected_cart().unwrap().clone();
    std::fs::remove_file(&selected.rom).unwrap();
    app.apply(Action::Insert);
    assert!(matches!(app.phase(), Phase::Shelf));
    app.library_presented();
    finish_refresh(&mut app);
    assert!(!app.carts().any(|cart| cart.rom == selected.rom));
}

#[test]
fn changing_a_shell_cannot_be_undone_by_an_older_refresh() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_library_cache(root.path(), &scan(root.path()).unwrap()).unwrap();
    let mut app = boot(root.path());
    app.library_presented();
    app.update(1.0 / 60.0);
    let shell = slot_store::ShellChoice::parse("rounded 112233 clear").unwrap();
    slot_store::cart_shell::write_choice(root.path(), "Emerald", Some(&shell)).unwrap();
    app.set_cart_shell(slot_store::Platform::Gba, "Emerald", Some(shell));
    finish_refresh(&mut app);
    assert_eq!(
        app.carts()
            .find(|cart| cart.stem == "Emerald")
            .unwrap()
            .shell,
        Some(shell)
    );
}
