// Eventide Blackhole-inspired infinite space reverb.
//
// Architecture:
//   predelay → 8× parallel comb (scalable delay, LP damping in feedback)
//           → 4× modulated allpass per channel (LFO-swept delay → shimmer)
//           → wet/dry mix
//
// "Gravity" is raw feedback (0–1); at 1.0 the tail sustains forever.
// "Size" stretches all comb delay lengths from 1× to 4× the base lengths.
// "Mod" drives the LFO depth on the allpass delays — the source of shimmer.
// "Predelay" inserts up to 500 ms before the reverb network.

use std::f32::consts::{TAU, FRAC_PI_2, FRAC_PI_4};

// Base comb delays at 44 100 Hz (same primes as Freeverb for density).
const COMB_BASE_L: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const COMB_BASE_R: [usize; 8] = [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640];

// Allpass delays at 44 100 Hz (Freeverb values).
const ALLPASS_BASE_L: [usize; 4] = [556, 441, 341, 225];
const ALLPASS_BASE_R: [usize; 4] = [579, 464, 364, 248];

const MAX_SIZE_SCALE: f32  = 4.0;   // size = 1.0 → 4× longer delay lines
const MAX_PREDELAY:   usize = 22050; // 500 ms at 44 100 Hz
const MOD_DEPTH_MAX:  f32  = 64.0;  // allpass tap can sweep ± this many samples
const FIXED_GAIN:     f32  = 0.03;  // 2× Freeverb — compensates for lower default feedback
const SCALE_DAMP:     f32  = 0.5;   // damp 0–1 → LP coefficient 0–0.5
const LFO_RATE:       f32  = 0.35;  // Hz — slow enough for shimmer, not pitch-wobble

// ── Feedback comb filter with one-pole LP in the loop ────────────────────────
struct Comb {
    buf: Vec<f32>,
    write_pos: usize,
    delay: usize,
    filterstore: f32,
}

impl Comb {
    fn new(max_delay: usize) -> Self {
        Self { buf: vec![0.0; max_delay + 1], write_pos: 0, delay: max_delay, filterstore: 0.0 }
    }

    fn set_delay(&mut self, d: usize) {
        self.delay = d.clamp(1, self.buf.len() - 1);
    }

    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let read = (self.write_pos + self.buf.len() - self.delay) % self.buf.len();
        let output = self.buf[read];
        self.filterstore = output * damp2 + self.filterstore * damp1;
        self.buf[self.write_pos] = input + self.filterstore * feedback;
        self.write_pos = (self.write_pos + 1) % self.buf.len();
        output
    }
}

// ── Schroeder allpass with modulated read-pointer (shimmer source) ────────────
//
// H(z) = (z^{-D} − g) / (1 − g·z^{-D})    |H| = 1 for constant D
// Sweeping D with the LFO violates the constant-delay assumption and introduces
// subtle pitch artefacts — the characteristic Blackhole shimmer.
struct ModAllpass {
    buf: Vec<f32>,
    write_pos: usize,
    base_delay: f32,
}

impl ModAllpass {
    fn new(base_delay: usize) -> Self {
        let buf_size = base_delay + MOD_DEPTH_MAX as usize + 2;
        Self { buf: vec![0.0; buf_size], write_pos: 0, base_delay: base_delay as f32 }
    }

    fn process(&mut self, input: f32, mod_offset: f32) -> f32 {
        let delay = (self.base_delay + mod_offset).clamp(1.0, self.buf.len() as f32 - 1.0);
        let int_d = delay as usize;
        let frac  = delay.fract();
        // Linear interpolation between adjacent taps.
        let i0 = (self.write_pos + self.buf.len() - int_d    ) % self.buf.len();
        let i1 = (self.write_pos + self.buf.len() - int_d - 1) % self.buf.len();
        let tap = self.buf[i0] * (1.0 - frac) + self.buf[i1] * frac;
        // g = 0.5 (classic Schroeder coefficient for flat passband)
        let v = input + 0.5 * tap;
        self.buf[self.write_pos] = v;
        self.write_pos = (self.write_pos + 1) % self.buf.len();
        tap - 0.5 * v
    }
}

// ── Public Blackhole struct ───────────────────────────────────────────────────
pub struct Blackhole {
    comb_l:    [Comb;       8],
    comb_r:    [Comb;       8],
    allpass_l: [ModAllpass; 4],
    allpass_r: [ModAllpass; 4],

    predelay_l:      Vec<f32>,
    predelay_r:      Vec<f32>,
    predelay_pos:    usize,
    predelay_samples: usize,

    lfo_phase: f32,
    lfo_inc:   f32,

    damp1: f32,
    damp2: f32,

    pub wet:       f32, // 0–1
    pub gravity:   f32, // 0–1 → feedback in comb (1.0 = infinite tail)
    pub size:      f32, // 0–1 → scales comb delays 1×–4×
    pub damp:      f32, // 0–1 → LP rolloff inside comb feedback loop
    pub mod_depth: f32, // 0–1 → allpass shimmer sweep range
    pub predelay:  f32, // 0–1 → 0–500 ms
}

impl Blackhole {
    pub fn new(sample_rate: f32) -> Self {
        let mut bh = Self {
            comb_l:    std::array::from_fn(|i| Comb::new((COMB_BASE_L[i] as f32 * MAX_SIZE_SCALE) as usize)),
            comb_r:    std::array::from_fn(|i| Comb::new((COMB_BASE_R[i] as f32 * MAX_SIZE_SCALE) as usize)),
            allpass_l: std::array::from_fn(|i| ModAllpass::new(ALLPASS_BASE_L[i])),
            allpass_r: std::array::from_fn(|i| ModAllpass::new(ALLPASS_BASE_R[i])),

            predelay_l:       vec![0.0; MAX_PREDELAY],
            predelay_r:       vec![0.0; MAX_PREDELAY],
            predelay_pos:     0,
            predelay_samples: 0,

            lfo_phase: 0.0,
            lfo_inc:   LFO_RATE / sample_rate,

            damp1: 0.0,
            damp2: 1.0,

            wet:       0.0,
            gravity:   0.85,
            size:      0.0,   // base Freeverb delays → first echo ≈ 25ms, fast buildup
            damp:      0.5,
            mod_depth: 0.3,
            predelay:  0.0,
        };

        // Set initial comb delay lengths from default size.
        bh.update_delays();
        bh.update_damp();
        bh
    }

    fn update_delays(&mut self) {
        let scale = 1.0 + self.size * (MAX_SIZE_SCALE - 1.0);
        for (i, c) in self.comb_l.iter_mut().enumerate() {
            c.set_delay((COMB_BASE_L[i] as f32 * scale).round() as usize);
        }
        for (i, c) in self.comb_r.iter_mut().enumerate() {
            c.set_delay((COMB_BASE_R[i] as f32 * scale).round() as usize);
        }
    }

    fn update_damp(&mut self) {
        self.damp1 = self.damp * SCALE_DAMP;
        self.damp2 = 1.0 - self.damp1;
    }

    pub fn set_wet(&mut self, v: f32)       { self.wet       = v.clamp(0.0, 1.0); }
    pub fn set_gravity(&mut self, v: f32)   { self.gravity   = v.clamp(0.0, 1.0); }
    pub fn set_size(&mut self, v: f32)      { self.size      = v.clamp(0.0, 1.0); self.update_delays(); }
    pub fn set_damp(&mut self, v: f32)      { self.damp      = v.clamp(0.0, 1.0); self.update_damp(); }
    pub fn set_mod_depth(&mut self, v: f32) { self.mod_depth = v.clamp(0.0, 1.0); }
    pub fn set_predelay(&mut self, v: f32, sample_rate: f32) {
        self.predelay = v.clamp(0.0, 1.0);
        self.predelay_samples = (self.predelay * MAX_PREDELAY as f32) as usize;
        let _ = sample_rate; // stored externally if needed
    }

    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        // 1 ── Predelay ────────────────────────────────────────────────────────
        // Bypass the buffer entirely when predelay = 0 — a zero-sample circular
        // buffer read would alias onto the write slot, feeding zeros to the combs.
        let (pre_l, pre_r) = if self.predelay_samples == 0 {
            (in_l, in_r)
        } else {
            let pd_read = (self.predelay_pos + MAX_PREDELAY - self.predelay_samples) % MAX_PREDELAY;
            let out = (self.predelay_l[pd_read], self.predelay_r[pd_read]);
            self.predelay_l[self.predelay_pos] = in_l;
            self.predelay_r[self.predelay_pos] = in_r;
            self.predelay_pos = (self.predelay_pos + 1) % MAX_PREDELAY;
            out
        };

        // 2 ── LFO ─────────────────────────────────────────────────────────────
        let lfo_rad = self.lfo_phase * TAU;
        self.lfo_phase = (self.lfo_phase + self.lfo_inc) % 1.0;

        // 3 ── Comb bank ───────────────────────────────────────────────────────
        let input = (pre_l + pre_r) * FIXED_GAIN;
        let (fb, d1, d2) = (self.gravity, self.damp1, self.damp2);
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for c in self.comb_l.iter_mut() { out_l += c.process(input, fb, d1, d2); }
        for c in self.comb_r.iter_mut() { out_r += c.process(input, fb, d1, d2); }

        // 4 ── Modulated allpass chain (shimmer) ───────────────────────────────
        // L allpasses: phases at 0°, 90°, 180°, 270°
        // R allpasses: shifted by an additional 45° for stereo decorrelation
        let mod_samps = self.mod_depth * MOD_DEPTH_MAX;
        for (i, ap) in self.allpass_l.iter_mut().enumerate() {
            let phase = lfo_rad + i as f32 * FRAC_PI_2;
            out_l = ap.process(out_l, phase.sin() * mod_samps);
        }
        for (i, ap) in self.allpass_r.iter_mut().enumerate() {
            let phase = lfo_rad + i as f32 * FRAC_PI_2 + FRAC_PI_4;
            out_r = ap.process(out_r, phase.sin() * mod_samps);
        }

        // 5 ── Wet / dry mix ───────────────────────────────────────────────────
        let dry = 1.0 - self.wet;
        (in_l * dry + out_l * self.wet,
         in_r * dry + out_r * self.wet)
    }
}
