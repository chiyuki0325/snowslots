/// Short, stereo, waveform-aligned grains compress game time without changing the
/// sample rate inside each grain. The bounded search keeps work predictable on the device.
const HOP: usize = 256;
const SEARCH: usize = 64;

#[derive(Default)]
pub struct PitchStretch {
    source: Vec<i16>,
    tail: Vec<i16>,
    next: f64,
    speeds: Vec<(usize, u32)>,
}

impl PitchStretch {
    pub fn reset(&mut self) {
        self.source.clear();
        self.tail.clear();
        self.next = 0.0;
        self.speeds.clear();
    }

    // Map an output-time position to its source position using the actual speed
    // of each buffered batch, including batches from earlier presents.
    fn source_position(&self, time: f64) -> Option<f64> {
        let mut start = 0;
        let mut elapsed = 0.0;
        for &(end, speed) in &self.speeds {
            let duration = (end - start) as f64 / f64::from(speed);
            if time <= elapsed + duration {
                return Some(start as f64 + (time - elapsed) * f64::from(speed));
            }
            elapsed += duration;
            start = end;
        }
        None
    }

    /// `speed` is the number of game frames actually run this present, not the menu ceiling.
    /// Output is accumulated across presents: a grain waiting for source samples is finished
    /// by the next batch rather than padded or repeated at the boundary.
    pub fn process(&mut self, input: &[i16], speed: u32, out: &mut Vec<i16>) {
        out.clear();
        self.source.extend_from_slice(input);
        let frames = self.source.len() / 2;
        if self.speeds.last().is_some_and(|&(_, old)| old == speed) {
            self.speeds.last_mut().unwrap().0 = frames;
        } else if !input.is_empty() {
            self.speeds.push((frames, speed));
        }
        if self.tail.is_empty() {
            if frames < HOP * 2 {
                return;
            }
            out.extend_from_slice(&self.source[..HOP * 2]);
            self.tail.extend_from_slice(&self.source[HOP * 2..HOP * 4]);
            self.next = HOP as f64;
        }
        while let Some(position) = self.source_position(self.next) {
            if position.round() as usize + HOP * 2 > frames {
                break;
            }
            let centre = position.round() as usize;
            let lower = centre.saturating_sub(SEARCH);
            let upper = (centre + SEARCH).min(frames - HOP * 2);
            let mut best = centre.min(upper);
            let mut score = f64::NEG_INFINITY;
            for at in (lower..=upper).step_by(2) {
                // One mono correlation chooses the same offset for both channels; moving
                // the channels separately would make the stereo image drift.
                let mut dot = 0.0;
                let mut energy = 0.0;
                for i in (0..HOP).step_by(2) {
                    let a = (f64::from(self.tail[i * 2]) + f64::from(self.tail[i * 2 + 1])) * 0.5;
                    let b = (f64::from(self.source[(at + i) * 2])
                        + f64::from(self.source[(at + i) * 2 + 1]))
                        * 0.5;
                    dot += a * b;
                    energy += b * b;
                }
                let similarity = dot / energy.max(1.0).sqrt();
                if similarity > score {
                    score = similarity;
                    best = at;
                }
            }
            for i in 0..HOP {
                let fade = i as f32 / HOP as f32;
                for channel in 0..2 {
                    let old = f32::from(self.tail[i * 2 + channel]);
                    let new = f32::from(self.source[(best + i) * 2 + channel]);
                    out.push((old * (1.0 - fade) + new * fade).round() as i16);
                }
            }
            self.tail
                .copy_from_slice(&self.source[(best + HOP) * 2..(best + HOP * 2) * 2]);
            self.next += HOP as f64;
        }
        // No source before the search window can be used by the next grain. Keep only
        // that small look-behind instead of retaining the whole game's audio.
        let consumed = (self.source_position(self.next).unwrap_or(frames as f64) as usize)
            .saturating_sub(SEARCH)
            .min(frames.saturating_sub(SEARCH));
        self.source.drain(..consumed * 2);
        let mut remaining = consumed;
        let mut elapsed = 0.0;
        let mut start = 0;
        for &(end, batch_speed) in &self.speeds {
            let taken = remaining.min(end - start);
            elapsed += taken as f64 / f64::from(batch_speed);
            remaining -= taken;
            start = end;
            if remaining == 0 {
                break;
            }
        }
        self.next -= elapsed;
        self.speeds.retain_mut(|(end, _)| {
            *end = end.saturating_sub(consumed);
            *end != 0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_tone_and_real_time_length_at_every_fast_speed() {
        let rate = 32_768.0;
        for speed in [2, 3, 4, 6] {
            let mut stretch = PitchStretch::default();
            let mut wave = Vec::new();
            let mut batch = Vec::new();
            for present in 0..60 {
                let start = present * 546 * speed;
                let src: Vec<i16> = (start..start + 546 * speed)
                    .flat_map(|i| {
                        let v = (8000.0
                            * (std::f64::consts::TAU * 440.0 * f64::from(i) / rate).sin())
                            as i16;
                        [v, v / 2]
                    })
                    .collect();
                stretch.process(&src, speed as u32, &mut batch);
                assert!(
                    stretch.source.len() / 2 < 4096,
                    "{speed}x retained unbounded audio"
                );
                assert!(
                    batch
                        .chunks_exact(2)
                        .all(|p| (i32::from(p[0]) - 2 * i32::from(p[1])).abs() <= 3),
                    "{speed}x changed the stereo relationship"
                );
                wave.extend_from_slice(&batch);
            }
            let frames = wave.len() / 2;
            assert!(
                (frames as isize - 60 * 546).abs() < 600,
                "{speed}x produced {frames} frames"
            );
            let skip = 2000.min(frames / 4);
            let channel: Vec<i16> = wave[skip * 2..]
                .chunks_exact(2)
                .map(|pair| pair[0])
                .collect();
            let crossings = channel.windows(2).filter(|p| p[0] <= 0 && p[1] > 0).count();
            let hz = crossings as f64 * rate / channel.len() as f64;
            assert!(
                (hz - 440.0).abs() < 35.0,
                "{speed}x shifted tone to {hz:.1} Hz"
            );
        }
    }

    #[test]
    fn changing_the_frames_actually_run_does_not_accumulate_audio() {
        let mut stretch = PitchStretch::default();
        let mut out = Vec::new();
        let mut total = 0;
        for present in 0..120 {
            let speed = [2, 4, 6, 1][present % 4];
            stretch.process(&vec![0; 546 * speed * 2], speed as u32, &mut out);
            total += out.len() / 2;
            assert!(stretch.source.len() / 2 < 4096);
        }
        assert!(
            (total as isize - 120 * 546).abs() < 1200,
            "{total} output frames"
        );
    }

    #[test]
    fn reset_drops_pending_audio() {
        let mut stretch = PitchStretch::default();
        let mut out = Vec::new();
        stretch.process(&vec![123; HOP * 4], 2, &mut out);
        stretch.reset();
        stretch.process(&vec![0; HOP * 4], 2, &mut out);
        assert!(out.iter().all(|&sample| sample == 0));
    }
}
