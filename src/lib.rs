pub mod wavetable;
pub mod oscillator;
pub mod adsr;

#[cfg(target_arch = "wasm32")]
pub mod wasm_synth;
