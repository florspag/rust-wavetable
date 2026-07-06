use crossterm::event::KeyCode;

pub enum Cmd {
    NoteOn(f32),
    NoteOff,
    Waveform(usize),
}

pub fn midi_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

pub fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (midi / 12) as i32 - 1;
    format!("{}{}", NAMES[(midi % 12) as usize], octave)
}

pub fn key_note(code: KeyCode) -> Option<u8> {
    // QWERTY piano layout: white keys on home row, black keys on top row
    //   W  E     T  Y  U
    //  A  S  D  F  G  H  J  K
    // C4 D4 E4 F4 G4 A4 B4 C5
    match code {
        KeyCode::Char('a') => Some(60), // C4
        KeyCode::Char('w') => Some(61), // C#4
        KeyCode::Char('s') => Some(62), // D4
        KeyCode::Char('e') => Some(63), // D#4
        KeyCode::Char('d') => Some(64), // E4
        KeyCode::Char('f') => Some(65), // F4
        KeyCode::Char('t') => Some(66), // F#4
        KeyCode::Char('g') => Some(67), // G4
        KeyCode::Char('y') => Some(68), // G#4
        KeyCode::Char('h') => Some(69), // A4
        KeyCode::Char('u') => Some(70), // A#4
        KeyCode::Char('j') => Some(71), // B4
        KeyCode::Char('k') => Some(72), // C5
        _ => None,
    }
}