use wasm_bindgen::prelude::*;
use crate::oscillator::Oscillator;
use crate::adsr::Adsr;
use crate::filter::Filter;
use crate::reverb::Reverb;
use crate::flanger::Flanger;
use std::f32::consts::FRAC_PI_4;

const VOICES: usize = 8;
const MAX_UNISON: usize = 8;
const MOD_SLOTS: usize = 4;

/// One cell in the modulation matrix.
/// source: 0 = None, 1 = LFO, 2 = Env
/// dest:   0 = Pitch, 1 = Cutoff, 2 = Resonance, 3 = Mix
#[derive(Clone, Copy)]
struct ModSlot { source: u8, dest: u8, amount: f32 }
impl ModSlot {
    fn off() -> Self { Self { source: 0, dest: 0, amount: 0.0 } }
}

fn unison_cents(i: usize, count: usize, detune: f32) -> f32 {
    if count <= 1 { 0.0 }
    else { detune * (i as f32 / (count - 1) as f32 - 0.5) }
}

struct Voice {
    osc1s: Vec<Oscillator>,  // MAX_UNISON detuned copies; only [..unison_count] are active
    osc2: Oscillator,
    env: Adsr,
    filter: Filter,
    freq: f32,
    age: u64,
    auto_release: u64,
    pan_l: f32,      // equal-power left gain  (precomputed from spread)
    pan_r: f32,      // equal-power right gain (precomputed from spread)
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc1s: (0..MAX_UNISON).map(|_| Oscillator::new()).collect(),
            osc2: Oscillator::new(),
            env: Adsr::new(sample_rate),
            filter: Filter::new(sample_rate),
            freq: 0.0,
            age: 0,
            auto_release: 0,
            pan_l: FRAC_PI_4.cos(),  // centre pan: cos(π/4) = 1/√2
            pan_r: FRAC_PI_4.sin(),  // centre pan: sin(π/4) = 1/√2
        }
    }
}

#[wasm_bindgen]
pub struct Synth {
    voices: Vec<Voice>,
    sample_rate: f32,
    auto_release_samples: u64,
    voice_counter: u64,
    detune_ratio: f32,   // 2^(cents/2400): osc1 = freq*ratio, osc2 = freq/ratio
    osc2_mix: f32,       // 0 = osc1 only, 1 = equal blend of both
    unison_count: usize, // 1–MAX_UNISON active osc1 copies per voice
    unison_detune: f32,  // total cents spread across all unison oscillators (0–100)
    lfo: Oscillator,
    lfo2: Oscillator,
    mod_matrix: [ModSlot; MOD_SLOTS],
    base_cutoff: f32,    // unmodulated filter cutoff
    base_resonance: f32, // user-set filter resonance
    base_filter_type: u32,
    spread: f32,         // 0 = mono, 1 = full stereo spread across 8 voices
    reverb: Reverb,
    flanger: Flanger,
    last_left: f32,
    last_right: f32,
}

// Private helpers — separate impl block so wasm_bindgen does not export them.
impl Synth {
    fn update_pans(&mut self) {
        for (i, v) in self.voices.iter_mut().enumerate() {
            let raw = if VOICES > 1 {
                -1.0 + 2.0 * i as f32 / (VOICES - 1) as f32
            } else { 0.0 };
            let pan = raw * self.spread;           // [-spread, +spread]
            let angle = (pan + 1.0) * FRAC_PI_4;  // [0, π/2]
            v.pan_l = angle.cos();
            v.pan_r = angle.sin();
        }
    }

    fn update_unison_freqs(&mut self) {
        let count = self.unison_count;
        let detune = self.unison_detune;
        let dr = self.detune_ratio;
        let sr = self.sample_rate;
        for v in self.voices.iter_mut() {
            if v.freq <= 0.0 { continue; }
            let base = v.freq * dr;
            for i in 0..count {
                let cents = unison_cents(i, count, detune);
                v.osc1s[i].change_freq(base * 2f32.powf(cents / 1200.0), sr);
            }
        }
    }
}

#[wasm_bindgen]
impl Synth {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        let mut lfo = Oscillator::new();
        lfo.change_freq(1.0, sample_rate); // default: 1 Hz sine
        let mut lfo2 = Oscillator::new();
        lfo2.change_freq(0.5, sample_rate); // default: 0.5 Hz sine
        let mut synth = Self {
            voices: (0..VOICES).map(|_| Voice::new(sample_rate)).collect(),
            sample_rate,
            auto_release_samples: (sample_rate * 0.5) as u64,
            voice_counter: 0,
            detune_ratio: 1.0,
            osc2_mix: 0.0,
            unison_count: 1,
            unison_detune: 0.0,
            lfo,
            lfo2,
            mod_matrix: [ModSlot::off(); MOD_SLOTS],
            base_cutoff: sample_rate * 0.49, // matches Filter::new() default — fully open
            base_resonance: 0.707,
            base_filter_type: 0,
            spread: 0.0,
            reverb: Reverb::new(),
            flanger: Flanger::new(sample_rate),
            last_left: 0.0,
            last_right: 0.0,
        };
        synth.update_pans();
        synth
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
        let bc = self.base_cutoff;
        let br = self.base_resonance;
        let bt = self.base_filter_type;
        let count = self.unison_count;
        let detune = self.unison_detune;
        let v = &mut self.voices[idx];
        for i in 0..count {
            let cents = unison_cents(i, count, detune);
            v.osc1s[i].set_freq(freq * dr * 2f32.powf(cents / 1200.0), sr);
            // Distribute phases evenly to avoid phase cancellation across unison voices.
            v.osc1s[i].set_phase(i as f32 / count as f32);
        }
        v.osc2.set_freq(freq / dr, sr);
        v.env.note_on();
        // Reset filter to current global settings (stolen voice may have a mid-sweep state).
        v.filter.set_cutoff(bc);
        v.filter.set_resonance(br);
        v.filter.set_type(bt);
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
        let count = self.unison_count;
        let detune = self.unison_detune;
        if let Some(v) = self.voices.iter_mut()
            .filter(|v| v.env.is_active())
            .max_by_key(|v| v.age)
        {
            for i in 0..count {
                let cents = unison_cents(i, count, detune);
                v.osc1s[i].change_freq(freq * dr * 2f32.powf(cents / 1200.0), sr);
            }
            v.osc2.change_freq(freq / dr, sr);
            v.freq = freq;
            v.auto_release = ar;
        } else {
            self.note_on(freq);
        }
    }

    /// Detune in cents (0 = unison). Osc1 shifts up by half, osc2 down by half.
    pub fn set_detune(&mut self, cents: f32) {
        self.detune_ratio = 2f32.powf(cents / 2400.0);
    }

    /// 0.0 = osc1 only, 1.0 = equal mix of both oscillators.
    pub fn set_osc2_mix(&mut self, mix: f32) {
        self.osc2_mix = mix.clamp(0.0, 1.0);
    }

    /// 0.0 = all voices centred (mono), 1.0 = voice 0 hard-left → voice 7 hard-right.
    pub fn set_spread(&mut self, spread: f32) {
        self.spread = spread.clamp(0.0, 1.0);
        self.update_pans();
    }

    /// Number of detuned osc1 copies per voice (1–8).
    pub fn set_unison_count(&mut self, n: u32) {
        self.unison_count = (n as usize).clamp(1, MAX_UNISON);
        self.update_unison_freqs();
    }

    /// Total pitch spread across all unison oscillators in cents (0–100).
    pub fn set_unison_detune(&mut self, cents: f32) {
        self.unison_detune = cents.clamp(0.0, 100.0);
        self.update_unison_freqs();
    }

    /// Stereo output — call get_left() / get_right() after each tick().
    pub fn tick(&mut self) {
        // Copy cheaply-cloneable state so we can mutably borrow voices below.
        let count         = self.unison_count;
        let lfo_val       = self.lfo.tick();    // -1.0 to +1.0
        let lfo2_val      = self.lfo2.tick();  // -1.0 to +1.0
        let mod_matrix    = self.mod_matrix;   // [ModSlot; 4] is Copy
        let base_cutoff   = self.base_cutoff;
        let base_resonance= self.base_resonance;
        let osc2_mix      = self.osc2_mix;

        let mut left  = 0.0f32;
        let mut right = 0.0f32;

        for v in self.voices.iter_mut() {
            if v.auto_release > 0 {
                v.auto_release -= 1;
                if v.auto_release == 0 { v.env.note_off(); }
            }
            if !v.env.is_active() { continue; }

            let env_val = v.env.tick();

            // ── Accumulate per-voice modulation from each active slot ─────
            let (mut pitch_mod, mut cutoff_mod, mut res_mod, mut mix_mod) = (0f32, 0f32, 0f32, 0f32);
            for slot in &mod_matrix {
                let src = match slot.source {
                    1 => lfo_val,
                    2 => env_val,
                    3 => lfo2_val,
                    _ => continue,
                };
                let c = src * slot.amount;
                match slot.dest {
                    0 => pitch_mod  += c,
                    1 => cutoff_mod += c,
                    2 => res_mod    += c,
                    3 => mix_mod    += c,
                    _ => {}
                }
            }

            // ── Apply modulation ──────────────────────────────────────────
            // Pitch: amount=±1 gives ±2 semitones vibrato.
            let pitch_scale = 2f32.powf(pitch_mod * 2.0 / 12.0);
            for osc in v.osc1s[..count].iter_mut() { osc.set_pitch_scale(pitch_scale); }
            v.osc2.set_pitch_scale(pitch_scale);

            // Cutoff: amount=±1 sweeps ±3 octaves around base.
            let mod_cutoff = (base_cutoff * 2f32.powf(cutoff_mod * 3.0)).clamp(20.0, 20_000.0);
            v.filter.set_cutoff(mod_cutoff);

            // Resonance: amount=±1 shifts Q by ±10.
            let mod_res = (base_resonance + res_mod * 10.0).clamp(0.1, 20.0);
            v.filter.set_resonance(mod_res);

            // Mix: additive offset, clamped.
            let effective_mix = (osc2_mix + mix_mod).clamp(0.0, 1.0);

            // ── Audio path ────────────────────────────────────────────────
            let mut osc1_out = 0.0f32;
            for osc in v.osc1s[..count].iter_mut() { osc1_out += osc.tick(); }
            osc1_out /= count as f32;

            let osc = (osc1_out + v.osc2.tick() * effective_mix) / (1.0 + effective_mix);
            let filtered = v.filter.process(osc * env_val);
            left  += filtered * v.pan_l;
            right += filtered * v.pan_r;
        }

        let raw_l = (left  * 0.3).tanh();
        let raw_r = (right * 0.3).tanh();
        let (fl_l, fl_r) = self.flanger.process(raw_l, raw_r);
        let (out_l, out_r) = self.reverb.process(fl_l, fl_r);
        self.last_left  = out_l;
        self.last_right = out_r;
    }

    pub fn get_left(&self)  -> f32 { self.last_left  }
    pub fn get_right(&self) -> f32 { self.last_right }

    pub fn set_waveform(&mut self, idx: u32) {
        for v in self.voices.iter_mut() {
            for osc in v.osc1s.iter_mut() { osc.set_waveform(idx as usize); }
        }
    }

    pub fn set_osc2_waveform(&mut self, idx: u32) {
        for v in self.voices.iter_mut() { v.osc2.set_waveform(idx as usize); }
    }

    pub fn load_osc1_custom_table(&mut self, samples: &[f32]) {
        for v in self.voices.iter_mut() {
            for osc in v.osc1s.iter_mut() { osc.load_custom_table(samples); }
        }
    }

    pub fn load_osc2_custom_table(&mut self, samples: &[f32]) {
        for v in self.voices.iter_mut() { v.osc2.load_custom_table(samples); }
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

    /// LFO 1 rate in Hz (clamped to 0.01–20).
    pub fn set_lfo_rate(&mut self, freq: f32) {
        self.lfo.change_freq(freq.clamp(0.01, 20.0), self.sample_rate);
    }

    /// LFO 2 rate in Hz (clamped to 0.01–20).
    pub fn set_lfo2_rate(&mut self, freq: f32) {
        self.lfo2.change_freq(freq.clamp(0.01, 20.0), self.sample_rate);
    }

    /// Reverb wet/dry mix (0 = dry only, 1 = full reverb).
    pub fn set_reverb_wet(&mut self, v: f32)       { self.reverb.set_wet(v); }
    /// Reverb room size (0 = short tail, 1 = long tail).
    pub fn set_reverb_room(&mut self, v: f32)      { self.reverb.set_room_size(v); }
    /// Reverb damping (0 = bright, 1 = dark).
    pub fn set_reverb_damp(&mut self, v: f32)      { self.reverb.set_damp(v); }

    /// LFO 1 waveform (0 = Sine, 1 = Saw, 2 = Square, 3 = Triangle, 4 = Pulse).
    pub fn set_lfo_waveform(&mut self, idx: u32) {
        self.lfo.set_waveform(idx as usize);
    }

    /// LFO 2 waveform (0 = Sine, 1 = Saw, 2 = Square, 3 = Triangle, 4 = Pulse).
    pub fn set_lfo2_waveform(&mut self, idx: u32) {
        self.lfo2.set_waveform(idx as usize);
    }

    pub fn set_flanger_wet(&mut self, v: f32)      { self.flanger.set_wet(v); }
    pub fn set_flanger_rate(&mut self, hz: f32)    { self.flanger.set_rate(hz, self.sample_rate); }
    pub fn set_flanger_depth(&mut self, v: f32)    { self.flanger.set_depth(v); }
    pub fn set_flanger_feedback(&mut self, v: f32) { self.flanger.set_feedback(v); }

    /// Configure one modulation matrix slot.
    ///
    /// - `slot`   — 0–3
    /// - `source` — 0 = None, 1 = LFO, 2 = Env
    /// - `dest`   — 0 = Pitch, 1 = Cutoff, 2 = Resonance, 3 = Mix
    /// - `amount` — −1.0 to +1.0 (bipolar depth)
    pub fn set_mod_slot(&mut self, slot: u32, source: u32, dest: u32, amount: f32) {
        let i = slot as usize;
        if i < MOD_SLOTS {
            self.mod_matrix[i] = ModSlot {
                source: source as u8,
                dest:   dest   as u8,
                amount: amount.clamp(-1.0, 1.0),
            };
        }
    }

    pub fn set_filter_cutoff(&mut self, freq: f32) {
        self.base_cutoff = freq;
        for v in self.voices.iter_mut() { v.filter.set_cutoff(freq); }
    }

    pub fn set_filter_resonance(&mut self, q_factor: f32) {
        self.base_resonance = q_factor;
        for v in self.voices.iter_mut() { v.filter.set_resonance(q_factor); }
    }

    pub fn set_filter_type(&mut self, filter_type: u32) {
        self.base_filter_type = filter_type;
        for v in self.voices.iter_mut() { v.filter.set_type(filter_type); }
    }
}
