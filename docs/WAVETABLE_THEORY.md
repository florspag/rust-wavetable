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

Each voice has its own independent biquad filter — a second-order IIR implemented with Audio EQ Cookbook coefficients. Keeping the filter per-voice means:

- Voices don't bleed shared filter state into one another (a stolen voice's filter history does not colour the new note).
- The LFO can modulate the cutoff on every active voice simultaneously without a single shared biquad accumulating different per-voice history.

Three modes are available:

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

Global parameter changes (`set_filter_cutoff`, `set_filter_resonance`, `set_filter_type`) are propagated to all 8 voice filters immediately. The values are also stored as `base_cutoff`, `base_resonance`, and `base_filter_type` in `Synth` so that a stolen voice's filter can be re-initialised to the correct settings when its note slot is reused.

---

## Polyphony and Voice Management

The synth maintains **8 voices** running in parallel. Each voice owns two `Oscillator` instances, an `Adsr`, and a `Filter`; the filtered output is then panned into a stereo mix.

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

## Modulation Matrix

The synth has a **4-slot modulation matrix**. Each slot independently connects a **source** signal to a **destination** parameter, with a bipolar **amount** (−1 to +1). Multiple slots can target the same destination — their contributions are summed before being applied.

### Sources

| Source | Signal range | Description |
| ------ | ------------ | ----------- |
| **LFO 1** | −1 to +1 (bipolar) | `Oscillator` running at 0.01–20 Hz, global (shared across all voices); waveform selectable (sin / saw / sqr / tri / pls); default 1 Hz sine |
| **LFO 2** | −1 to +1 (bipolar) | Independent second `Oscillator` at 0.01–20 Hz; same selectable waveforms; default 0.25 Hz sine — allows two independent modulation shapes and rates simultaneously |
| **Env** | 0 to +1 (unipolar) | The current ADSR level of each individual voice — different per voice |

Both LFOs reuse the same `Oscillator` struct used for audio, running at the audio sample rate with a very low `phase_inc`. The waveform shape is selected independently for each LFO via `set_lfo_waveform` / `set_lfo2_waveform`, which delegate to the standard `Oscillator::set_waveform` — giving access to any of the five non-custom waveforms (0 = sine for smooth modulation, 1 = sawtooth for rising ramps, 2 = square for hard step effects, 3 = triangle for linear sweep, 4 = pulse for asymmetric gates). Having two independent LFOs with different shapes lets you, for example, combine a sine vibrato on Pitch (LFO 1) with a square hard-gating on Cutoff (LFO 2) simultaneously.

The Env source makes modulation inherently polyphonic: a voice in its attack stage sweeps differently from one mid-sustain, so notes naturally feel independent.

### Destinations

| Dest | Base value stored in | Scale at amount = ±1 |
| ---- | -------------------- | -------------------- |
| **Pitch** | — (relative to current freq) | ±2 semitones |
| **Cutoff** | `base_cutoff` | ±3 octaves around base |
| **Resonance** | `base_resonance` | ±10 Q units |
| **Mix** | `osc2_mix` | ±1 full swing of osc2 blend |

### Per-voice computation

Every tick, each active voice independently accumulates the modulation contributions from all slots:

```rust
let env_val = v.env.tick();  // advance envelope once; use value for both amp and mod

let (mut pitch_mod, mut cutoff_mod, mut res_mod, mut mix_mod) = (0f32, 0f32, 0f32, 0f32);
for slot in &mod_matrix {
    let src = match slot.source { 1 => lfo_val, 2 => env_val, 3 => lfo2_val, _ => continue };
    match slot.dest {
        0 => pitch_mod  += src * slot.amount,
        1 => cutoff_mod += src * slot.amount,
        2 => res_mod    += src * slot.amount,
        3 => mix_mod    += src * slot.amount,
        _ => {}
    }
}

let pitch_scale  = 2f32.powf(pitch_mod * 2.0 / 12.0);
let mod_cutoff   = (base_cutoff * 2f32.powf(cutoff_mod * 3.0)).clamp(20.0, 20_000.0);
let mod_res      = (base_resonance + res_mod * 10.0).clamp(0.1, 20.0);
let effective_mix = (osc2_mix + mix_mod).clamp(0.0, 1.0);
```

Because modulation is recomputed fresh every sample from the stored base values, clearing or changing a slot takes effect in one sample — no explicit cleanup of stuck values is needed.

### Pitch destination

`pitch_scale` is applied by multiplying into the oscillator's phase advance:

```rust
self.phase = (self.phase + self.phase_inc * self.pitch_scale) % 1.0;
```

At `pitch_mod = 0`, `pitch_scale = 2^0 = 1.0` — no modulation. At `pitch_mod = ±1`, `pitch_scale = 2^(±2/12) ≈ ±12 %` — exactly ±2 semitones of vibrato or pitch bend.

### Cutoff destination

`base_cutoff` stores the user-set value. The exponential scale gives logarithmic sweeps matching human pitch perception:

```rust
let mod_cutoff = (base_cutoff * 2f32.powf(cutoff_mod * 3.0)).clamp(20.0, 20_000.0);
```

`2^3 = 8×` at `cutoff_mod = +1` — three full octaves upward. Negative contributions sweep downward symmetrically. When no slot targets Cutoff, `cutoff_mod = 0` and `mod_cutoff = base_cutoff * 1.0 = base_cutoff` exactly.

---

## Unison / Supersaw

Each voice can run up to **8 detuned copies** of Osc 1 simultaneously. This is the technique behind the "supersaw" sound: multiple slightly flat/sharp oscillators beating against each other produce a thick, animated timbre that a single oscillator cannot.

### Oscillator count

The **Unison** count selector (1–8) controls how many copies are active per voice. At count = 1 the feature is off; the voice sounds exactly as before. At count = 8 you get the full supersaw spread.

### Detune distribution

The active copies are spread evenly from −½D to +½D cents, where D is the **Detune** knob value (0–100 cents):

```text
cents_i = D × (i / (count − 1) − 0.5)   for i = 0 … count − 1
copy_i freq = base_freq × 2^(cents_i / 1200)
```

At count = 1, `cents_i = 0` — no detuning. At count = 7 with D = 50 cents the copies are at −25, −16.7, −8.3, 0, +8.3, +16.7, +25 cents. One copy always lands at 0 cents (the centre) when count is odd.

### Phase staggering

When a note starts (`note_on`) all copies are given evenly-spaced initial phases instead of all starting at 0:

```rust
osc1s[i].set_freq(detuned_freq, sr);
osc1s[i].set_phase(i as f32 / count as f32);
```

Without this, copies at nearly the same frequency would constructively interfere on note-on, producing a harsh transient burst before drifting apart. Staggered phases ensure they are already spread across the cycle at attack time.

### Mixing

All active copies are summed and normalised by count before being blended with Osc 2:

```rust
let mut osc1_out = 0.0f32;
for osc in osc1s[..count].iter_mut() { osc1_out += osc.tick(); }
osc1_out /= count as f32;
```

Dividing by `count` keeps the amplitude constant regardless of how many copies are running, so adding more voices does not require rebalancing the output gain.

---

## Stereo Panning

Each voice is panned to a fixed position in the stereo field. The **Spread** knob (0–1) controls how far apart the voices are. At spread = 0 all voices are centred (mono); at spread = 1 voice 0 is hard-left and voice 7 is hard-right.

### Voice positions

Voices are distributed evenly across the spread range:

```rust
let raw = -1.0 + 2.0 * i as f32 / (VOICES - 1) as f32;  // -1 to +1
let pan  = raw * self.spread;                              // -spread to +spread
```

Voice 0 gets `pan = -spread`, voice 7 gets `pan = +spread`, and the six middle voices land at equal intervals between them.

### Equal-power panning

Pan position `pan ∈ [-1, 1]` is converted to a stereo angle and then to per-channel gains:

```rust
let angle = (pan + 1.0) * FRAC_PI_4;  // maps [-1, +1] → [0, π/2]
v.pan_l   = angle.cos();
v.pan_r   = angle.sin();
```

At centre (`pan = 0`, angle = π/4): `cos = sin = 1/√2 ≈ 0.707`, so the voice appears equally in both channels. The constant-power identity `cos²θ + sin²θ = 1` ensures the perceived loudness stays the same at any pan position.

The `pan_l` / `pan_r` values are precomputed in `Voice` fields and only recalculated when `set_spread` is called, so there is no per-sample overhead.

---

## Reverb

The reverb is a **Freeverb**-style algorithmic reverberator (Jezar at Dreampoint, 1997): a Schroeder–Moorer network of parallel comb filters followed by series allpass filters, running at the audio sample rate on the mixed stereo bus.

### Comb filter

A feedback comb filter creates a repeating echo with exponential decay:

```text
y[n] = x[n] + feedback × y[n − L]
```

where `L` is the delay length in samples. A one-pole low-pass filter sits inside the feedback loop to model high-frequency absorption by room boundaries:

```text
store[n] = y[n − L] × damp₂ + store[n−1] × damp₁       (damp₁ + damp₂ = 1)
y[n]     = x[n] + store[n] × feedback
```

`damp₁` is the LP coefficient: higher values roll off treble faster, producing a darker, more absorptive space. The transfer function of this damped comb is:

```text
H(z) = 1 / (1 − feedback × H_lp(z) × z^{−L})
where H_lp(z) = damp₂ / (1 − damp₁ z^{−1})
```

The comb adds resonant peaks at multiples of `sr / L` Hz. Eight combs with different prime-related lengths are summed to spread these peaks across the spectrum and reduce metallic coloration.

### Allpass filter

The Schroeder allpass diffuses the signal in time without altering its spectrum (flat magnitude response, `|H| = 1` for all frequencies):

```text
y[n] = −x[n] + x[n − L] + 0.5 × y[n − L]
```

The all-pass property can be verified: writing in the z-domain gives `H(z) = (z^{−L} − 0.5) / (1 − 0.5 z^{−L})` — numerator and denominator are conjugate-reciprocal, so `|H(e^{jω})| = 1`. In the time domain it smears transients and randomizes inter-sample phase relationships, breaking up the flutter echoes left by the comb bank.

### Network topology

```text
input = (in_L + in_R) × FIXED_GAIN     ← 0.015 — scales down before feedback builds up

   ┌── comb_L[0] ──┐          ┌── comb_R[0] ──┐
   ├── comb_L[1] ──┤          ├── comb_R[1] ──┤
   │       ⋮       │  Σ→      │       ⋮       │  Σ→
   └── comb_L[7] ──┘  sumL    └── comb_R[7] ──┘  sumR

sumL → allpass_L[0] → allpass_L[1] → allpass_L[2] → allpass_L[3] → out_L
sumR → allpass_R[0] → allpass_R[1] → allpass_R[2] → allpass_R[3] → out_R
```

All 8 combs receive the same mono input; their outputs are summed independently for L and R. The four allpass filters then run in series on each sum.

### Delay lengths (tuned for 44 100 Hz)

The lengths are chosen so that no two are integer multiples of each other — coincident resonances would collapse the echo density. The right channel adds a fixed 23-sample offset for stereo decorrelation.

| Channel | Comb delays (samples) | Allpass delays |
| ------- | --------------------- | -------------- |
| **Left** | 1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617 | 556, 441, 341, 225 |
| **Right** | +23 to each left length | +23 to each left length |

### Parameter mapping

| UI knob | Internal effect |
| ------- | --------------- |
| **Room** (0–1) | `feedback = room × 0.28 + 0.70` → range [0.70, 0.98]; higher = longer tail |
| **Damp** (0–1) | `damp₁ = damp × 0.40` → [0, 0.40]; higher = darker, more absorptive |
| **Wet** (0–1) | `output = dry × (1 − wet) + reverb_out × wet`; 0 = bypass |

The `FIXED_GAIN = 0.015` is chosen so that with 8 comb filters summing into feedback ≈ 0.84 (Room = 0.5), the reverb output amplitude stays within ±1.

### Wet / dry mix

The reverb engine always runs (keeps the delay lines warm), and the mix is applied at output:

```rust
let dry = 1.0 - self.wet;
(in_l * dry + out_l * self.wet,
 in_r * dry + out_r * self.wet)
```

At `wet = 0` the signal passes through unchanged. As wet increases, the reverberated signal blends in while the direct signal fades proportionally, keeping the total perceived loudness roughly constant.

---

## Flanger

A flanger is a **modulated feedback comb filter**: the input is mixed with a slightly delayed, slightly modified copy of itself. As the delay length sweeps in time, the comb-filter notches sweep across the spectrum, producing the characteristic jet-plane whoosh.

### The delay line

A circular buffer of length `BUF_SIZE = 512` samples holds the signal history. On each sample the write pointer advances by one; a delayed read at position `write_ptr − d` gives the signal from `d` samples ago. When `d` is not an integer, **linear interpolation** between the two neighbouring samples recovers the fractional-delay value:

```text
i₀ = (write_ptr − ⌊d⌋)     mod BUF_SIZE   ← sample just past the target delay
i₁ = (write_ptr − ⌊d⌋ − 1) mod BUF_SIZE   ← one sample further back
frac = d − ⌊d⌋

tap = buf[i₀] × (1 − frac) + buf[i₁] × frac
```

### Feedback and write

Rather than writing the dry input directly, the signal stored in the buffer includes a portion of the already-delayed signal fed back into itself:

```text
buf[n] = x[n] + feedback × tap[n]
y[n]   = dry × x[n] + wet × tap[n]
```

where `dry = 1 − wet`.

### Transfer function (constant delay)

For a fixed delay of `D` samples, setting `z = e^{jω}` (the unit circle), the buffer signal `B(z)` satisfies:

```text
B(z) = X(z) + feedback × z^{−D} × B(z)
     ⟹  B(z) = X(z) / (1 − feedback × z^{−D})
```

The delayed tap at the output is `z^{−D} × B(z)`, so the overall transfer function is:

```text
H(z) = (1 − wet) + wet × z^{−D} / (1 − feedback × z^{−D})
     = [ (1 − wet) × (1 − feedback × z^{−D}) + wet × z^{−D} ]
       / (1 − feedback × z^{−D})
     = (1 − wet) + [wet − (1 − wet) × feedback] × z^{−D}
       / (1 − feedback × z^{−D})
```

### Comb pattern

With `feedback = 0` and `wet = 0.5`, the magnitude squared reduces to:

```text
|H(e^{jω})|² = ½ (1 + cos(ωD))
```

Notches (zeros) appear wherever `cos(ωD) = −1`, i.e. at:

```text
f_k = (2k + 1) × sr / (2D)    k = 0, 1, 2, …
```

Peaks appear at `f_k = k × sr / D`. The result is a **comb filter** whose teeth are spaced `sr / D` Hz apart. As the delay `D` sweeps, all teeth shift simultaneously — frequencies are swallowed and released in rapid succession.

For `D` sweeping between the implementation constants `CENTER ± depth × MAX_DEPTH`:

```text
CENTER          = 110 samples ≈ 2.5 ms
MAX_DEPTH       = 110 samples ≈ 2.5 ms
delay range     = [CENTER − depth × MAX_DEPTH,  CENTER + depth × MAX_DEPTH]
first notch     = sr / (2D) ∈ [~100 Hz, ∞)   (depth = 1, D at maximum)
```

Feedback `g > 0` narrows the notches and adds resonant peaks, intensifying the effect. The poles of `H(z)` are at `z = g^{1/D} e^{j 2πk/D}`, which stay inside the unit circle as long as `|g| < 1` — stability is guaranteed by clamping `feedback` to [0, 0.9].

### LFO sweep

The delay length is driven by a sinusoidal LFO:

```text
d(n) = CENTER + depth × MAX_DEPTH × sin(2π × rate × n / sr)
```

The right-channel LFO runs **90° ahead** of the left:

```text
d_R(n) = CENTER + depth × MAX_DEPTH × sin(2π × rate × n / sr + π/2)
        = CENTER + depth × MAX_DEPTH × cos(2π × rate × n / sr)
```

This quarter-period phase offset decorrelates the comb spectra of L and R: when the left channel has a notch at a given frequency the right channel is mid-sweep past it, producing a wide stereo image that rotates with the LFO.

### Flanger parameter mapping

| UI knob | Implementation |
| ------- | -------------- |
| **Wet** (0–1) | `dry = 1 − wet`; 0 = bypass, 1 = all flanger |
| **Rate** (0.05–8 Hz) | LFO frequency; logarithmic knob; sweep speed |
| **Depth** (0–1) | Scales `MAX_DEPTH_SAMPLES = 110`; 0 = static notch at CENTER, 1 = full ±2.5 ms sweep |
| **Feedback** (0–0.9) | Recirculation gain; higher = narrower, more resonant notch teeth |

---

## Signal Flow

```text
Piano key / MIDI  →  note_on(freq)       Frequency slider  →  change_freq(freq)
                        ↓                                           ↓
               Voice[0..8] allocated or stolen          updates both osc phase_incs
               each voice: osc1 + osc2 + Adsr + Filter + pan_l/pan_r
                        ↓
  Modulation matrix (4 slots, evaluated per-voice every sample):
    source: LFO 1 / LFO 2 (global, −1…+1) or Env (per-voice, 0…+1)
    Σ contributions → pitch_mod, cutoff_mod, res_mod, mix_mod
                        ↓
  per-voice tick():
    env_val = env.tick()  ← used as both amplitude shaper AND Env mod source
    pitch_scale  = 2^(pitch_mod × 2/12)
    mod_cutoff   = base_cutoff × 2^(cutoff_mod × 3)
    mod_res      = base_resonance + res_mod × 10
    effective_mix = (osc2_mix + mix_mod).clamp(0, 1)
    osc1[0..n]: freq × detune_ratio × unison_cents  →  mip table  →  sinc LUT × pitch_scale
    osc2:       freq ÷ detune_ratio                 →  mip table  →  sinc LUT × pitch_scale
    osc_out = (osc1_sum/n + osc2 × effective_mix) / (1 + effective_mix)
    osc_out × env_val
                        ↓
    biquad filter per-voice (LP / HP / BP, mod_cutoff, mod_res)
                        ↓
    filtered × pan_l  →  left_acc
    filtered × pan_r  →  right_acc
                        ↓
  sum across all 8 voices → left_acc, right_acc
                        ↓
  tanh(left_acc × 0.3), tanh(right_acc × 0.3)   ← soft clip each channel
                        ↓
  Flanger (modulated feedback comb, stereo):
    d(n) = CENTER + depth × MAX_DEPTH × sin(2π·rate·n/sr)
    d_R(n) = same but cos (90° ahead) ← stereo decorrelation
    tap = read_tap(buf, write_ptr, d(n))  ← linear interpolation
    buf[n] = raw[n] + feedback × tap
    output = (1−wet) × raw[n] + wet × tap
                        ↓
  Freeverb reverb (8 comb + 4 allpass per channel):
    input = (fl_l + fl_r) × 0.015
    out_L/R ← parallel comb bank → series allpass chain
    output = fl × (1 − wet) + reverb_out × wet
                        ↓
  ScriptProcessorNode (2 channels): get_left() / get_right() → speakers
```

---

## Where to Go Next

1. **Waveform morphing** — crossfade between two tables by blending `table[a]` and `table[b]` samples for smooth timbral evolution; morph position could be a mod matrix destination.
2. **Per-voice unison stereo spread** — give each unison copy its own pan position within the voice rather than mixing to mono first; the eight copies would fan across the stereo field independently of the voice-level Spread knob.
3. **LFO sync to note** — reset LFO phase on each `note_on` so every note begins at the same modulation point; useful for attack-synced vibrato or rhythmically consistent filter sweeps.
