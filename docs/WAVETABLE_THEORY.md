# Wavetable Synthesis — Theory

## Core Idea

A wavetable is a fixed-length buffer containing one cycle of a waveform, stored as floating-point samples. Instead of computing `sin(2π·f·t)` every sample (expensive), you compute it once, store the 2048 values, then *read* from that buffer at audio rate — a table lookup.

The table holds values in `[-1.0, 1.0]` and represents **one period**, normalized to the range `[0, 1)` on the time axis (`t = i / TABLE_SIZE`).

---

## Phase Accumulation

The oscillator tracks a **phase** in `[0.0, 1.0)`:

```
phase_inc = freq / sample_rate
phase = (phase + phase_inc) % 1.0
```

Each call to `tick()` advances the phase by `phase_inc`. At `440 Hz` / `44100 Hz` sample rate, that is `0.00997...` per sample — so the table wraps roughly 440 times per second, producing 440 Hz.

The table index is then: `pos = phase × TABLE_SIZE`.

This is called a **phasor** — a normalized ramp from 0→1 that drives the table read.

The **frequency slider** in the browser GUI directly sets this value: dragging it changes `phase_inc`, which changes how fast the table cycles and therefore the pitch of the waveform. Unlike pressing a piano key, the slider uses `change_freq` which updates `phase_inc` without resetting the phase, so pitch slides smoothly without a click.

---

## Linear Interpolation

The table has 2048 entries, but `pos` is a float. Truncating to an integer creates audible stepping artifacts. Linear interpolation fixes this:

```rust
let i0 = pos as usize % TABLE_SIZE;
let i1 = (i0 + 1) % TABLE_SIZE;
let frac = pos.fract();
let s = table[i0] + frac * (table[i1] - table[i0]);
```

`frac` is how far between `i0` and `i1` the true position is. This smooths the output at the cost of slight high-frequency rolloff.

Higher-quality alternatives:
- **Cubic (Hermite/Catmull-Rom)** — 4-point, less HF rolloff
- **Sinc interpolation** — theoretically ideal, computationally expensive

---

## Aliasing — The Key Problem

A naive square or sawtooth wave contains **infinite harmonics**. When those harmonics exceed Nyquist (`sr / 2`), they fold back into audible frequencies as **aliasing** — a harsh, inharmonic distortion.

The waveforms in `wavetable.rs` bake a single alias-prone shape into the table at fill time. This is fine at low pitches but degrades at high ones.

### The Fix: Bandlimited Wavetables

- Build multiple tables for different frequency ranges, each containing only the harmonics that fit below Nyquist for that range.
- At playback, select the table whose harmonic content matches the current pitch.
- This is how commercial synths (Serum, Vital, etc.) achieve clean high-frequency response.

Common techniques: **BLIT** (Bandlimited Impulse Train), **BLEP** (Bandlimited Step), **multi-table mip-mapping**.

---

## Waveforms

Seven built-in tables are generated at startup; an eighth slot holds a user-drawn custom table.

| `kind` | Name | Formula / method | Character |
| ------ | ---- | ---------------- | --------- |
| `0` | Sine | `sin(2π·t)` | Pure tone, fundamental only |
| `1` | Sawtooth | `2t − 1` | All harmonics `1/n` — bright, buzzy |
| `2` | Square | `±1` at 50% duty | Odd harmonics `1/n` — hollow, reedy |
| `3` | Triangle | `1 − 4·\|t − 0.5\|` | Odd harmonics `1/n²` — soft, flute-like |
| `4` | Pulse | `±1` at 25% duty | Odd + even harmonics — thin, nasal |
| `5` | Organ | Sum of harmonics 1–4 with `½, ¼, ⅛` weights | Warm, Hammond-style |
| `6` | Additive | Three odd harmonics: `sin + sin(3f)/3 + sin(5f)/5` | Bandlimited square approximation |
| `7` | Custom | User-drawn in the browser, resampled to 2048 samples | Anything |

Triangle falls off as `1/n²` so it aliases far less than saw or square. The Additive waveform pre-sums a few harmonics in the table — same principle as the bandlimited techniques below, just applied once at build time rather than per-pitch.

### Custom wavetable

The browser GUI provides a draw canvas (shown when **Custom** is selected). The user drags to sculpt one full cycle; on release the browser resamples the drawing to 2048 `Float32` samples and sends them to the WASM synth via:

```
JS Float32Array  →  wasm-bindgen  →  &[f32]  →  resample to TABLE_SIZE  →  tables[7]
```

The Rust side resamples the input to exactly TABLE_SIZE entries using the same linear interpolation used during playback, so any input length works.

---

## ADSR Envelope

The envelope shapes the amplitude of each note over time across four stages:

```
Amplitude
  1.0 ┤    ●
      │   ╱ ╲
      │  ╱   ╲___________
  S   │ ╱              ╲
  0.0 ┼╱  A    D    S   R╲
      └──────────────────────▶ time
                ↑ note-off
```

| Stage | What happens |
| ------- | ----------- |
| **Attack** | Level rises from 0 → 1.0 over the attack time |
| **Decay** | Level falls from 1.0 → sustain level |
| **Sustain** | Level holds until note-off |
| **Release** | Level falls back to 0 after note-off |

Each rate is stored as `1 / (time_in_seconds × sample_rate)` — the amount the level changes per sample. This means shorter times produce larger rates and faster transitions.

In the browser GUI the ADSR canvas lets you drag control points directly on the envelope curve. The yellow handle controls both decay time (drag X) and sustain level (drag Y). A dashed vertical line marks the note-off point. Positions use a square-root scale so short times remain easy to grab.

---

## Biquad Filter

After the oscillator, the signal passes through a second-order IIR (biquad) filter implemented with the Audio EQ Cookbook coefficients. Three modes are available:

| Mode | What it does |
| ------- | ------------ |
| **LP** (Low-Pass) | Passes frequencies below the cutoff, attenuates above — rounds off harsh harmonics |
| **HP** (High-Pass) | Passes frequencies above the cutoff, attenuates below — thins out the sound |
| **BP** (Band-Pass) | Passes a band around the cutoff, attenuates both sides — nasal / telephone effect |

**Cutoff** sets the −3 dB transition frequency (20 Hz – 20 kHz, logarithmic).  
**Resonance** (Q) controls the peak at the cutoff: Q = 0.707 (Butterworth) is flat; higher Q creates a resonant peak used for classic synth sweep sounds.

The filter type cycles LP → HP → BP → LP with each click of the selector — matching the one-knob-per-function feel of hardware synthesisers.

The transfer function is computed via the Direct Form II transposed structure, which is numerically stable for audio-rate coefficients:

```
y[n] = b0·x[n] + b1·x[n−1] + b2·x[n−2] − a1·y[n−1] − a2·y[n−2]
```

---

## Polyphony and Voice Management

The synth maintains **8 voices** running in parallel. Each voice owns its own `Oscillator` and `Adsr`; the `Filter` is shared and applied to the final mix.

### Voice allocation

When a note is played:

1. If any voice is silent (envelope inactive), use it.
2. Otherwise **steal** the oldest voice — the one with the lowest `age` counter. The age counter increments on every `note_on`, so the voice that has been playing longest is always the steal candidate.

```rust
let idx = voices.iter().position(|v| !v.env.is_active())
    .unwrap_or_else(|| voices.iter().enumerate()
        .min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap());
```

### note_off matching

`note_off(freq)` finds all voices whose frequency is within ±1 Hz of the released note, then releases the **most recently triggered** one (`max_by_key(age)`). This handles the common case where the same key is re-pressed before it finishes releasing — the older, decaying instance is left to tail out naturally.

### Soft clipping

With 8 voices simultaneously active, the raw sum can exceed ±1.0. A `tanh` limiter is applied after mixing:

```
mixed = tanh(sum × 0.3)
```

The `0.3` scale factor means a single voice (`0.3 × 1.0 = 0.3`) passes through almost linearly (`tanh(0.3) ≈ 0.291`), while 8 voices in phase (`tanh(2.4) ≈ 0.984`) saturate gracefully instead of clipping hard.

---

## Signal Flow

```
Piano key / MIDI  →  note_on(freq)       Frequency slider  →  change_freq(freq)
                        ↓                                           ↓
               Voice[0..8] allocated or stolen          updates phase_inc only
               each voice: Oscillator + Adsr                  (no phase reset,
                        ↓                                      no click)
  per-voice tick():
    phase → table lookup → linear interp → raw sample
    raw sample × ADSR envelope level
                        ↓
       sum all 8 voices
                        ↓
    tanh(sum × 0.3)   ← soft clip / normalise
                        ↓
    biquad filter (LP / HP / BP, cutoff, Q)
                        ↓
    ScriptProcessorNode / AudioWorklet → speakers
```

---

## Where to Go Next

1. **Multi-table mip-mapping** — generate one table per octave with harmonics capped at Nyquist for that octave, select the right table in `set_freq`.
2. **Cubic interpolation** — replace the linear lerp with 4-point Hermite for better HF accuracy.
3. **Waveform morphing** — crossfade between two tables by blending `table[a]` and `table[b]` samples for smooth timbral evolution.
4. **Unison / detune** — run two or more oscillators per voice with slight pitch offsets and mix them, classic for fat synth sounds.
5. **Per-voice filter** — move the `Filter` inside each `Voice` for independent cutoff envelopes; stereo panning per voice.
