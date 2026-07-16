pub mod wavetable;
pub mod oscillator;
pub mod adsr;
pub mod filter;
pub mod reverb;
pub mod flanger;

#[cfg(target_arch = "wasm32")]
pub mod wasm_synth;
