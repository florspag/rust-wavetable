
#[derive(PartialEq, Clone, Copy)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}


pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_rate: f32,
    decay_rate: f32,
    sustain: f32,
    release_rate: f32,
    sample_rate: f32,
}

impl Adsr {
    pub fn new(sr: f32) -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_rate: 1.0 / (0.01 * sr),
            decay_rate: 1.0 / (0.15 * sr),
            sustain: 0.7,
            release_rate: 1.0 / (0.4 * sr),
            sample_rate: sr,
        }
    }

    pub fn set_attack(&mut self, secs: f32) {
        self.attack_rate = 1.0 / (secs.max(0.001) * self.sample_rate);
    }

    pub fn set_decay(&mut self, secs: f32) {
        self.decay_rate = 1.0 / (secs.max(0.001) * self.sample_rate);
    }

    pub fn set_sustain(&mut self, level: f32) {
        self.sustain = level.clamp(0.0, 1.0);
    }

    pub fn set_release(&mut self, secs: f32) {
        self.release_rate = 1.0 / (secs.max(0.001) * self.sample_rate);
    }

    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => 0.0,
            Stage::Attack => {
                self.level = (self.level + self.attack_rate).min(1.0);
                if self.level >= 1.0 {
                    self.stage = Stage::Decay;
                }
                self.level
            }
            Stage::Decay => {
                self.level = (self.level - self.decay_rate).max(self.sustain);
                if self.level <= self.sustain {
                    self.stage = Stage::Sustain;
                }
                self.level
            }
            Stage::Sustain => self.level,
            Stage::Release => {
                self.level = (self.level - self.release_rate).max(0.0);
                if self.level <= 0.0 {
                    self.stage = Stage::Idle;
                }
                self.level
            }
        }
    }
}