use crate::wavetable;

pub const WAVEFORMS: [&str; 4] = ["Sine", "Saw", "Square", "Triangle"];



pub struct Oscillator {
    tables: Vec<Vec<f32>>,
    active: usize,
    phase: f32,
    phase_inc: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        Self {
            tables: (0..4).map(wavetable::build_wavetable).collect(),
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
        self.active = idx.min(3);
    }

    pub fn tick(&mut self) -> f32 {
        let table = &self.tables[self.active];
        let pos = self.phase * wavetable::TABLE_SIZE as f32;
        let i0 = pos as usize % wavetable::TABLE_SIZE;
        let i1 = (i0 + 1) % wavetable::TABLE_SIZE;
        let frac = pos.fract();
        let s = table[i0] + frac * (table[i1] - table[i0]);
        self.phase = (self.phase + self.phase_inc) % 1.0;
        s
    }
}