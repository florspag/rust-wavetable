use crate::wavetable;
use std::f32::consts::PI;
use std::sync::OnceLock;

pub const WAVEFORMS: [&str; 8] = ["Sine", "Saw", "Square", "Triangle", "Pulse", "Organ", "Additive", "Custom"];

// ── Blackman-windowed sinc LUT ───────────────────────────────────────────────

const SINC_L: isize = 4;
const SINC_TAPS: usize = (SINC_L * 2) as usize;
const SINC_TABLE_SIZE: usize = 512;

static SINC_TABLE: OnceLock<Vec<[f32; SINC_TAPS]>> = OnceLock::new();

fn sinc_kernel(x: f32) -> f32 {
    if x.abs() < 1e-6 { return 1.0; }
    let pix = PI * x;
    let window = 0.42 + 0.5 * (PI * x / SINC_L as f32).cos()
                      + 0.08 * (2.0 * PI * x / SINC_L as f32).cos();
    pix.sin() / pix * window
}

fn build_sinc_table() -> Vec<[f32; SINC_TAPS]> {
    (0..=SINC_TABLE_SIZE).map(|qi| {
        let t = qi as f32 / SINC_TABLE_SIZE as f32;
        let mut weights = [0.0f32; SINC_TAPS];
        for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {
            weights[j] = sinc_kernel(t - k as f32);
        }
        weights
    }).collect()
}

// ── Oscillator ───────────────────────────────────────────────────────────────

pub struct Oscillator {
    custom_table: Vec<f32>,   // slot 7: user-drawn; nil until load_custom_table()
    active_waveform: usize,
    active_mip: usize,        // updated by set_freq / change_freq
    phase: f32,
    phase_inc: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        // Warm both global tables on the first oscillator; subsequent calls are free.
        SINC_TABLE.get_or_init(build_sinc_table);
        wavetable::get_mip_tables();
        Self {
            custom_table: vec![0.0; wavetable::TABLE_SIZE],
            active_waveform: 0,
            active_mip: 0,
            phase: 0.0,
            phase_inc: 0.0,
        }
    }

    pub fn set_freq(&mut self, freq: f32, sr: f32) {
        self.phase_inc = freq / sr;
        self.phase = 0.0;
        self.active_mip = wavetable::select_mip_level(freq, sr);
    }

    pub fn change_freq(&mut self, freq: f32, sr: f32) {
        self.phase_inc = freq / sr;
        self.active_mip = wavetable::select_mip_level(freq, sr);
        // phase preserved → no click when sliding pitch live
    }

    pub fn set_waveform(&mut self, idx: usize) {
        if idx < 8 { self.active_waveform = idx; }
    }

    pub fn load_custom_table(&mut self, samples: &[f32]) {
        let n = samples.len();
        self.custom_table = (0..wavetable::TABLE_SIZE)
            .map(|i| {
                let pos = (i as f32 / wavetable::TABLE_SIZE as f32) * n as f32;
                let i0 = pos as usize % n;
                let i1 = (i0 + 1) % n;
                let frac = pos.fract();
                (samples[i0] + frac * (samples[i1] - samples[i0])).clamp(-1.0, 1.0)
            })
            .collect();
        self.active_waveform = 7;
    }

    pub fn tick(&mut self) -> f32 {
        let n = wavetable::TABLE_SIZE;
        let pos = self.phase * n as f32;
        let i0 = pos as usize % n;
        let t = pos.fract();

        // Sinc LUT: interpolate between adjacent rows for sub-row smoothness.
        let sinc_table = SINC_TABLE.get_or_init(build_sinc_table);
        let frac_idx = t * SINC_TABLE_SIZE as f32;
        let qi = frac_idx as usize;
        let alpha = frac_idx.fract();
        let w0 = &sinc_table[qi];
        let w1 = &sinc_table[qi + 1]; // safe: table has SINC_TABLE_SIZE + 1 rows

        // Select the correct mip level; custom waveform bypasses mip tables.
        let mip_tables = wavetable::get_mip_tables();
        let table: &[f32] = if self.active_waveform == 7 {
            &self.custom_table
        } else {
            &mip_tables[self.active_waveform][self.active_mip]
        };

        let mut s = 0.0f32;
        for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {
            let idx = ((i0 as isize + k).rem_euclid(n as isize)) as usize;
            s += table[idx] * (w0[j] + alpha * (w1[j] - w0[j]));
        }

        self.phase = (self.phase + self.phase_inc) % 1.0;
        s
    }
}
