use wasm_bindgen::prelude::*;
use crate::oscillator::Oscillator;
use crate::adsr::Adsr;
use crate::filter::Filter;

const VOICES: usize = 8;

struct Voice {
    osc1: Oscillator,
    osc2: Oscillator,
    env: Adsr,
    freq: f32,
    age: u64,        // incremented each note_on — lower = older = steal first
    auto_release: u64,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc1: Oscillator::new(),
            osc2: Oscillator::new(),
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
    detune_ratio: f32,  // 2^(cents/2400): osc1 = freq*ratio, osc2 = freq/ratio
    osc2_mix: f32,      // 0 = osc1 only, 1 = equal blend of both
    lfo: Oscillator,
    lfo_depth: f32,     // 0.0–1.0
    lfo_target: u32,    // 0=pitch, 1=cutoff, 2=mix
    base_cutoff: f32,   // user-set cutoff before LFO modulation
}

#[wasm_bindgen]
impl Synth {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        let mut lfo = Oscillator::new();
        lfo.change_freq(1.0, sample_rate); // default: 1 Hz sine
        Self {
            voices: (0..VOICES).map(|_| Voice::new(sample_rate)).collect(),
            filter: Filter::new(sample_rate),
            sample_rate,
            auto_release_samples: (sample_rate * 0.5) as u64,
            voice_counter: 0,
            detune_ratio: 1.0,
            osc2_mix: 0.0,
            lfo,
            lfo_depth: 0.0,
            lfo_target: 0,
            base_cutoff: 20000.0,
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
        let dr = self.detune_ratio;
        let sr = self.sample_rate;
        let v = &mut self.voices[idx];
        v.osc1.set_freq(freq * dr, sr);
        v.osc2.set_freq(freq / dr, sr);
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
        // Glide: update osc freqs without resetting phase, or start a new voice.
        let dr = self.detune_ratio;
        let sr = self.sample_rate;
        let ar = self.auto_release_samples;
        if let Some(v) = self.voices.iter_mut()
            .filter(|v| v.env.is_active())
            .max_by_key(|v| v.age)
        {
            v.osc1.change_freq(freq * dr, sr);
            v.osc2.change_freq(freq / dr, sr);
            v.freq = freq;
            v.auto_release = ar;
        } else {
            self.note_on(freq);
        }
    }

    /// Detune in cents (0 = unison). Osc1 shifts up by half, osc2 shifts down
    /// by half, so the centre pitch stays on the played note.
    pub fn set_detune(&mut self, cents: f32) {
        self.detune_ratio = 2f32.powf(cents / 2400.0);
    }

    /// 0.0 = osc1 only, 1.0 = equal mix of both oscillators.
    pub fn set_osc2_mix(&mut self, mix: f32) {
        self.osc2_mix = mix.clamp(0.0, 1.0);
    }

    pub fn set_waveform(&mut self, idx: u32) {
        for v in self.voices.iter_mut() {
            v.osc1.set_waveform(idx as usize);
        }
    }

    pub fn set_osc2_waveform(&mut self, idx: u32) {
        for v in self.voices.iter_mut() {
            v.osc2.set_waveform(idx as usize);
        }
    }

    pub fn load_osc1_custom_table(&mut self, samples: &[f32]) {
        for v in self.voices.iter_mut() {
            v.osc1.load_custom_table(samples);
        }
    }

    pub fn load_osc2_custom_table(&mut self, samples: &[f32]) {
        for v in self.voices.iter_mut() {
            v.osc2.load_custom_table(samples);
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

    /// LFO rate in Hz (clamped to 0.01–20).
    pub fn set_lfo_rate(&mut self, hz: f32) {
        self.lfo.change_freq(hz.clamp(0.01, 20.0), self.sample_rate);
    }

    /// LFO depth 0.0–1.0.  Pitch: ±2 semitones at 1.0.  Cutoff: ±3 octaves.  Mix: ±1.0.
    pub fn set_lfo_depth(&mut self, depth: f32) {
        self.lfo_depth = depth.clamp(0.0, 1.0);
    }

    /// LFO target: 0 = pitch, 1 = filter cutoff, 2 = osc2 mix.
    pub fn set_lfo_target(&mut self, target: u32) {
        self.lfo_target = target % 3;
        // Immediately restore clean state for the parameter we're leaving.
        if self.lfo_target != 0 {
            for v in self.voices.iter_mut() {
                v.osc1.set_pitch_scale(1.0);
                v.osc2.set_pitch_scale(1.0);
            }
        }
        if self.lfo_target != 1 {
            self.filter.set_cutoff(self.base_cutoff);
        }
    }

    pub fn set_filter_cutoff(&mut self, hz: f32) {
        self.base_cutoff = hz;
        self.filter.set_cutoff(hz);
    }
    pub fn set_filter_resonance(&mut self, q: f32) { self.filter.set_resonance(q); }
    pub fn set_filter_type(&mut self, t: u32) { self.filter.set_type(t); }

    pub fn tick(&mut self) -> f32 {
        let osc2_mix = self.osc2_mix;
        let lfo_out   = self.lfo.tick();       // -1.0 to +1.0 (sine)
        let lfo_depth = self.lfo_depth;
        let lfo_target = self.lfo_target;

        // Pitch: ±2 semitones at full depth.  2^(x*d*2/12) = 1.0 when d=0 or x=0.
        let pitch_scale = 2f32.powf(lfo_out * lfo_depth * 2.0 / 12.0);

        // Cutoff: ±3 octaves at full depth.
        if lfo_target == 1 {
            let cutoff = (self.base_cutoff * 2f32.powf(lfo_out * lfo_depth * 3.0))
                .clamp(20.0, 20_000.0);
            self.filter.set_cutoff(cutoff);
        }

        // Mix: additive offset clamped to [0, 1].
        let effective_mix = if lfo_target == 2 {
            (osc2_mix + lfo_out * lfo_depth).clamp(0.0, 1.0)
        } else {
            osc2_mix
        };

        let sum: f32 = self.voices.iter_mut().map(|v| {
            if v.auto_release > 0 {
                v.auto_release -= 1;
                if v.auto_release == 0 { v.env.note_off(); }
            }
            // Apply pitch scale (1.0 when not targeting pitch or depth=0).
            let ps = if lfo_target == 0 { pitch_scale } else { 1.0 };
            v.osc1.set_pitch_scale(ps);
            v.osc2.set_pitch_scale(ps);
            if v.env.is_active() {
                let osc = (v.osc1.tick() + v.osc2.tick() * effective_mix) / (1.0 + effective_mix);
                osc * v.env.tick()
            } else {
                0.0
            }
        }).sum();

        // Soft-clip the polyphonic mix via tanh.
        let mixed = (sum * 0.3).tanh();
        self.filter.process(mixed)
    }
}
