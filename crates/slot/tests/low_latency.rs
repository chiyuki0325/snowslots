mod common;

use std::time::{Duration, Instant};

use slot::app::Phase;
use slot::emu::FrameOutcome;
use slot::session::Session;
use slot_input::{Action, Btn, RawEvent};
use slot_store::{write_slot_state, SlotState};

const WAIT: Duration = Duration::from_secs(2);

fn seated(device: bool) -> (tempfile::TempDir, Session) {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        root.path(),
        &SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            low_latency: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut session = Session::boot(root.path().to_path_buf());
    session.set_display_paced(device);
    let deadline = Instant::now() + WAIT;
    let mut now = 0;
    while !matches!(session.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "cart never seated");
        now += 16;
        session.feed([], now);
        session.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    (root, session)
}

#[test]
fn a_device_session_configures_new_cores_before_they_start_and_requests_after_input() {
    let (_root, mut s) = seated(true);
    assert_eq!(
        s.frames_published(),
        0,
        "new core ran before the first display request"
    );
    s.feed([RawEvent::Down(Btn::A)], 2000);
    s.update(1.0 / 60.0);
    assert_eq!(s.emu().unwrap().input().0, slot_retro::ButtonMask::A);
    assert_eq!(
        s.request_frame_until(Instant::now() + WAIT),
        FrameOutcome::Published
    );
    assert!(s.frame().is_some());
    assert_eq!(s.frames_published(), 1);
    s.app_mut().apply(Action::PowerHold);
    s.update(1.0 / 60.0);
    assert_eq!(
        s.request_frame_until(Instant::now() + WAIT),
        FrameOutcome::NoFrame
    );
    s.app_mut().apply(Action::GbaDown(Btn::B));
    s.update(1.0 / 60.0);
    assert_eq!(
        s.request_frame_until(Instant::now() + WAIT),
        FrameOutcome::Published
    );
}

#[test]
fn a_saved_experiment_does_not_change_the_host_clock() {
    let (_root, s) = seated(false);
    assert!(s.app().low_latency());
    let deadline = Instant::now() + WAIT;
    while s.frames_published() < 3 {
        assert!(
            Instant::now() < deadline,
            "host started waiting for display requests"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        s.request_frame_until(Instant::now() + WAIT),
        FrameOutcome::NoFrame
    );
}

#[test]
fn an_empty_slot_does_not_wait_for_a_core() {
    let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        root.path(),
        &SlotState {
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut s = Session::boot(root.path().to_path_buf());
    s.set_display_paced(true);
    assert_eq!(
        s.request_frame_until(Instant::now() + WAIT),
        FrameOutcome::NoFrame
    );
}
