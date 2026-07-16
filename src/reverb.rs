// Freeverb — Jezar at Dreampoint's algorithm, adapted for mono-in / stereo-out.
// Delay lengths are tuned for 44 100 Hz; they would need to be scaled for other rates.

const COMB_TUNING_L:    [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const COMB_TUNING_R:    [usize; 8] = [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640];
const ALLPASS_TUNING_L: [usize; 4] = [556, 441, 341, 225];
const ALLPASS_TUNING_R: [usize; 4] = [579, 464, 364, 248];

const FIXED_GAIN:  f32 = 0.015; // scales input before comb banks
const SCALE_ROOM:  f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;   // feedback range: [0.7, 0.98]
const SCALE_DAMP:  f32 = 0.4;   // damp range:     [0.0, 0.4]

struct CombFilter {
    buf:      Vec<f32>,
    pos:      usize,
    store:    f32,   // one-pole low-pass state
    feedback: f32,
    damp1:    f32,
    damp2:    f32,
}

impl CombFilter {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], pos: 0, store: 0.0, feedback: 0.0, damp1: 0.0, damp2: 1.0 }
    }
    fn process(&mut self, input: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * self.damp2 + self.store * self.damp1;
        self.buf[self.pos] = input + self.store * self.feedback;
        self.pos += 1;
        if self.pos >= self.buf.len() { self.pos = 0; }
        out
    }
    fn set_feedback(&mut self, f: f32) { self.feedback = f; }
    fn set_damp(&mut self, d: f32) { self.damp1 = d; self.damp2 = 1.0 - d; }
}

struct AllpassFilter {
    buf: Vec<f32>,
    pos: usize,
}

impl AllpassFilter {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], pos: 0 }
    }
    fn process(&mut self, input: f32) -> f32 {
        let bufout = self.buf[self.pos];
        let out = -input + bufout;
        self.buf[self.pos] = input + bufout * 0.5;
        self.pos += 1;
        if self.pos >= self.buf.len() { self.pos = 0; }
        out
    }
}

pub struct Reverb {
    comb_l:    Vec<CombFilter>,
    comb_r:    Vec<CombFilter>,
    allpass_l: Vec<AllpassFilter>,
    allpass_r: Vec<AllpassFilter>,
    pub wet:       f32, // 0–1 wet/dry mix exposed to wasm_synth
    room_size: f32, // 0–1
    damp:      f32, // 0–1
}

impl Reverb {
    pub fn new() -> Self {
        let mut r = Self {
            comb_l:    COMB_TUNING_L.iter().map(|&n| CombFilter::new(n)).collect(),
            comb_r:    COMB_TUNING_R.iter().map(|&n| CombFilter::new(n)).collect(),
            allpass_l: ALLPASS_TUNING_L.iter().map(|&n| AllpassFilter::new(n)).collect(),
            allpass_r: ALLPASS_TUNING_R.iter().map(|&n| AllpassFilter::new(n)).collect(),
            wet:       0.0,
            room_size: 0.5,
            damp:      0.5,
        };
        r.apply_params();
        r
    }

    fn apply_params(&mut self) {
        let feedback = self.room_size * SCALE_ROOM + OFFSET_ROOM;
        let damp1    = self.damp * SCALE_DAMP;
        for c in self.comb_l.iter_mut().chain(self.comb_r.iter_mut()) {
            c.set_feedback(feedback);
            c.set_damp(damp1);
        }
    }

    pub fn set_wet(&mut self, v: f32)       { self.wet       = v.clamp(0.0, 1.0); }
    pub fn set_room_size(&mut self, v: f32) { self.room_size = v.clamp(0.0, 1.0); self.apply_params(); }
    pub fn set_damp(&mut self, v: f32)      { self.damp      = v.clamp(0.0, 1.0); self.apply_params(); }

    /// Returns the fully mixed (dry + wet) stereo output.
    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let input = (in_l + in_r) * FIXED_GAIN;

        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for c in &mut self.comb_l    { out_l += c.process(input); }
        for c in &mut self.comb_r    { out_r += c.process(input); }
        for a in &mut self.allpass_l { out_l = a.process(out_l); }
        for a in &mut self.allpass_r { out_r = a.process(out_r); }

        let dry = 1.0 - self.wet;
        (in_l * dry + out_l * self.wet,
         in_r * dry + out_r * self.wet)
    }
}
