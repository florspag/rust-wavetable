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

## The 4 Waveforms

| `kind` | Waveform | Formula | Harmonic content |
|--------|----------|---------|-----------------|
| `0` | Sine | `sin(2π·t)` | Fundamental only |
| `1` | Sawtooth | `2t − 1` | All harmonics: `1/n` amplitude |
| `2` | Square | `±1` at 50% duty | Odd harmonics: `1/n` amplitude |
| `3` | Triangle | `1 − 4·|t − 0.5|` | Odd harmonics: `1/n²` amplitude |

Triangle falls off as `1/n²` instead of `1/n`, so it aliases far less than saw or square — heard as a rounder, mellower tone.

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

## Signal Flow

```
Piano key OR frequency slider → frequency (Hz)
            ↓
  note_on: set_freq() resets phase + triggers envelope
  slider:  change_freq() updates phase_inc only (no click, no retrigger)
            ↓
  tick() — called once per audio sample
            ↓
  phase → table index → linear interpolation → raw sample
            ↓
  ADSR envelope → amplitude × raw sample × 0.3
            ↓
  ScriptProcessorNode / cpal → speakers
```

---

## Where to Go Next

1. **Multi-table mip-mapping** — generate one table per octave with harmonics capped at Nyquist for that octave, select the right table in `set_freq`.
2. **Cubic interpolation** — replace the linear lerp with 4-point Hermite for better HF accuracy.
3. **Waveform morphing** — crossfade between two tables by blending `table[a]` and `table[b]` samples for smooth timbral evolution.
4. **Unison / detune** — run multiple oscillators with slight pitch offsets and mix them, classic for fat synth sounds.
5. **Custom wavetable loading** — import single-cycle waveforms from `.wav` files to extend beyond the 4 built-in shapes.
