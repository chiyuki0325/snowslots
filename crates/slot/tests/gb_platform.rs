mod common;

use slot::app::App;
use slot::persist;
use slot::video_mode::VideoMode;
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, Core, Platform, SlotState, StateRing};

#[test]
fn identical_stems_keep_battery_and_resume_bytes_on_their_own_platform() {
    let d = common::tmp_root_with_carts(&["Tetris"]);
    for platform in [Platform::Gb, Platform::Gbc] {
        let save = platform.dir_name().as_bytes();
        persist::flush_for_platform(
            d.path(),
            platform,
            Core::Mgba,
            "Tetris",
            Some(save),
            Some(save),
        )
        .unwrap();
    }
    assert_eq!(persist::read_sav(d.path(), "Tetris"), None);
    for platform in [Platform::Gb, Platform::Gbc] {
        let expected = platform.dir_name().as_bytes();
        assert_eq!(
            persist::read_sav_for_platform(d.path(), platform, "Tetris").as_deref(),
            Some(expected)
        );
        assert_eq!(
            persist::read_resume_for_platform(d.path(), platform, Core::Mgba, "Tetris").as_deref(),
            Some(expected)
        );
        assert!(
            StateRing::for_platform(d.path(), platform, Core::Mgba, "Tetris")
                .read_resume()
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn game_boy_shoulders_change_the_picture_without_reaching_the_game() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("Games/GB")).unwrap();
    std::fs::create_dir_all(d.path().join("System")).unwrap();
    std::fs::write(d.path().join("Games/GB/Tetris.gb"), vec![0; 0x150]).unwrap();
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Tetris".into()),
            cart_platform: Some(Platform::Gb),
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut app = App::boot(d.path());
    app.on_core_ready();
    app.update(1.0);
    assert_eq!(app.taken_buttons(), &[Btn::L1, Btn::R1]);
    app.apply(Action::GbaDown(Btn::L1));
    assert!(app.takes_from_the_game(Action::GbaDown(Btn::L1)));
    assert_eq!(
        app.source_rect(),
        [40.0 / 240.0, 8.0 / 160.0, 160.0 / 240.0, 144.0 / 160.0]
    );
    assert_eq!(
        slot::video_mode::video_mode_for(d.path(), Platform::Gb, "Tetris"),
        VideoMode::Stretch
    );
    app.apply(Action::GbaDown(Btn::R1));
    assert_eq!(app.source_rect(), slot_gfx::WHOLE_TEXTURE);
}

#[test]
fn gpsp_selection_cannot_open_a_game_boy_cart() {
    let d = common::tmp_root_with_carts(&["Tetris"]);
    slot_store::write_selected_core(d.path(), "Tetris", Core::Gpsp).unwrap();
    assert_eq!(
        slot_store::core_for_platform(d.path(), "Tetris", Platform::Gba),
        Core::Gpsp
    );
    assert_eq!(
        slot_store::core_for_platform(d.path(), "Tetris", Platform::Gb),
        Core::Mgba
    );
    assert_eq!(
        slot_store::core_for_platform(d.path(), "Tetris", Platform::Gbc),
        Core::Mgba
    );
}
