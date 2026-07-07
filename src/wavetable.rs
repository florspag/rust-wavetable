use std::f32::consts::PI;
use std::sync::OnceLock;

pub const TABLE_SIZE: usize = 2048;

/// Number of mip levels.  Level 0 = richest (low pitch); level 9 = sparsest (high pitch).
pub const MIP_LEVELS: usize = 10;

/// Maximum harmonic count stored in each mip level, in descending order.
/// Level i is used when floor(Nyquist / freq) first drops below MIP_MAX_HARMONICS[i].
pub const MIP_MAX_HARMONICS: [usize; MIP_LEVELS] = [512, 256, 128, 64, 32, 16, 8, 4, 2, 1];

// ── Global mip table ────────────────────────────────────────────────────────
// Indexed [waveform 0-6][mip level][sample].  Waveform 7 (Custom) is stored
// per-oscillator since it is user-defined at runtime.
static MIP_TABLES: OnceLock<Vec<Vec<Vec<f32>>>> = OnceLock::new();

pub fn get_mip_tables() -> &'static Vec<Vec<Vec<f32>>> {
    MIP_TABLES.get_or_init(|| (0..7).map(build_mip_for_kind).collect())
}

/// Select the mip level with the most harmonics that fit below Nyquist.
pub fn select_mip_level(freq: f32, sr: f32) -> usize {
    let needed = ((sr * 0.5) / freq.max(1.0)) as usize;
    // MIP_MAX_HARMONICS is descending: first entry <= needed is the richest safe level.
    MIP_MAX_HARMONICS.iter()
        .position(|&h| h <= needed)
        .unwrap_or(MIP_LEVELS - 1)
}

// ── Per-waveform mip stack ───────────────────────────────────────────────────

fn build_mip_for_kind(kind: usize) -> Vec<Vec<f32>> {
    if kind == 0 {
        // Sine has only the fundamental — identical at every mip level.
        let table = build_band_table(0, 1);
        return vec![table; MIP_LEVELS];
    }
    MIP_MAX_HARMONICS.iter().map(|&max_h| build_band_table(kind, max_h)).collect()
}

fn build_band_table(kind: usize, max_h: usize) -> Vec<f32> {
    let mut table: Vec<f32> = (0..TABLE_SIZE)
        .map(|i| band_sample(kind, i as f32 / TABLE_SIZE as f32, max_h))
        .collect();
    normalize(&mut table);
    table
}

fn normalize(table: &mut Vec<f32>) {
    let peak = table.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
    if peak > 1e-6 { table.iter_mut().for_each(|x| *x /= peak); }
}

// ── Bandlimited sample functions (one per waveform kind) ────────────────────

fn band_sample(kind: usize, t: f32, max_h: usize) -> f32 {
    match kind {
        // ── Sine ────────────────────────────────────────────────────────────
        0 => (2.0 * PI * t).sin(),

        // ── Sawtooth  2t−1 = −(2/π) Σ_{k=1}^N sin(2πkt)/k ────────────────
        1 => {
            let s: f32 = (1..=max_h)
                .map(|k| (2.0 * PI * k as f32 * t).sin() / k as f32)
                .sum();
            s * (-2.0 / PI)
        },

        // ── Square 50%  (4/π) Σ_{k odd} sin(2πkt)/k ────────────────────────
        2 => {
            let s: f32 = (0..)
                .map(|n| 2 * n + 1)
                .take_while(|&k| k <= max_h)
                .map(|k| (2.0 * PI * k as f32 * t).sin() / k as f32)
                .sum();
            s * (4.0 / PI)
        },

        // ── Triangle  1−4|t−0.5| = −(8/π²) Σ_{k odd} cos(2πkt)/k² ─────────
        3 => {
            let s: f32 = (0..)
                .map(|n| 2 * n + 1)
                .take_while(|&k| k <= max_h)
                .map(|k| (2.0 * PI * k as f32 * t).cos() / (k as f32 * k as f32))
                .sum();
            s * (-8.0 / (PI * PI))
        },

        // ── Pulse 25%  DC + Σ (4sin(πkd)/(πk)) cos(2πkt−πkd), d=0.25 ──────
        4 => {
            let d = 0.25_f32;
            let harmonics: f32 = (1..=max_h).map(|k| {
                let kf = k as f32;
                (4.0 * (PI * kf * d).sin() / (PI * kf))
                    * (2.0 * PI * kf * t - PI * kf * d).cos()
            }).sum();
            2.0 * d - 1.0 + harmonics   // DC = −0.5; normalize() centres it
        },

        // ── Organ  harmonics 1–4 with geometric weights ───────────────────
        5 => {
            let h = |n: usize| -> f32 {
                if n <= max_h { (2.0 * PI * n as f32 * t).sin() } else { 0.0 }
            };
            h(1) + h(2) * 0.5 + h(3) * 0.25 + h(4) * 0.125
        },

        // ── Additive  odd harmonics 1, 3, 5 ──────────────────────────────
        _ => {
            let h = |n: usize| -> f32 {
                if n <= max_h { (2.0 * PI * n as f32 * t).sin() } else { 0.0 }
            };
            h(1) + h(3) / 3.0 + h(5) / 5.0
        },
    }
}
