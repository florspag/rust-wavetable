use std::f32::consts::PI;

pub const TABLE_SIZE: usize = 2048;

pub fn build_wavetable(kind: usize) -> Vec<f32> {
    (0..TABLE_SIZE)
        .map(|i| {
            let t = i as f32 / TABLE_SIZE as f32;
            match kind {
                0 => (2.0 * PI * t).sin(),
                1 => 2.0 * t - 1.0,
                2 => if t < 0.5 { 1.0 } else { -1.0 },
                3 => 1.0 - 4.0 * (t - 0.5).abs(),
                4 => if t < 0.25 { 1.0 } else { -1.0 },  // Pulse 25%
                5 => {                                      // Organ: harmonics 1–4
                    let h = |n: f32| (2.0 * PI * n * t).sin();
                    (h(1.0) + h(2.0)*0.5 + h(3.0)*0.25 + h(4.0)*0.125) / 1.875
                },
                _ => {                                      // Additive square: 3 odd harmonics
                    let v = (2.0*PI*t).sin()
                          + (6.0*PI*t).sin() / 3.0
                          + (10.0*PI*t).sin() / 5.0;
                    (v / 0.867).clamp(-1.0, 1.0)
                },
            }
        })
        .collect()
}