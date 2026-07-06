use crate::wavetable;

pub const WAVEFORMS: [&str; 8] = ["Sine", "Saw", "Square", "Triangle", "Pulse", "Organ", "Additive", "Custom"];

pub struct Oscillator {
    tables: Vec<Vec<f32>>,
    active: usize,
    phase: f32,
    phase_inc: f32,
}

impl Oscillator {
    pub fn new() -> Self {
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
        let i1 = pos as usize % n;
        let im1 = (i1 + n - 1) % n;
        let i2 = (i1 + 1) % n;
        let i3 = (i1 + 2) % n;
        let t = pos.fract();
        let (p0, p1, p2, p3) = (table[im1], table[i1], table[i2], table[i3]);
        // Catmull-Rom cubic: C1-continuous, no overshoot on sine-like shapes
        let s = p1 + 0.5 * t * (p2 - p0 + t * (2.0*p0 - 5.0*p1 + 4.0*p2 - p3 + t * (3.0*(p1-p2) + p3 - p0)));
        self.phase = (self.phase + self.phase_inc) % 1.0;
        s
    }
}
