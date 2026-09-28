//! Device frame ordering: the experiment hands input to a requested core batch before drawing.

use std::time::Duration;

/// Measured from the start of the display iteration, not added after input handling.
/// Leave some of the 16.67 ms present for drawing and swap; slow cores never hold the UI forever.
pub const CORE_WAIT_BUDGET: Duration = Duration::from_millis(14);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStep {
    Advance,
    Emulate,
    Render,
    Swap,
}

/// Take the setting once, before any step can change it. A menu toggle must not advance
/// twice (or not at all) in the frame that receives it. Stop on swap failure as before.
pub fn run_frame<E>(
    low_latency: bool,
    mut step: impl FnMut(FrameStep) -> Result<(), E>,
) -> Result<(), E> {
    use FrameStep::*;
    let steps: &[_] = if low_latency {
        &[Advance, Emulate, Render, Swap]
    } else {
        &[Render, Swap, Advance]
    };
    for &next in steps {
        step(next)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run_frame, FrameStep::*};

    #[test]
    fn default_keeps_input_after_swap() {
        let mut calls = Vec::new();
        run_frame(false, |step| {
            calls.push(step);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(calls, [Render, Swap, Advance]);
    }

    #[test]
    fn low_latency_delivers_input_before_render_and_swap() {
        let mut calls = Vec::new();
        run_frame(true, |step| {
            calls.push(step);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(calls, [Advance, Emulate, Render, Swap]);
    }

    #[test]
    fn toggling_during_advance_only_changes_the_next_frames_order() {
        for initial in [false, true] {
            let mut enabled = initial;
            let mut calls = Vec::new();
            for _ in 0..2 {
                run_frame(enabled, |step| {
                    calls.push(step);
                    if step == Advance {
                        enabled = !enabled;
                    }
                    Ok::<_, ()>(())
                })
                .unwrap();
            }
            let normal: &[_] = &[Render, Swap, Advance];
            let low: &[_] = &[Advance, Emulate, Render, Swap];
            let expected = if initial {
                [low, normal]
            } else {
                [normal, low]
            };
            assert_eq!(calls, expected.concat());
        }
    }

    #[test]
    fn swap_failure_stops_the_frame_without_a_second_advance() {
        for enabled in [false, true] {
            let mut calls = Vec::new();
            let result = run_frame(enabled, |step| {
                calls.push(step);
                if step == Swap {
                    Err("swap failed")
                } else {
                    Ok(())
                }
            });
            assert_eq!(result, Err("swap failed"));
            let expected = if enabled {
                vec![Advance, Emulate, Render, Swap]
            } else {
                vec![Render, Swap]
            };
            assert_eq!(calls, expected);
        }
    }
}
