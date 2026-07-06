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
                _ => 1.0 - 4.0 * (t - 0.5).abs(),
            }
        })
        .collect()
}