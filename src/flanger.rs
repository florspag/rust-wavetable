// Flanger — modulated comb filter via a short, LFO-swept delay line.
//
// delay(t) = CENTER + depth × sin(2π·rate·t)
//
// Delay lines are tuned for 44 100 Hz. The right channel's LFO runs 90°
// ahead of the left channel's, giving the classic stereo sweep.

use std::f32::consts::TAU;

const CENTER_SAMPLES: f32 = 110.0; // 2.5 ms at 44 100 Hz
const MAX_DEPTH_SAMPLES: f32 = 110.0; // ±2.5 ms maximum sweep
const BUF_SIZE: usize = 512; // headroom for max delay + interpolation tap

pub struct Flanger {
    buf_l:     Vec<f32>,
    buf_r:     Vec<f32>,
    write_pos: usize,
    lfo_phase: f32,  // 0–1 (normalised)
    lfo_inc:   f32,  // phase advance per sample

    pub wet:      f32, // 0–1
    pub rate:     f32, // Hz
    pub depth:    f32, // 0–1 → 0..MAX_DEPTH_SAMPLES
    pub feedback: f32, // 0–0.9
}

impl Flanger {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            buf_l:     vec![0.0; BUF_SIZE],
            buf_r:     vec![0.0; BUF_SIZE],
            write_pos: 0,
            lfo_phase: 0.0,
            lfo_inc:   0.0,
            wet:       0.0,
            rate:      0.3,
            depth:     0.5,
            feedback:  0.5,
        };
        f.update_lfo(sample_rate);
        f
    }

    fn update_lfo(&mut self, sample_rate: f32) {
        self.lfo_inc = self.rate / sample_rate;
    }

    pub fn set_wet(&mut self, v: f32)                 { self.wet      = v.clamp(0.0, 1.0); }
    pub fn set_rate(&mut self, hz: f32, sr: f32)      { self.rate     = hz.clamp(0.05, 8.0); self.update_lfo(sr); }
    pub fn set_depth(&mut self, v: f32)               { self.depth    = v.clamp(0.0, 1.0); }
    pub fn set_feedback(&mut self, v: f32)            { self.feedback = v.clamp(0.0, 0.9); }

    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        // Stereo LFO — right channel 90° ahead for a wide, sweeping image
        let lfo_l = (self.lfo_phase * TAU).sin();
        let lfo_r = ((self.lfo_phase + 0.25) * TAU).sin();
        self.lfo_phase = (self.lfo_phase + self.lfo_inc) % 1.0;

        let depth_samps = self.depth * MAX_DEPTH_SAMPLES;
        let delay_l = (CENTER_SAMPLES + lfo_l * depth_samps).max(1.0);
        let delay_r = (CENTER_SAMPLES + lfo_r * depth_samps).max(1.0);

        let wp = self.write_pos;
        let delayed_l = read_tap(&self.buf_l, wp, delay_l);
        let delayed_r = read_tap(&self.buf_r, wp, delay_r);

        // Write input + feedback into the delay line
        self.buf_l[wp] = in_l + delayed_l * self.feedback;
        self.buf_r[wp] = in_r + delayed_r * self.feedback;
        self.write_pos = (wp + 1) % BUF_SIZE;

        let dry = 1.0 - self.wet;
        (in_l * dry + delayed_l * self.wet,
         in_r * dry + delayed_r * self.wet)
    }
}

/// Linear interpolation between two adjacent delay-line taps.
fn read_tap(buf: &[f32], write_pos: usize, delay: f32) -> f32 {
    let int_d = delay as usize;
    let frac  = delay.fract();
    let i0 = (write_pos + BUF_SIZE - int_d)     % BUF_SIZE;
    let i1 = (write_pos + BUF_SIZE - int_d - 1) % BUF_SIZE;
    buf[i0] * (1.0 - frac) + buf[i1] * frac
}
