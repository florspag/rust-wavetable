use std::f32::consts::PI;

pub enum FilterType { LowPass, HighPass, BandPass }

pub struct Filter {
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
    x1: f32, x2: f32,
    y1: f32, y2: f32,
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    kind: FilterType,
}

impl Filter {
    pub fn new(sr: f32) -> Self {
        let mut f = Self {
            b0: 1.0, b1: 0.0, b2: 0.0,
            a1: 0.0, a2: 0.0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate: sr,
            cutoff: sr * 0.49, // fully open
            resonance: 0.707,  // Butterworth — no resonance peak
            kind: FilterType::LowPass,
        };
        f.update();
        f
    }

    pub fn set_cutoff(&mut self, hz: f32) {
        self.cutoff = hz.clamp(20.0, self.sample_rate * 0.49);
        self.update();
    }

    pub fn set_resonance(&mut self, q: f32) {
        self.resonance = q.clamp(0.1, 20.0);
        self.update();
    }

    pub fn set_type(&mut self, t: u32) {
        self.kind = match t {
            1 => FilterType::HighPass,
            2 => FilterType::BandPass,
            _ => FilterType::LowPass,
        };
        self.update();
    }

    fn update(&mut self) {
        let w0 = 2.0 * PI * self.cutoff / self.sample_rate;
        let sin_w0 = w0.sin();
        let cos_w0 = w0.cos();
        let alpha = sin_w0 / (2.0 * self.resonance);

        let (b0, b1, b2, a0, a1, a2) = match self.kind {
            FilterType::LowPass => (
                (1.0 - cos_w0) / 2.0,
                1.0 - cos_w0,
                (1.0 - cos_w0) / 2.0,
                1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha,
            ),
            FilterType::HighPass => (
                (1.0 + cos_w0) / 2.0,
                -(1.0 + cos_w0),
                (1.0 + cos_w0) / 2.0,
                1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha,
            ),
            FilterType::BandPass => (
                sin_w0 / 2.0,
                0.0,
                -sin_w0 / 2.0,
                1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha,
            ),
        };

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
              - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }
}
