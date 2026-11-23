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
                let mut dot = 0.0;
                let mut energy = 0.0;
                for frame in (0..HOP).step_by(2) {
                    for channel in 0..2 {
                        let previous = f64::from(self.tail[frame * 2 + channel]);
                        let current = f64::from(self.source[(at + frame) * 2 + channel]);
                        dot += previous * current;
                        energy += current * current;
                    }
                }
                let similarity = dot / energy.max(1.0).sqrt();
                if similarity > score {
                    score = similarity;
                    best = at;
                }
            }
            for frame in 0..HOP {
                let fade = frame as f32 / HOP as f32;
                for channel in 0..2 {
                    let old = f32::from(self.tail[frame * 2 + channel]);
                    let new = f32::from(self.source[(best + frame) * 2 + channel]);
                    out.push((old * (1.0 - fade) + new * fade).round() as i16);
                }
            }
            self.tail
                .copy_from_slice(&self.source[(best + HOP) * 2..(best + HOP * 2) * 2]);
            self.next += HOP as f64;
        }
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
    fn keeps_pitch_duration_and_stereo_at_every_fast_speed() {
        let rate = 32_768.0;
        for speed in [2, 3, 4, 6] {
            for right_scale in [0.5, -1.0] {
                let mut stretch = PitchStretch::default();
                let mut wave = Vec::new();
                let mut batch = Vec::new();
                for present in 0..60 {
                    let start = present * 546 * speed;
                    let source: Vec<i16> = (start..start + 546 * speed)
                        .flat_map(|frame| {
                            let value = (8000.0
                                * (std::f64::consts::TAU * 440.0 * f64::from(frame) / rate).sin())
                                as i16;
                            [value, (f64::from(value) * right_scale) as i16]
                        })
                        .collect();
                    stretch.process(&source, speed as u32, &mut batch);
                    assert!(stretch.source.len() / 2 < 4096);
                    assert!(batch.chunks_exact(2).all(|pair| {
                        (f64::from(pair[1]) - f64::from(pair[0]) * right_scale).abs() <= 2.0
                    }));
                    wave.extend_from_slice(&batch);
                }
                let frames = wave.len() / 2;
                assert!((frames as isize - 60 * 546).abs() < 600);
                let channel: Vec<i16> = wave[4000..].chunks_exact(2).map(|pair| pair[0]).collect();
                let crossings = channel
                    .windows(2)
                    .filter(|pair| pair[0] <= 0 && pair[1] > 0)
                    .count();
                let hz = crossings as f64 * rate / channel.len() as f64;
                assert!((hz - 440.0).abs() < 35.0, "{speed}x shifted to {hz} Hz");
            }
        }
    }

    #[test]
    fn varying_speed_and_silence_do_not_accumulate_audio() {
        let mut stretch = PitchStretch::default();
        let mut out = Vec::new();
        let mut total = 0;
        for present in 0..120 {
            let speed = [2, 4, 6, 1][present % 4];
            stretch.process(&vec![0; 546 * speed * 2], speed as u32, &mut out);
            total += out.len() / 2;
            assert!(stretch.source.len() / 2 < 4096);
        }
        assert!((total as isize - 120 * 546).abs() < 1200);
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
