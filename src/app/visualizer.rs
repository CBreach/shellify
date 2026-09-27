//! The audio visualizer's animation model.
//!
//! Loudness comes from mpv (overall RMS and peak in dB, ~15 times a second;
//! see `player::mpv`). mpv doesn't expose a frequency spectrum, so the bars
//! share that real loudness out with a smoothly wandering, bass-heavy
//! pattern: heights and punch follow the music, the per-bar shape is a
//! stylization. With no meter data (e.g. mpv missing) it idles gently.
//!
//! Everything here is pure and deterministic (own PRNG, time passed in), so
//! the animation is unit-testable; `ui::visualizer` only draws the result.

use serde::Deserialize;

/// Number of bands the model keeps; the renderer samples as many as fit.
pub const BANDS: usize = 48;

/// Loudness mapped to 0..1: this many dB below full scale is silence.
const FLOOR_DB: f32 = -50.0;
/// And this is treated as full height (music is rarely mastered hotter).
const CEILING_DB: f32 = -8.0;
/// Seconds for bars to rise / fall most of the way to their target.
const ATTACK: f32 = 0.05;
const RELEASE: f32 = 0.30;
/// Peak caps hold briefly, then fall with this acceleration (height/s²).
const PEAK_HOLD: f32 = 0.35;
const PEAK_GRAVITY: f32 = 2.5;
/// Without meter data for this long, fall back to the gentle idle motion.
const STALE_AFTER: f32 = 1.0;
const IDLE_LEVEL: f32 = 0.18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VizStyle {
    /// Vertical bars with falling peak caps.
    #[default]
    Bars,
    /// Bars growing up and down from a center line.
    Mirror,
    /// An oscilloscope-style line.
    Wave,
    /// Bouncing particles.
    Dots,
}

impl VizStyle {
    pub const ALL: [VizStyle; 4] = [Self::Bars, Self::Mirror, Self::Wave, Self::Dots];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bars => "bars",
            Self::Mirror => "mirror",
            Self::Wave => "wave",
            Self::Dots => "dots",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.label() == s)
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone)]
pub struct Visualizer {
    /// Smoothed overall loudness, 0..1.
    pub level: f32,
    /// Band heights, 0..1, low frequencies first.
    pub bands: [f32; BANDS],
    /// Peak caps (bars style), 0..1.
    pub peaks: [f32; BANDS],
    /// Advances with time and loudness; drives the wave and dots motion.
    pub phase: f32,
    /// Loudness from the latest meter reading.
    target: f32,
    /// A burst on sudden peaks (drum hits), decaying quickly.
    kick: f32,
    /// Per-band weights that wander slowly, so the shape keeps changing.
    weights: [f32; BANDS],
    peak_age: [f32; BANDS],
    peak_speed: [f32; BANDS],
    /// Seconds since the last meter reading.
    since_meter: f32,
    rng: u64,
}

impl Default for Visualizer {
    fn default() -> Self {
        let mut v = Self {
            level: 0.0,
            bands: [0.0; BANDS],
            peaks: [0.0; BANDS],
            phase: 0.0,
            target: 0.0,
            kick: 0.0,
            weights: [0.5; BANDS],
            peak_age: [0.0; BANDS],
            peak_speed: [0.0; BANDS],
            since_meter: f32::INFINITY,
            rng: 0x9E37_79B9_7F4A_7C15,
        };
        for i in 0..BANDS {
            v.weights[i] = 0.4 + 0.6 * v.random();
        }
        v
    }
}

impl Visualizer {
    /// A meter reading from mpv, in dBFS (`-inf` for digital silence).
    pub fn meter(&mut self, rms_db: f32, peak_db: f32) {
        let loud = db_to_unit(rms_db);
        // Peak well above RMS, or a jump in loudness, reads as a hit.
        let crest = ((peak_db - rms_db) / 12.0).clamp(0.0, 1.0);
        let jump = (loud - self.target).max(0.0) * 3.0;
        self.kick = self.kick.max((crest * 0.5 + jump).min(1.0) * loud);
        self.target = loud;
        self.since_meter = 0.0;
    }

    /// Advances the animation by `dt` seconds. `playing` false lets
    /// everything settle to rest.
    pub fn tick(&mut self, dt: f32, playing: bool) {
        let dt = dt.clamp(0.0, 0.25);
        self.since_meter += dt;
        let target = if !playing {
            0.0
        } else if self.since_meter > STALE_AFTER {
            IDLE_LEVEL
        } else {
            self.target
        };
        self.level = approach(self.level, target, dt);
        self.kick *= (-dt / 0.12).exp();
        self.phase += dt * (1.0 + 3.0 * self.level);

        for i in 0..BANDS {
            // Random walk, pulled back towards the middle.
            let step = (self.random() - 0.5) * 2.4 * dt;
            self.weights[i] =
                (self.weights[i] + step + (0.7 - self.weights[i]) * dt).clamp(0.2, 1.0);

            // Bass-heavy tilt plus a slow travelling ripple.
            let x = i as f32 / (BANDS - 1) as f32;
            let tilt = 1.0 - 0.55 * x;
            let ripple = 0.85 + 0.15 * (self.phase * 1.7 + x * 9.0).sin();
            let burst = self.kick * (0.6 + 0.4 * self.random());
            let band_target = if playing {
                ((self.level * self.weights[i] * tilt * ripple) * 1.25 + burst * tilt).min(1.0)
            } else {
                0.0
            };
            self.bands[i] = approach(self.bands[i], band_target, dt);

            // Peaks jump up with the bar, hold, then fall faster and faster.
            if self.bands[i] >= self.peaks[i] {
                self.peaks[i] = self.bands[i];
                self.peak_age[i] = 0.0;
                self.peak_speed[i] = 0.0;
            } else {
                self.peak_age[i] += dt;
                if self.peak_age[i] > PEAK_HOLD {
                    self.peak_speed[i] += PEAK_GRAVITY * dt;
                    self.peaks[i] = (self.peaks[i] - self.peak_speed[i] * dt).max(self.bands[i]);
                }
            }
        }
    }

    /// Nothing is moving any more, so the animation clock can stop.
    pub fn at_rest(&self) -> bool {
        self.level < 0.005 && self.peaks.iter().all(|&p| p < 0.005)
    }

    /// Band `i` of `n`, resampled from the model's bands (averaging).
    pub fn sample(values: &[f32; BANDS], i: usize, n: usize) -> f32 {
        if n == 0 {
            return 0.0;
        }
        let start = i * BANDS / n;
        let end = ((i + 1) * BANDS / n).max(start + 1).min(BANDS);
        values[start..end].iter().sum::<f32>() / (end - start) as f32
    }

    /// xorshift64*: small, fast, deterministic.
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let x = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// dBFS to 0..1 on the visualizer's scale.
pub fn db_to_unit(db: f32) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// Exponential smoothing: fast when rising, slower when falling.
fn approach(current: f32, target: f32, dt: f32) -> f32 {
    let tau = if target > current { ATTACK } else { RELEASE };
    target + (current - target) * (-dt / tau).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 30.0;

    fn run(v: &mut Visualizer, seconds: f32, playing: bool) {
        for _ in 0..(seconds / FRAME) as usize {
            v.tick(FRAME, playing);
        }
    }

    #[test]
    fn db_scale() {
        assert_eq!(db_to_unit(f32::NEG_INFINITY), 0.0);
        assert_eq!(db_to_unit(-80.0), 0.0);
        assert_eq!(db_to_unit(-8.0), 1.0);
        assert_eq!(db_to_unit(0.0), 1.0);
        assert!((db_to_unit(-29.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn follows_loudness_quickly_up_and_slowly_down() {
        let mut v = Visualizer::default();
        v.meter(-12.0, -6.0);
        run(&mut v, 0.2, true);
        let loud = v.level;
        assert!(loud > 0.85, "rises fast: {loud}");

        v.meter(-45.0, -42.0);
        v.tick(FRAME, true);
        assert!(v.level > 0.7, "falls gradually, not instantly: {}", v.level);
        run(&mut v, 1.5, true);
        assert!(v.level < 0.2, "but does fall: {}", v.level);
    }

    #[test]
    fn louder_music_means_taller_bars() {
        let average = |db: f32| {
            let mut v = Visualizer::default();
            for _ in 0..30 {
                v.meter(db, db + 3.0);
                run(&mut v, 0.066, true);
            }
            v.bands.iter().sum::<f32>() / BANDS as f32
        };
        assert!(average(-12.0) > average(-30.0) + 0.2);
        assert!(average(-30.0) > average(-48.0));
    }

    #[test]
    fn peaks_hold_then_fall_and_never_sit_below_the_bar() {
        let mut v = Visualizer::default();
        v.meter(-10.0, -4.0);
        run(&mut v, 0.3, true);
        let top = v.peaks[0];
        v.meter(-60.0, -60.0);
        run(&mut v, 0.2, true);
        assert!(v.peaks[0] > top * 0.9, "held");
        run(&mut v, 1.5, true);
        assert!(v.peaks[0] < top * 0.5, "fell");
        assert!(v.peaks.iter().zip(&v.bands).all(|(p, b)| p >= b));
    }

    #[test]
    fn pausing_settles_to_rest_and_idle_motion_without_meter() {
        let mut v = Visualizer::default();
        v.meter(-10.0, -5.0);
        run(&mut v, 0.5, true);
        assert!(!v.at_rest());
        run(&mut v, 4.0, false);
        assert!(
            v.at_rest(),
            "level {} peak {:?}",
            v.level,
            v.peaks.iter().cloned().fold(0.0, f32::max)
        );

        // Playing but no meter readings (no mpv): gentle, non-zero motion.
        let mut v = Visualizer::default();
        run(&mut v, 2.0, true);
        assert!((v.level - IDLE_LEVEL).abs() < 0.02);
    }

    #[test]
    fn resampling_covers_every_band() {
        let mut values = [0.0; BANDS];
        values[BANDS - 1] = 1.0;
        assert!(
            Visualizer::sample(&values, 9, 10) > 0.0,
            "last column sees the last band"
        );
        assert_eq!(Visualizer::sample(&values, 0, 10), 0.0);
        assert_eq!(Visualizer::sample(&values, 0, 0), 0.0);
        // More columns than bands still works.
        assert!(Visualizer::sample(&values, 99, 100) > 0.0);
    }

    #[test]
    fn styles_cycle_and_parse() {
        assert_eq!(VizStyle::Dots.next(), VizStyle::Bars);
        assert_eq!(VizStyle::parse("wave"), Some(VizStyle::Wave));
        assert_eq!(VizStyle::parse("disco"), None);
    }
}
