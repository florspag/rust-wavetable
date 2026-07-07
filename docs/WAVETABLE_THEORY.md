# Wavetable Synthesis — Theory

## Core Idea

A wavetable is a fixed-length buffer containing one cycle of a waveform, stored as floating-point samples. Instead of computing `sin(2π·f·t)` every sample (expensive), you compute it once, store the 2048 values, then *read* from that buffer at audio rate — a table lookup.

The table holds values in `[-1.0, 1.0]` and represents **one period**, normalized to the range `[0, 1)` on the time axis (`t = i / TABLE_SIZE`).

---

## Phase Accumulation

The oscillator tracks a **phase** in `[0.0, 1.0)`:

```text
phase_inc = freq / sample_rate
phase = (phase + phase_inc) % 1.0
```

Each call to `tick()` advances the phase by `phase_inc`. At `440 Hz` / `44100 Hz` sample rate, that is `0.00997...` per sample — so the table wraps roughly 440 times per second, producing 440 Hz.

The table index is then: `pos = phase × TABLE_SIZE`.

This is called a **phasor** — a normalized ramp from 0→1 that drives the table read.

The **frequency slider** in the browser GUI directly sets this value: dragging it changes `phase_inc`, which changes how fast the table cycles and therefore the pitch of the waveform. Unlike pressing a piano key, the slider uses `change_freq` which updates `phase_inc` without resetting the phase, so pitch slides smoothly without a click.

---

## Windowed-Sinc Interpolation

The table has 2048 entries, but the read position `pos` is a float. The oscillator uses an **8-tap Blackman-windowed sinc** kernel to reconstruct the continuous waveform — the theoretically ideal approach for a band-limited signal.

### The kernel

The sinc function `sin(π·x) / (π·x)` is the perfect reconstruction filter: it evaluates to 1 at `x = 0` and exactly 0 at every other integer, so adjacent samples do not bleed into one another. Alone it has infinite extent; the Blackman window truncates it to `2·L` samples while minimising side-lobe energy:

```rust
const SINC_L: isize = 4; // 4 samples each side → 8 taps total

fn sinc_kernel(x: f32) -> f32 {
    if x.abs() < 1e-6 { return 1.0; }
    let pix = PI * x;
    let window = 0.42 + 0.5 * (PI * x / SINC_L as f32).cos()
                      + 0.08 * (2.0 * PI * x / SINC_L as f32).cos();
    pix.sin() / pix * window
}
```

The Blackman window reaches exactly 0 at `|x| = L`, so the first and last taps fade cleanly to zero.

### Pre-computed look-up table with row interpolation

Calling `sin()` eight times per audio sample (× 8 voices = 64 calls per sample) is wasteful because the kernel shape only depends on the fractional position `t ∈ [0, 1)`. The kernel is pre-computed once at startup into a global table keyed by a quantized `t`.

The table has **`SINC_TABLE_SIZE + 1` rows** — the extra row at `t = 1.0` lets the interpolation step safely read `row[qi + 1]` at the top of the range without a bounds check:

```rust
const SINC_TABLE_SIZE: usize = 512;  // fractional subdivisions
static SINC_TABLE: OnceLock<Vec<[f32; SINC_TAPS]>> = OnceLock::new();

fn build_sinc_table() -> Vec<[f32; SINC_TAPS]> {
    (0..=SINC_TABLE_SIZE).map(|qi| {          // 513 rows
        let t = qi as f32 / SINC_TABLE_SIZE as f32;
        let mut weights = [0.0f32; SINC_TAPS];
        for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {
            weights[j] = sinc_kernel(t - k as f32);
        }
        weights
    }).collect()
}
```

513 rows × 8 weights × 4 bytes = **~16 KB** — fits in L1 cache. `OnceLock` ensures the table is built exactly once and shared across all 8 voices.

### How `tick()` uses it

```rust
let frac_idx = t * SINC_TABLE_SIZE as f32;
let qi    = frac_idx as usize;   // row below  — always < SINC_TABLE_SIZE since t < 1.0
let alpha = frac_idx.fract();    // sub-row fraction ∈ [0, 1)
let w0    = &SINC_TABLE[qi];
let w1    = &SINC_TABLE[qi + 1]; // safe: table has SINC_TABLE_SIZE + 1 rows

let mut s = 0.0f32;
for (j, k) in (-(SINC_L - 1)..=SINC_L).enumerate() {  // k = −3 … +4
    let idx = ((i0 as isize + k).rem_euclid(TABLE_SIZE as isize)) as usize;
    s += table[idx] * (w0[j] + alpha * (w1[j] - w0[j]));
}
```

Snapping to the nearest row would introduce a step error of up to `1 / (2 × SINC_TABLE_SIZE) ≈ 0.001` in the kernel argument. Interpolating between rows reduces this to the floating-point rounding error of the `frac_idx.fract()` computation — effectively zero. The extra cost is 8 multiply-adds for the weight lerp, still far cheaper than 8 `sin()` calls.

### Comparison of interpolation methods

| Method | Taps | Continuity | HF accuracy | Cost per sample |
| ------ | ---- | ---------- | ----------- | --------------- |
| Truncate | 1 | C−1 (discontinuous) | Poor — steps alias | 0 extra ops |
| Linear | 2 | C0 (value only) | ~−6 dB/oct rolloff | 1 multiply |
| Catmull-Rom | 4 | C1 (value + slope) | Good at mid pitches | 4 multiplies, no trig |
| Blackman-sinc + LUT (snap) | 8 | C∞ (ideal band-limit) | Excellent | 8 multiplies, no trig |
| Blackman-sinc + LUT (lerp) | 8 | C∞ + sub-row smooth | Ideal | 16 multiplies, no trig |

---

## Aliasing — The Key Problem

A naive square or sawtooth wave contains **infinite harmonics**. When those harmonics exceed Nyquist (`sr / 2`), they fold back into audible frequencies as **aliasing** — a harsh, inharmonic distortion.

The solution used here is **multi-table mip-mapping**: each waveform is pre-built at 10 harmonic densities. `set_freq` picks the richest table whose highest harmonic still fits below Nyquist.

---

## Multi-Table Mip-Mapping

### Mip levels

Ten tables are built per waveform at startup, covering the full piano range:

| Level | Max harmonics | Aliasing-free up to |
| ----- | ------------- | ------------------- |
| 0 | 512 | ~43 Hz |
| 1 | 256 | ~86 Hz |
| 2 | 128 | ~172 Hz |
| 3 | 64 | ~344 Hz |
| 4 | 32 | ~689 Hz |
| 5 | 16 | ~1 378 Hz |
| 6 | 8 | ~2 756 Hz |
| 7 | 4 | ~5 512 Hz |
| 8 | 2 | ~11 025 Hz |
| 9 | 1 | any pitch |

### Level selection

On every `set_freq` or `change_freq` call the oscillator computes:

```rust
let needed = ((sr * 0.5) / freq) as usize;   // harmonics that fit below Nyquist
// MIP_MAX_HARMONICS = [512, 256, 128, 64, 32, 16, 8, 4, 2, 1]
let level = MIP_MAX_HARMONICS.iter()
    .position(|&h| h <= needed)
    .unwrap_or(MIP_LEVELS - 1);
```

Searching a descending list for the first entry `≤ needed` gives the level with the **most harmonics that still avoids aliasing**.  At A4 (440 Hz, sr = 44100): `needed = 50` → level 4 (32 harmonics; highest = 32 × 440 = 14 080 Hz < 22 050 ✓).

### Bandlimited table generation

Instead of the closed-form formulas used in naive wavetable synths (e.g. `2t − 1`), each table is built by **additive Fourier synthesis** — summing only the harmonics that fit in that level — then peak-normalised to `[-1, 1]`:

| Waveform | Fourier series |
| -------- | -------------- |
| Sawtooth | `−(2/π) Σ_{k=1}^{N} sin(2πkt)/k` |
| Square | `(4/π) Σ_{k odd} sin(2πkt)/k` |
| Triangle | `−(8/π²) Σ_{k odd} cos(2πkt)/k²` |
| Pulse 25% | `(2d−1) + Σ (4sin(πkd)/(πk)) cos(2πkt−πkd)` |

Sine is already band-limited (one harmonic), so it reuses the same table at every level. Organ and Additive contain ≤5 harmonics, so higher-level tables simply omit the harmonics that would alias.

### Global OnceLock

All 7 × 10 = 70 tables (~560 KB) are stored in a single `static MIP_TABLES: OnceLock<…>` and built once on the first `Oscillator::new()` call. All 16 oscillators (8 voices × 2 osc) share the same allocation.

```rust
pub fn get_mip_tables() -> &'static Vec<Vec<Vec<f32>>> {
    MIP_TABLES.get_or_init(|| (0..7).map(build_mip_for_kind).collect())
}
```

Custom waveforms (slot 7) are not part of the mip stack — they are stored per-oscillator in `custom_table: Vec<f32>` since they change at runtime.

---

## Waveforms

Seven built-in waveforms are generated at startup via bandlimited Fourier synthesis (see Multi-Table Mip-Mapping above); an eighth slot holds a user-drawn custom table.

| `kind` | Name | Fourier series (harmonics capped at Nyquist) | Character |
| ------ | ---- | -------------------------------------------- | --------- |
| `0` | Sine | `sin(2π·t)` (single harmonic — no aliasing) | Pure tone |
| `1` | Sawtooth | `−(2/π) Σ_{k=1}^{N} sin(2πkt)/k` | All harmonics `1/n` — bright, buzzy |
| `2` | Square | `(4/π) Σ_{k odd} sin(2πkt)/k` | Odd harmonics `1/n` — hollow, reedy |
| `3` | Triangle | `−(8/π²) Σ_{k odd} cos(2πkt)/k²` | Odd harmonics `1/n²` — soft, flute-like |
| `4` | Pulse | `(2d−1) + Σ (4sin(πkd)/(πk)) cos(2πkt−πkd)`, d = 0.25 | Thin, nasal |
| `5` | Organ | Harmonics 1–4: `sin(2πt) + ½sin(4πt) + ¼sin(6πt) + ⅛sin(8πt)` | Warm, Hammond-style |
| `6` | Additive | `sin(2πt) + sin(6πt)/3 + sin(10πt)/5` | Soft square approximation |
| `7` | Custom | User-drawn in the browser, resampled to 2048 samples | Anything |

Triangle falls off as `1/n²` so it aliases far less than saw or square at any given mip level. The Organ and Additive waveforms use only a small fixed set of harmonics, so their higher mip levels simply silence the harmonics that would alias.

### Custom wavetable

Each oscillator has its own independent draw canvas, revealed when that oscillator's **Custom** button is selected. The user drags to sculpt one full cycle; on release the browser resamples the drawing to 2048 `Float32` samples and routes it to the correct WASM method — `load_osc1_custom_table` or `load_osc2_custom_table`:

```text
JS Float32Array  →  wasm-bindgen  →  &[f32]  →  resample to TABLE_SIZE  →  osc1.custom_table
                                                                        or  osc2.custom_table
```

Both canvases can be visible simultaneously when both oscillators are set to Custom, so Osc 1 and Osc 2 can each hold a distinct user-drawn waveform at the same time. The Rust side resamples the input to exactly TABLE_SIZE entries using linear interpolation, so any input length works.

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

```text
y[n] = b0·x[n] + b1·x[n−1] + b2·x[n−2] − a1·y[n−1] − a2·y[n−2]
```

---

## Polyphony and Voice Management

The synth maintains **8 voices** running in parallel. Each voice owns its own two `Oscillator` instances and an `Adsr`; the `Filter` is shared and applied to the final mix.

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

```text
mixed = tanh(sum × 0.3)
```

The `0.3` scale factor means a single voice (`0.3 × 1.0 = 0.3`) passes through almost linearly (`tanh(0.3) ≈ 0.291`), while 8 voices in phase (`tanh(2.4) ≈ 0.984`) saturate gracefully instead of clipping hard.

---

## Dual Oscillators per Voice

Each `Voice` contains two independent `Oscillator` instances. Each oscillator has its own wavetable selection (set via `set_waveform` for osc1, `set_osc2_waveform` for osc2) and is pitched symmetrically around the played note using **detune** (in cents, 0–100):

```text
osc1 frequency = freq × 2^(+cents / 2400)   ← slightly sharp
osc2 frequency = freq ÷ 2^(+cents / 2400)   ← equally flat
```

Splitting the detune symmetrically keeps the perceived centre pitch locked to the played note. At 0 cents both oscillators are in perfect unison; at higher values they drift in and out of phase with each other, producing the classic **chorus / supersaw beating** effect.

The two outputs are blended with **Osc2 Mix** (0–1) and normalised so total amplitude stays constant regardless of mix level:

```text
osc_out = (osc1 + osc2 × mix) / (1 + mix)
```

| mix | result |
| --- | ------ |
| 0.0 | osc1 only (no detuned oscillator heard) |
| 0.5 | osc1 at ⅔ level + osc2 at ⅓ level |
| 1.0 | osc1 and osc2 at equal level (÷ 2) |

The normalization denominator `(1 + mix)` ensures peak amplitude never increases when osc2 is blended in. With 8 voices × 2 oscillators the tanh soft-clipper in the final mix stage still handles any transient excess.

### Waveform selection

The browser GUI shows two labelled rows of waveform buttons — **Osc 1** (blue highlight) and **Osc 2** (green highlight). Selecting different shapes in each row layers two timbres; classic combinations include Saw + Square for a dense analogue texture, or Sine + Organ for a softer layered pad. When a row selects **Custom**, a dedicated draw canvas appears below for that oscillator; each oscillator stores its waveform in its own `custom_table` field (`osc1.custom_table` / `osc2.custom_table`), so both can hold different user-drawn shapes simultaneously.

---

## LFO

A single **LFO (Low-Frequency Oscillator)** runs in parallel with the audio voices and modulates one target at a time. Internally it is a plain `Oscillator` instance — the same struct used for audio voices — running at the audio sample rate but at a very low `phase_inc`. Waveform 0 (sine) is used, giving the smoothest modulation shape.

### Targets

| Target | What is modulated | Depth range (depth = 1.0) |
| ------ | ----------------- | ------------------------- |
| **Pitch** | `phase_inc` of every active osc1 and osc2 via `pitch_scale` | ±2 semitones |
| **Cutoff** | Filter cutoff around the user-set base frequency | ±3 octaves |
| **Mix** | Osc2 blend level (`osc2_mix + lfo × depth`, clamped to [0, 1]) | full swing |

### Pitch modulation

The `Oscillator` struct has a `pitch_scale: f32` field (default `1.0`) that is multiplied into the phase advance each sample:

```rust
self.phase = (self.phase + self.phase_inc * self.pitch_scale) % 1.0;
```

`wasm_synth` computes the scale from the LFO output and sets it on all active oscillators every tick:

```rust
let pitch_scale = 2f32.powf(lfo_out * lfo_depth * 2.0 / 12.0);
```

At `depth = 0` the exponent is always zero → `pitch_scale = 1.0` → no modulation. At `depth = 1.0` and `lfo_out = ±1` the scale is `2^(±2/12) ≈ ±12 %` — exactly ±2 semitones of vibrato.

### Filter cutoff modulation

`set_filter_cutoff` stores the user-set value in `base_cutoff` separately from what the filter currently uses. Each sample `set_cutoff` is called with the modulated frequency:

```rust
let cutoff = (base_cutoff * 2f32.powf(lfo_out * lfo_depth * 3.0)).clamp(20.0, 20_000.0);
self.filter.set_cutoff(cutoff);
```

The exponential scale (`2^(3×depth)` → up to 8×) gives the characteristic logarithmic filter sweep heard on classic synthesisers.

### Target switching

Changing targets immediately restores the parameter that was being modulated: pitch_scale resets to 1.0 for all oscillators, and the filter is reset to `base_cutoff`. This prevents stuck offsets when the user changes the target while the LFO is mid-cycle.

---

## Signal Flow

```text
Piano key / MIDI  →  note_on(freq)       Frequency slider  →  change_freq(freq)
                        ↓                                           ↓
               Voice[0..8] allocated or stolen          updates both osc phase_incs
               each voice: osc1 + osc2 + Adsr           + selects mip level for freq
                        ↓                                      (no phase reset)
  LFO (sine oscillator, 0.01–20 Hz) — one target at a time:
    Pitch  →  pitch_scale = 2^(lfo × depth × 2/12)   → applied to osc phase advance
    Cutoff →  base_cutoff × 2^(lfo × depth × 3)      → filter.set_cutoff() each sample
    Mix    →  (osc2_mix + lfo × depth).clamp(0, 1)   → effective_mix in voice blend
                        ↓
  per-voice tick():
    osc1: freq × detune_ratio  →  mip table[waveform1][level]  →  sinc LUT × pitch_scale  →  s1
    osc2: freq ÷ detune_ratio  →  mip table[waveform2][level]  →  sinc LUT × pitch_scale  →  s2
    osc_out = (s1 + s2 × effective_mix) / (1 + effective_mix)   ← amplitude-normalised blend
    osc_out × ADSR envelope level
                        ↓
       sum all 8 voices
                        ↓
    tanh(sum × 0.3)   ← soft clip / normalise
                        ↓
    biquad filter (LP / HP / BP, base_cutoff [± LFO], Q)
                        ↓
    ScriptProcessorNode / AudioWorklet → speakers
```

---

## Where to Go Next

1. **Unison / supersaw** — spawn N detuned oscillators per voice (typically 4–8) with randomised initial phases and spread across the stereo field; gives the dense "supersaw" lead sound found in classic analogue polysynths.
2. **Waveform morphing** — crossfade between two tables by blending `table[a]` and `table[b]` samples for smooth timbral evolution; morph position could be an LFO target.
3. **Per-voice filter** — move the `Filter` inside each `Voice` for independent cutoff envelopes; stereo panning per voice.
