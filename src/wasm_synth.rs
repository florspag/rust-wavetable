use wasm_bindgen::prelude::*;
use crate::oscillator::Oscillator;
use crate::adsr::Adsr;
use crate::filter::Filter;

#[wasm_bindgen]
pub struct Synth {
    osc: Oscillator,
    filter: Filter,
    env: Adsr,
    sample_rate: f32,
    auto_release: u64,
    auto_release_samples: u64,
}

#[wasm_bindgen]
impl Synth {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Oscillator::new(),
            filter: Filter::new(sample_rate),
            env: Adsr::new(sample_rate),
            sample_rate,
            auto_release: 0,
            auto_release_samples: (sample_rate * 0.5) as u64,
        }
    }

    pub fn note_on(&mut self, freq: f32) {
        self.osc.set_freq(freq, self.sample_rate);
        self.env.note_on();
        self.auto_release = self.auto_release_samples;
    }

    pub fn note_off(&mut self) {
        self.auto_release = 0;
        self.env.note_off();
    }

    pub fn set_waveform(&mut self, idx: u32) {
        self.osc.set_waveform(idx as usize);
    }

    pub fn change_freq(&mut self, freq: f32) {
        self.osc.change_freq(freq, self.sample_rate);
        if !self.env.is_active() {
            self.env.note_on();
            self.auto_release = self.auto_release_samples;
        }
    }

    pub fn set_attack(&mut self, secs: f32) {
        self.env.set_attack(secs);
    }

    pub fn set_decay(&mut self, secs: f32) {
        self.env.set_decay(secs);
    }

    pub fn set_sustain(&mut self, level: f32) {
        self.env.set_sustain(level);
    }

    pub fn set_release(&mut self, secs: f32) {
        self.env.set_release(secs);
    }

    pub fn set_filter_cutoff(&mut self, hz: f32) {
        self.filter.set_cutoff(hz);
    }

    pub fn set_filter_resonance(&mut self, q: f32) {
        self.filter.set_resonance(q);
    }

    pub fn set_filter_type(&mut self, t: u32) {
        self.filter.set_type(t);
    }

    pub fn tick(&mut self) -> f32 {
        if self.auto_release > 0 {
            self.auto_release -= 1;
            if self.auto_release == 0 {
                self.env.note_off();
            }
        }
        if self.env.is_active() {
            let osc = self.osc.tick();
            let filtered = self.filter.process(osc);
            filtered * self.env.tick() * 0.3
        } else {
            0.0
        }
    }
}
