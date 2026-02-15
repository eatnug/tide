// Keyboard event to byte conversion for Terminal

use tide_core::{Key, Modifiers};

use super::Terminal;

impl Terminal {
    /// Convert a key event to the byte sequence that should be sent to the PTY
    pub fn key_to_bytes(key: &Key, modifiers: &Modifiers) -> Vec<u8> {
        match key {
            Key::Char(c) => {
                if modifiers.ctrl {
                    // Ctrl+A..Z maps to 0x01..0x1A
                    let lower = c.to_ascii_lowercase();
                    if lower.is_ascii_lowercase() {
                        return vec![(lower as u8) - b'a' + 1];
                    }
                }
                if modifiers.alt {
                    // Alt sends ESC prefix
                    let mut bytes = vec![0x1b];
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    bytes.extend_from_slice(s.as_bytes());
                    return bytes;
                }
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.as_bytes().to_vec()
            }
            Key::Enter => {
                if modifiers.shift {
                    vec![0x1b, b'[', b'1', b'3', b';', b'2', b'u'] // CSI u: ESC[13;2u
                } else {
                    vec![0x0d] // CR
                }
            }
            Key::Backspace => vec![0x7f],   // DEL
            Key::Tab => {
                if modifiers.shift {
                    vec![0x1b, b'[', b'Z'] // Shift+Tab = CSI Z
                } else {
                    vec![0x09]
                }
            }
            Key::Escape => vec![0x1b],
            Key::Delete => vec![0x1b, b'[', b'3', b'~'],
            Key::Up => Self::arrow_bytes(b'A', modifiers),
            Key::Down => Self::arrow_bytes(b'B', modifiers),
            Key::Right => Self::arrow_bytes(b'C', modifiers),
            Key::Left => Self::arrow_bytes(b'D', modifiers),
            Key::Home => vec![0x1b, b'[', b'H'],
            Key::End => vec![0x1b, b'[', b'F'],
            Key::PageUp => vec![0x1b, b'[', b'5', b'~'],
            Key::PageDown => vec![0x1b, b'[', b'6', b'~'],
            Key::Insert => vec![0x1b, b'[', b'2', b'~'],
            Key::F(n) => match n {
                1 => vec![0x1b, b'O', b'P'],
                2 => vec![0x1b, b'O', b'Q'],
                3 => vec![0x1b, b'O', b'R'],
                4 => vec![0x1b, b'O', b'S'],
                5 => vec![0x1b, b'[', b'1', b'5', b'~'],
                6 => vec![0x1b, b'[', b'1', b'7', b'~'],
                7 => vec![0x1b, b'[', b'1', b'8', b'~'],
                8 => vec![0x1b, b'[', b'1', b'9', b'~'],
                9 => vec![0x1b, b'[', b'2', b'0', b'~'],
                10 => vec![0x1b, b'[', b'2', b'1', b'~'],
                11 => vec![0x1b, b'[', b'2', b'3', b'~'],
                12 => vec![0x1b, b'[', b'2', b'4', b'~'],
                _ => vec![],
            },
        }
    }

    /// Build the CSI escape sequence for an arrow key with modifier support.
    /// Plain arrow: `\e[{dir}`, with modifiers: `\e[1;{mod}{dir}`
    /// Modifier codes: 2=Shift, 3=Alt, 5=Ctrl, etc.
    fn arrow_bytes(dir: u8, modifiers: &Modifiers) -> Vec<u8> {
        let modifier_code = 1
            + if modifiers.shift { 1 } else { 0 }
            + if modifiers.alt { 2 } else { 0 }
            + if modifiers.ctrl { 4 } else { 0 };
        if modifier_code > 1 {
            // CSI 1 ; {modifier} {dir}
            let code = b'0' + modifier_code;
            vec![0x1b, b'[', b'1', b';', code, dir]
        } else {
            vec![0x1b, b'[', dir]
        }
    }
}
