use wasm_bindgen::prelude::*;
use crate::oscillator::Oscillator;
use crate::adsr::Adsr;
use crate::filter::Filter;

const VOICES: usize = 8;

struct Voice {
    osc: Oscillator,
    env: Adsr,
    freq: f32,
    age: u64,        // incremented each note_on — lower = older = steal first
    auto_release: u64,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc: Oscillator::new(),
            env: Adsr::new(sample_rate),
            freq: 0.0,
            age: 0,
            auto_release: 0,
        }
    }
}

#[wasm_bindgen]
pub struct Synth {
    voices: Vec<Voice>,
    filter: Filter,
    sample_rate: f32,
    auto_release_samples: u64,
    voice_counter: u64,
}

#[wasm_bindgen]
impl Synth {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: (0..VOICES).map(|_| Voice::new(sample_rate)).collect(),
            filter: Filter::new(sample_rate),
            sample_rate,
            auto_release_samples: (sample_rate * 0.5) as u64,
            voice_counter: 0,
        }
    }

    pub fn note_on(&mut self, freq: f32) {
        // Prefer a silent voice; fall back to stealing the oldest active one.
        let idx = self.voices.iter().position(|v| !v.env.is_active())
            .unwrap_or_else(|| {
                self.voices.iter().enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });

        self.voice_counter += 1;
        let ar = self.auto_release_samples;
        let v = &mut self.voices[idx];
        v.osc.set_freq(freq, self.sample_rate);
        v.env.note_on();
        v.freq = freq;
        v.age = self.voice_counter;
        v.auto_release = ar;
    }

    pub fn note_off(&mut self, freq: f32) {
        // Release the most-recently-triggered voice playing this frequency.
        if let Some(v) = self.voices.iter_mut()
            .filter(|v| v.env.is_active() && (v.freq - freq).abs() < 1.0)
            .max_by_key(|v| v.age)
        {
            v.auto_release = 0;
            v.env.note_off();
        }
    }

    pub fn change_freq(&mut self, freq: f32) {
        // Update the most-recently-triggered active voice (freq slider glide),
        // or start a new voice if none are active.
        if let Some(v) = self.voices.iter_mut()
            .filter(|v| v.env.is_active())
            .max_by_key(|v| v.age)
        {
            v.osc.change_freq(freq, self.sample_rate);
            v.freq = freq;
            v.auto_release = self.auto_release_samples;
        } else {
            self.note_on(freq);
        }
    }

    pub fn set_waveform(&mut self, idx: u32) {
        for v in self.voices.iter_mut() {
            v.osc.set_waveform(idx as usize);
        }
    }

    pub fn load_custom_table(&mut self, samples: &[f32]) {
        for v in self.voices.iter_mut() {
            v.osc.load_custom_table(samples);
        }
    }

    pub fn set_attack(&mut self, secs: f32) {
        for v in self.voices.iter_mut() { v.env.set_attack(secs); }
    }

    pub fn set_decay(&mut self, secs: f32) {
        for v in self.voices.iter_mut() { v.env.set_decay(secs); }
    }

    pub fn set_sustain(&mut self, level: f32) {
        for v in self.voices.iter_mut() { v.env.set_sustain(level); }
    }

    pub fn set_release(&mut self, secs: f32) {
        for v in self.voices.iter_mut() { v.env.set_release(secs); }
    }

    pub fn set_filter_cutoff(&mut self, hz: f32) { self.filter.set_cutoff(hz); }
    pub fn set_filter_resonance(&mut self, q: f32) { self.filter.set_resonance(q); }
    pub fn set_filter_type(&mut self, t: u32) { self.filter.set_type(t); }

    pub fn tick(&mut self) -> f32 {
        let sum: f32 = self.voices.iter_mut().map(|v| {
            if v.auto_release > 0 {
                v.auto_release -= 1;
                if v.auto_release == 0 { v.env.note_off(); }
            }
            if v.env.is_active() { v.osc.tick() * v.env.tick() } else { 0.0 }
        }).sum();

        // Soft-clip the polyphonic mix via tanh — transparent at low amplitudes,
        // graceful saturation when many voices sound simultaneously.
        let mixed = (sum * 0.3).tanh();
        self.filter.process(mixed)
    }
}
