use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute,
    terminal,
};
use std::io::stdout;
use std::sync::mpsc;
use std::time::Duration;

mod oscillator;
mod wavetable;
mod adsr;
mod keys;


fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device found");
    let config = device.default_output_config().expect("no output config");
    let sr = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;

    let (tx, rx) = mpsc::channel::<keys::Cmd>();

    let mut osc = oscillator::Oscillator::new();
    let mut env = adsr::Adsr::new(sr);
    // Countdown in samples before auto-releasing a note (used when key-release events
    // aren't available). 0 means no countdown is active.
    let mut auto_release: u64 = 0;
    let auto_release_samples = (sr * 0.5) as u64; // 500 ms

    let _stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        keys::Cmd::NoteOn(freq) => {
                            osc.set_freq(freq, sr);
                            env.note_on();
                            auto_release = auto_release_samples;
                        }
                        keys::Cmd::NoteOff => {
                            auto_release = 0;
                            env.note_off();
                        }
                        keys::Cmd::Waveform(idx) => osc.set_waveform(idx),
                    }
                }
                for frame in data.chunks_mut(channels) {
                    if auto_release > 0 {
                        auto_release -= 1;
                        if auto_release == 0 {
                            env.note_off();
                        }
                    }
                    let s = if env.is_active() {
                        let osc_s = osc.tick();
                        let env_s = env.tick();
                        osc_s * env_s * 0.3
                    } else {
                        0.0
                    };
                    for ch in frame.iter_mut() {
                        *ch = s;
                    }
                }
            },
            |e| eprintln!("stream error: {e}"),
            None,
        )
        .expect("failed to build output stream");

    _stream.play().expect("failed to start audio stream");

    terminal::enable_raw_mode().expect("failed to enable raw mode");

    // Enable key-release events if the terminal supports it (kitty protocol)
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    }

    let mut waveform = 0usize;

    println!("=== Wavetable Synth ===\r");
    println!("  Notes  :  A W S E D F T G Y H U J K  (C4 -> C5)\r");
    println!("  Waveform:  1=Sine  2=Saw  3=Square  4=Triangle\r");
    println!("  Quit   :  Q\r");
    if enhanced {
        println!("  (hold keys to sustain notes)\r");
    } else {
        println!("  (notes sustain 500ms — hold supported in kitty/wezterm)\r");
    }
    println!("\r");

    loop {
        if !event::poll(Duration::from_millis(5)).unwrap_or(false) {
            continue;
        }
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };

        if key.kind == KeyEventKind::Repeat {
            continue;
        }

        if key.kind == KeyEventKind::Release {
            if keys::key_note(key.code).is_some() {
                let _ = tx.send(keys::Cmd::NoteOff);
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char(c @ '1'..='4') => {
                waveform = c as usize - '1' as usize;
                let _ = tx.send(keys::Cmd::Waveform(waveform));
                println!("Waveform: {}\r", oscillator::WAVEFORMS[waveform]);
            }
            code => {
                if let Some(midi) = keys::key_note(code) {
                    let freq = keys::midi_freq(midi);
                    let _ = tx.send(keys::Cmd::NoteOn(freq));
                    println!(
                        "Note: {} ({:.1} Hz)  [{}]\r",
                        keys::note_name(midi),
                        freq,
                        oscillator::WAVEFORMS[waveform]
                    );
                }
            }
        }
    }

    if enhanced {
        let _ = execute!(stdout(), crossterm::event::PopKeyboardEnhancementFlags);
    }
    terminal::disable_raw_mode().expect("failed to disable raw mode");
    println!("\r\nGoodbye!");
}
