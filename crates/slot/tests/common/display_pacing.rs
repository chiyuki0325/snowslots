use super::*;
use slot::audio::Ring;
use slot::emu::FrameOutcome;

const WAIT: Duration = Duration::from_secs(2);

/// Stop at a real core boundary rather than guessing how long the worker needs to get there.
pub(super) struct Gate {
    armed: Arc<AtomicBool>,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

struct GateControl {
    armed: Arc<AtomicBool>,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl Gate {
    fn new() -> (Self, GateControl) {
        let armed = Arc::new(AtomicBool::new(false));
        let (entered, seen) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        (
            Self {
                armed: armed.clone(),
                entered,
                release: resume,
            },
            GateControl {
                armed,
                entered: seen,
                release,
            },
        )
    }

    pub(super) fn enter(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.release
                .recv_timeout(WAIT)
                .expect("test did not release the core");
        }
    }
}

impl GateControl {
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    fn entered(&self) {
        self.entered
            .recv_timeout(WAIT)
            .expect("core never reached the gate");
    }

    fn release(&self) {
        self.release.send(()).unwrap();
    }
}

fn display_core(core: Box<dyn RetroCore>) -> EmuHandle {
    let emu = EmuHandle::spawn(
        core,
        PathBuf::from("mock"),
        Arc::new(Ring::new(0)),
        None,
        None,
    );
    emu.set_display_paced(true);
    assert!(wait_for(|| emu.state() == CoreState::Ready));
    emu.set_speed(Speed::Normal);
    assert!(wait_for(|| emu.observed_speed() == Speed::Normal));
    emu
}

fn present(emu: &EmuHandle) -> FrameOutcome {
    emu.request_frame_until(Instant::now() + WAIT)
}

#[test]
fn no_request_means_no_game_frames_and_each_request_uses_its_input() {
    let (mut core, _) = Probe::new(Duration::ZERO);
    let inputs = Arc::new(Mutex::new(Vec::new()));
    core.inputs = Some(inputs.clone());
    let emu = display_core(core);
    assert_eq!(frame_count(&emu), 0);
    std::thread::sleep(Duration::from_millis(45));
    assert_eq!(frame_count(&emu), 0);
    for (i, mask) in [ButtonMask::A, ButtonMask::B, 0].into_iter().enumerate() {
        emu.set_input(ButtonMask(mask), ButtonMask(0));
        assert_eq!(present(&emu), FrameOutcome::Published);
        assert_eq!(frame_count(&emu), i as u64 + 1);
        let frame = emu.latest_frame().expect("completion without a frame");
        let mut reference = MockCore::new();
        reference
            .unserialize(&(i as u64 + 1).to_le_bytes())
            .unwrap();
        assert_eq!(&frame[..], reference.video_xrgb8888());
    }
    assert_eq!(
        inputs.lock().unwrap().as_slice(),
        [
            ButtonMask(ButtonMask::A),
            ButtonMask(ButtonMask::B),
            ButtonMask(0)
        ]
    );
    std::thread::sleep(Duration::from_millis(45));
    assert_eq!(frame_count(&emu), 3);
}

#[test]
fn a_late_frame_cannot_complete_a_new_request_or_build_a_backlog() {
    let (mut core, _) = Probe::new(Duration::ZERO);
    let (gate, control) = Gate::new();
    core.frame_gate = Some(gate);
    let emu = display_core(core);
    control.arm();
    std::thread::scope(|scope| {
        let first =
            scope.spawn(|| emu.request_frame_until(Instant::now() + Duration::from_millis(100)));
        control.entered();
        assert_eq!(first.join().unwrap(), FrameOutcome::TimedOut);
        for _ in 0..20 {
            assert_eq!(present(&emu), FrameOutcome::Busy);
        }
        control.release();
        assert_eq!(frame_count(&emu), 1);
        assert!(emu.frame_ready(), "the late image is still usable");

        control.arm();
        let (done, result) = mpsc::channel();
        let emu = &emu;
        scope.spawn(move || {
            done.send(present(emu)).unwrap();
        });
        control.entered();
        assert!(
            result.try_recv().is_err(),
            "old publication satisfied the new request"
        );
        control.release();
        assert_eq!(result.recv_timeout(WAIT).unwrap(), FrameOutcome::Published);
    });
    assert_eq!(frame_count(&emu), 2);
}

#[test]
fn publication_does_not_wait_for_snapshot_and_expired_pending_work_is_discarded() {
    let (mut core, _) = Probe::new(Duration::ZERO);
    let (gate, control) = Gate::new();
    core.snapshot_gate = Some(gate);
    let emu = display_core(core);
    assert_eq!(present(&emu), FrameOutcome::Published);
    control.arm();
    assert_eq!(present(&emu), FrameOutcome::Published);
    control.entered(); // The second frame's trailing snapshot is still blocked.
    assert_eq!(emu.published_count(), 2);
    assert_eq!(
        emu.request_frame_until(Instant::now() + Duration::from_millis(20)),
        FrameOutcome::TimedOut
    );
    assert_eq!(present(&emu), FrameOutcome::Busy);
    control.release();
    assert_eq!(
        frame_count(&emu),
        2,
        "an expired request ran after the stall"
    );
    assert_eq!(present(&emu), FrameOutcome::Published);
    assert_eq!(frame_count(&emu), 3);
}

#[test]
fn queued_input_is_a_snapshot_and_mode_changes_invalidate_queued_requests() {
    let (mut core, _) = Probe::new(Duration::ZERO);
    let inputs = Arc::new(Mutex::new(Vec::new()));
    core.inputs = Some(inputs.clone());
    let (gate, control) = Gate::new();
    core.snapshot_gate = Some(gate);
    let emu = display_core(core);
    assert_eq!(present(&emu), FrameOutcome::Published);
    control.arm();
    assert_eq!(present(&emu), FrameOutcome::Published);
    control.entered();
    std::thread::scope(|scope| {
        emu.set_input(ButtonMask(ButtonMask::A), ButtonMask(0));
        let (done, result) = mpsc::channel();
        let emu = &emu;
        scope.spawn(move || {
            done.send(present(emu)).unwrap();
        });
        // The worker is stopped after publication. The occupied request slot proves the
        // caller has captured its input, without guessing which thread ran first.
        assert!(wait_for(|| emu.frame_request_pending()));
        emu.set_input(ButtonMask(ButtonMask::B), ButtonMask(0));
        control.release();
        assert_eq!(result.recv_timeout(WAIT).unwrap(), FrameOutcome::Published);
    });
    assert_eq!(
        inputs.lock().unwrap().last(),
        Some(&ButtonMask(ButtonMask::A))
    );

    control.arm();
    assert_eq!(present(&emu), FrameOutcome::Published); // Frame four's snapshot blocks.
    control.entered();
    std::thread::scope(|scope| {
        let (done, result) = mpsc::channel();
        let emu = &emu;
        scope.spawn(move || {
            done.send(present(emu)).unwrap();
        });
        assert!(wait_for(|| emu.frame_request_pending()));
        emu.set_display_paced(false);
        emu.set_display_paced(true);
        control.release();
        assert_eq!(result.recv_timeout(WAIT).unwrap(), FrameOutcome::NoFrame);
    });
    assert_eq!(frame_count(&emu), 4);
    assert_eq!(present(&emu), FrameOutcome::Published);
}

#[test]
fn pause_load_snapshots_and_stop_work_without_display_ticks() {
    let emu = display_core(Box::new(MockCore::new()));
    emu.set_speed(Speed::Paused);
    assert!(wait_for(|| emu.observed_speed() == Speed::Paused));
    assert_eq!(present(&emu), FrameOutcome::NoFrame);
    emu.request_load(42u64.to_le_bytes().to_vec());
    assert_eq!(frame_count(&emu), 42);
    let snapshot = emu.snapshot();
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let state = snapshot.state();
        let _ = snapshot.save_ram();
        let _ = snapshot.thumb();
        done.send(state).unwrap();
    });
    assert_eq!(
        result.recv_timeout(WAIT).unwrap(),
        Some(42u64.to_le_bytes().to_vec())
    );
    emu.set_speed(Speed::Normal);
    assert_eq!(present(&emu), FrameOutcome::Published);
    assert_eq!(frame_count(&emu), 43);
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        drop(emu);
        done.send(()).unwrap();
    });
    result.recv_timeout(WAIT).expect("idle worker did not stop");
}

#[test]
fn fast_batches_keep_their_ceiling_last_image_and_per_core_frame_turbo() {
    let (mut core, log) = Probe::new(Duration::ZERO);
    let inputs = Arc::new(Mutex::new(Vec::new()));
    core.inputs = Some(inputs.clone());
    let emu = display_core(core);
    for _ in 0..20 {
        assert_eq!(present(&emu), FrameOutcome::Published);
    }
    frame_count(&emu); // Finish trailing work before clearing the probes.
    log.lock().unwrap().clear();
    inputs.lock().unwrap().clear();
    emu.set_fast_steps(3);
    emu.set_input(ButtonMask(0), ButtonMask(ButtonMask::A));
    emu.set_speed(Speed::Fast);
    let mut expected_skips = Vec::new();
    for _ in 0..4 {
        let before = frame_count(&emu);
        assert_eq!(present(&emu), FrameOutcome::Published);
        let ran = frame_count(&emu) - before;
        // Three is a ceiling, not a promise: a descheduled test worker can exhaust its budget.
        assert!((1..=3).contains(&ran));
        expected_skips.extend(std::iter::repeat_n(true, ran as usize - 1));
        expected_skips.push(false);
    }
    assert_eq!(*log.lock().unwrap(), expected_skips);
    for (i, mask) in inputs.lock().unwrap().iter().enumerate() {
        assert_eq!(mask.0, if i % 6 < 3 { ButtonMask::A } else { 0 });
    }
    let frame = emu.latest_frame().unwrap();
    let mut reference = MockCore::new();
    reference
        .unserialize(&frame_count(&emu).to_le_bytes())
        .unwrap();
    assert_eq!(&frame[..], reference.video_xrgb8888());
}

#[test]
fn rewind_completes_even_when_there_is_no_history() {
    let emu = display_core(Box::new(MockCore::new()));
    emu.set_rewinding(true);
    assert_eq!(present(&emu), FrameOutcome::NoFrame);
    emu.set_rewinding(false);
    for _ in 0..8 {
        assert_eq!(present(&emu), FrameOutcome::Published);
    }
    assert_eq!(frame_count(&emu), 8);
    emu.set_rewinding(true);
    for want in [9, 7, 5, 3] {
        assert_eq!(present(&emu), FrameOutcome::Published);
        assert_eq!(frame_count(&emu), want);
    }
    assert_eq!(present(&emu), FrameOutcome::NoFrame);
    emu.set_rewinding(false);
    assert_eq!(present(&emu), FrameOutcome::Published);
    assert_eq!(frame_count(&emu), 4);
}

#[test]
fn disabling_the_experiment_restores_the_autonomous_clock() {
    let emu = display_core(Box::new(MockCore::new()));
    assert_eq!(present(&emu), FrameOutcome::Published);
    emu.set_display_paced(false);
    assert!(wait_for(|| emu.published_count() >= 4));
    emu.set_display_paced(true);
    let held = frame_count(&emu);
    std::thread::sleep(Duration::from_millis(45));
    assert_eq!(frame_count(&emu), held);
    assert_eq!(present(&emu), FrameOutcome::Published);
    assert_eq!(frame_count(&emu), held + 1);
}

#[test]
fn live_link_keeps_its_clock_and_ending_it_restores_display_pacing() {
    let emu = display_core(Box::new(MockCore::new()));
    let (_peer, local) = paired_links();
    emu.begin_link(0, Box::new(local));
    assert!(wait_for(|| emu.net().is_active()));
    assert!(wait_for(|| emu.published_count() >= 3));
    emu.end_link();
    assert!(wait_for(|| !emu.net().is_active()));
    let held = frame_count(&emu);
    std::thread::sleep(Duration::from_millis(45));
    assert_eq!(frame_count(&emu), held);
    assert_eq!(present(&emu), FrameOutcome::Published);
    assert_eq!(frame_count(&emu), held + 1);
}
