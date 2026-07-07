# Synth Architecture

Signal flow from input events through 8 polyphonic voices to stereo output.

```mermaid
flowchart TD
    MIDI["MIDI / Piano\nnote_on(freq)"]
    FREQ["Freq Slider\nchange_freq(freq)"]
    ALLOC["Voice Allocator\n8 voices\nprefer silent → steal oldest (min age)"]

    MIDI --> ALLOC
    FREQ --> ALLOC

    subgraph VOICE ["  Voice  ×8  —  each runs independently  "]
        direction TB
        OSC1["Osc 1\n1–8 unison copies\ncents_i = detune × (i/(n−1) − 0.5)\nphase_i = i / count\nmip table + sinc LUT"]
        OSC2["Osc 2\nfreq / detune_ratio\nown waveform + mip level"]
        BLEND["Blend\n(osc1 + osc2 × mix) / (1 + mix)"]
        ENV["ADSR Envelope\n× env.tick()"]
        FLT["Biquad Filter\nLP / HP / BP\nper-voice state — no bleed on steal"]
        PAN["Stereo Pan\nangle = (pan + 1) × π/4\npan_l = cos(angle)   pan_r = sin(angle)"]

        OSC1 --> BLEND
        OSC2 --> BLEND
        BLEND --> ENV
        ENV --> FLT
        FLT --> PAN
    end

    ALLOC --> VOICE

    LFO["LFO\nOscillator @ 0.01–20 Hz\nsinusoidal · global"]
    ENV_SRC["Env (per-voice)\ncurrent ADSR level\n0 → 1 (unipolar)"]
    MOD["Mod Matrix — 4 slots\nsource × dest × amount (±1)\nΣ contributions per dest"]
    SPREAD["Spread knob\n0 = mono\n1 = voice 0 hard-left, voice 7 hard-right"]

    LFO -. "lfo_val (−1…+1)" .-> MOD
    ENV_SRC -. "env_val (0…+1)\nper-voice" .-> MOD
    MOD -. "pitch_scale = 2^(Σ × 2/12)" .-> OSC1
    MOD -. "pitch_scale" .-> OSC2
    MOD -. "mod_cutoff = base×2^(Σ×3)" .-> FLT
    MOD -. "mod_res = base_res + Σ×10" .-> FLT
    MOD -. "effective_mix = osc2_mix + Σ" .-> BLEND
    SPREAD -. "pan = voice_pos × spread" .-> PAN

    SUM["Stereo Sum\nΣ(filtered × pan_l)   Σ(filtered × pan_r)"]
    CLIP["Soft Clip\ntanh(sum × 0.3)"]
    OUTL["get_left()"]
    OUTR["get_right()"]
    SPK["Speakers\nScriptProcessorNode — 256-sample stereo buffer"]

    PAN --> SUM
    SUM --> CLIP
    CLIP --> OUTL
    CLIP --> OUTR
    OUTL --> SPK
    OUTR --> SPK

    classDef osc1   fill:#1e3a5f,stroke:#89b4fa,color:#89b4fa
    classDef osc2   fill:#1a2e1a,stroke:#a6e3a1,color:#a6e3a1
    classDef blend  fill:#1e1e2e,stroke:#45475a,color:#cdd6f4
    classDef env    fill:#2e2a1a,stroke:#f9e2af,color:#f9e2af
    classDef flt    fill:#2a1a2e,stroke:#cba6f7,color:#cba6f7
    classDef pan    fill:#1a2a2e,stroke:#74c7ec,color:#74c7ec
    classDef lfo    fill:#2a1a3e,stroke:#cba6f7,color:#cba6f7
    classDef output fill:#181825,stroke:#a6e3a1,color:#a6e3a1
    classDef ctrl   fill:#181825,stroke:#45475a,color:#6c7086
    classDef io     fill:#1e1e2e,stroke:#45475a,color:#cdd6f4

    class OSC1 osc1
    class OSC2 osc2
    class BLEND,SUM,CLIP blend
    class ENV env
    class FLT flt
    class PAN pan
    class LFO,ENV_SRC,MOD,SPREAD lfo
    class OUTL,OUTR,SPK output
    class MIDI,FREQ ctrl
    class ALLOC io
```

## Block reference

| Block | Color | Key parameters | Source |
| ----- | ----- | -------------- | ------ |
| **Voice Allocator** | — | 8 slots, `voice_counter` age, `auto_release` 0.5 s | `wasm_synth.rs` |
| **Osc 1** | blue | waveform, unison count 1–8, detune 0–100 ¢, mip level | `oscillator.rs` |
| **Osc 2** | green | waveform, freq ÷ `detune_ratio` | `oscillator.rs` |
| **Blend** | — | `osc2_mix` 0–1; normalised so amplitude is constant | `wasm_synth.rs` |
| **ADSR** | yellow | attack / decay / sustain / release; rate = 1/(time × sr) | `adsr.rs` |
| **Biquad Filter** | purple | LP / HP / BP; `base_cutoff`, `base_resonance`; Direct Form II T | `filter.rs` |
| **Stereo Pan** | cyan | equal-power cos/sin; `spread` distributes 8 voices −1 → +1 | `wasm_synth.rs` |
| **LFO** | purple (dashed) | reuses `Oscillator` at very low `phase_inc`; sine; global | `wasm_synth.rs` |
| **Env (source)** | purple (dashed) | per-voice ADSR level (0–1) sampled once per tick; same value used for amplitude | `wasm_synth.rs` |
| **Mod Matrix** | purple (dashed) | 4 slots: source × dest × amount (±1); Σ contributions per dest; recomputed every sample, no cleanup needed | `wasm_synth.rs` |
| **Soft Clip** | — | `tanh(sum × 0.3)`; single voice passes almost linear, 8 voices saturate gracefully | `wasm_synth.rs` |

For the theory behind each block see [WAVETABLE_THEORY.md](WAVETABLE_THEORY.md).
