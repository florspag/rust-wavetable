use crate::wavetable;
use std::f32::consts::PI;
use std::sync::OnceLock;

pub const WAVEFORMS: [&str; 8] = ["Sine", "Saw", "Square", "Triangle", "Pulse", "Organ", "Additive", "Custom"];

const SINC_L: isize = 4;                              // taps on each side
const SINC_TAPS: usize = (SINC_L * 2) as usize;      // 8 total
const SINC_TABLE_SIZE: usize = 512;                   // fractional subdivisions

static SINC_TABLE: OnceLock<Vec<[f32; SINC_TAPS]>> = OnceLock::new();

fn sinc_kernel(x: f32) -> f32 {
    if x.abs() < 1e-6 { return 1.0; }
    let pix = PI * x;
    let window = 0.42 + 0.5 * (PI * x / SINC_L as f32).cos()
                      + 0.08 * (2.0 * PI * x / SINC_L as f32).cos();
    pix.sin() / pix * window
}

fn build_sinc_table() -> Vec<[f32; SINC_TAPS]> {
    (0..SINC_TABLE_SIZE).map(|qi| {
        let t = qi as f32 / SINC_TABLE_SIZE as f32;
        let mut weights = [0.0f32; SINC_TAPS];
        for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {
            weights[j] = sinc_kernel(t - k as f32);
        }
        weights
    }).collect()
}

pub struct Oscillator {
    tables: Vec<Vec<f32>>,
    active: usize,
    phase: f32,
    phase_inc: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        SINC_TABLE.get_or_init(build_sinc_table); // warm on first voice, shared by all
        let mut tables: Vec<Vec<f32>> = (0..7).map(wavetable::build_wavetable).collect();
        tables.push(vec![0.0; wavetable::TABLE_SIZE]); // slot 7: custom
        Self {
            tables,
            active: 0,
            phase: 0.0,
            phase_inc: 0.0,
        }
    }

    pub fn set_freq(&mut self, freq: f32, sr: f32) {
        self.phase_inc = freq / sr;
        self.phase = 0.0;
    }

    pub fn change_freq(&mut self, freq: f32, sr: f32) {
        self.phase_inc = freq / sr;
        // phase preserved → no click when sliding pitch live
    }

    pub fn set_waveform(&mut self, idx: usize) {
        if idx < self.tables.len() {
            self.active = idx;
        }
    }

    pub fn load_custom_table(&mut self, samples: &[f32]) {
        let n = samples.len();
        let table = (0..wavetable::TABLE_SIZE)
            .map(|i| {
                let pos = (i as f32 / wavetable::TABLE_SIZE as f32) * n as f32;
                let i0 = pos as usize % n;
                let i1 = (i0 + 1) % n;
                let frac = pos.fract();
                (samples[i0] + frac * (samples[i1] - samples[i0])).clamp(-1.0, 1.0)
            })
            .collect();
        self.tables[7] = table;
        self.active = 7;
    }

    pub fn tick(&mut self) -> f32 {
        let table = &self.tables[self.active];
        let n = wavetable::TABLE_SIZE;
        let pos = self.phase * n as f32;
        let i0 = pos as usize % n;
        let t = pos.fract();

        let weights = &SINC_TABLE.get_or_init(build_sinc_table)
            [(t * SINC_TABLE_SIZE as f32) as usize];

        let mut s = 0.0f32;
        for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {
            let idx = ((i0 as isize + k).rem_euclid(n as isize)) as usize;
            s += table[idx] * weights[j];
        }

        self.phase = (self.phase + self.phase_inc) % 1.0;
        s
    }
}
