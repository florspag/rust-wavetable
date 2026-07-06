pub mod wavetable;
pub mod oscillator;
pub mod adsr;
pub mod filter;

#[cfg(target_arch = "wasm32")]
pub mod wasm_synth;
