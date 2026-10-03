//! Keyboard and mouse decoding for the desktop and the text console.
//!
//! The IRQ handlers only queue raw bytes; decoding happens when a consumer
//! polls, so modifier state follows the order keys were actually pressed.
use crate::abi::*;

/// Decodes PS/2 scan code set 1 (as translated by the i8042) into key events.
pub struct KeyboardDecoder { extended: bool, shift_left: bool, shift_right: bool, ctrl: bool, alt: bool, caps: bool }

const PLAIN: &[u8; 58] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
const SHIFTED: &[u8; 58] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

impl KeyboardDecoder {
    pub const fn new() -> Self {
        Self { extended: false, shift_left: false, shift_right: false, ctrl: false, alt: false, caps: false }
    }

    pub fn modifiers(&self) -> u8 {
        (if self.shift_left || self.shift_right { MOD_SHIFT } else { 0 })
            | if self.ctrl { MOD_CTRL } else { 0 }
            | if self.alt { MOD_ALT } else { 0 }
            | if self.caps { MOD_CAPS } else { 0 }
    }

    /// Feeds one scan code byte; returns the completed key event, if any.
    pub fn feed(&mut self, byte: u8) -> Option<InputEvent> {
        if byte == 0xe0 { self.extended = true; return None; }
        let extended = core::mem::replace(&mut self.extended, false);
        let pressed = byte & 0x80 == 0;
        let code = byte & 0x7f;
        let key = if extended {
            match code {
                0x48 => KEY_UP, 0x50 => KEY_DOWN, 0x4b => KEY_LEFT, 0x4d => KEY_RIGHT,
                0x47 => KEY_HOME, 0x4f => KEY_END, 0x49 => KEY_PAGE_UP, 0x51 => KEY_PAGE_DOWN,
                0x53 => KEY_DELETE, 0x52 => KEY_INSERT, 0x1c => b'\n' as u16, 0x35 => b'/' as u16,
                0x1d => { self.ctrl = pressed; KEY_CTRL }
                0x38 => { self.alt = pressed; KEY_ALT }
                0x5b | 0x5c => KEY_SUPER,
                0x2a | 0x37 => return None, // Print Screen's fake shift sequence.
                _ => return None,
            }
        } else {
            match code {
                0x2a => { self.shift_left = pressed; KEY_SHIFT }
                0x36 => { self.shift_right = pressed; KEY_SHIFT }
                0x1d => { self.ctrl = pressed; KEY_CTRL }
                0x38 => { self.alt = pressed; KEY_ALT }
                0x3a => { if pressed { self.caps = !self.caps; } KEY_CAPS_LOCK }
                0x3b..=0x44 => KEY_F1 + (code - 0x3b) as u16,
                0x57 => KEY_F1 + 10, 0x58 => KEY_F1 + 11,
                // Numpad + keeps its historical meaning of '>' for redirection.
                0x4e => b'>' as u16,
                0x4a => b'-' as u16,
                _ if (code as usize) < PLAIN.len() && PLAIN[code as usize] != 0 => {
                    let plain = PLAIN[code as usize];
                    let shift = self.shift_left || self.shift_right;
                    let shifted = if plain.is_ascii_alphabetic() { shift != self.caps } else { shift };
                    (if shifted { SHIFTED[code as usize] } else { plain }) as u16
                }
                _ => return None,
            }
        };
        Some(InputEvent { kind: INPUT_KEY, modifiers: self.modifiers(), pressed: pressed as u8, key, ..Default::default() })
    }

    /// The character a console reader should receive for this event, if any.
    pub fn console_byte(event: &InputEvent) -> Option<u8> {
        if event.kind != INPUT_KEY || event.pressed == 0 || event.key >= 0x80 { return None; }
        let byte = event.key as u8;
        if event.modifiers & MOD_CTRL != 0 { return None; }
        matches!(byte, 8 | 9 | b'\n' | 27 | 32..=126).then_some(byte)
    }
}

/// Assembles PS/2 mouse packets (3 bytes, or 4 with an IntelliMouse wheel).
pub struct MouseDecoder { packet: [u8; 4], len: usize, pub wheel: bool }

impl MouseDecoder {
    pub const fn new() -> Self { Self { packet: [0; 4], len: 0, wheel: false } }

    pub fn feed(&mut self, byte: u8) -> Option<InputEvent> {
        // Bit 3 of the first byte is always set; use it to resynchronise.
        if self.len == 0 && byte & 0x08 == 0 { return None; }
        self.packet[self.len] = byte;
        self.len += 1;
        if self.len < if self.wheel { 4 } else { 3 } { return None; }
        self.len = 0;
        let flags = self.packet[0];
        if flags & 0xc0 != 0 { return None; } // Overflowed packets carry no usable delta.
        let dx = self.packet[1] as i16 - if flags & 0x10 != 0 { 256 } else { 0 };
        let dy = self.packet[2] as i16 - if flags & 0x20 != 0 { 256 } else { 0 };
        let wheel = if self.wheel { -((self.packet[3] as i8) << 4 >> 4) as i16 } else { 0 };
        Some(InputEvent { kind: INPUT_MOUSE, buttons: flags & 7, dx, dy: -dy, wheel, ..Default::default() })
    }
}

use alloc::collections::VecDeque;
use crate::memory::IrqSpinlock;

pub static KEYBOARD: IrqSpinlock<KeyboardDecoder> = IrqSpinlock::new(KeyboardDecoder::new());
pub static MOUSE: IrqSpinlock<MouseDecoder> = IrqSpinlock::new(MouseDecoder::new());
/// Decoded events waiting for the display owner.
static PENDING: IrqSpinlock<VecDeque<InputEvent>> = IrqSpinlock::new(VecDeque::new());
const PENDING_MAX: usize = 1024;

fn drain(buffer: &IrqSpinlock<crate::io::RingBuffer>) -> Option<u8> {
    let (ring, flags) = buffer.lock();
    let byte = ring.pop();
    buffer.unlock(flags);
    byte
}

/// Next console character from the keyboard, decoding modifiers.
pub fn console_byte() -> Option<u8> {
    while let Some(code) = drain(&crate::io::KEYBOARD_BUFFER) {
        let (decoder, flags) = KEYBOARD.lock();
        let event = decoder.feed(code);
        KEYBOARD.unlock(flags);
        if let Some(byte) = event.as_ref().and_then(KeyboardDecoder::console_byte) { return Some(byte); }
    }
    None
}

/// Decodes everything queued so far and returns up to `max` events in order.
/// Consecutive mouse motion with unchanged buttons is merged into one event.
pub fn poll(max: usize) -> alloc::vec::Vec<InputEvent> {
    let (pending, flags) = PENDING.lock();
    while let Some(code) = drain(&crate::io::KEYBOARD_BUFFER) {
        let (decoder, kf) = KEYBOARD.lock();
        let event = decoder.feed(code);
        KEYBOARD.unlock(kf);
        if let Some(event) = event { pending.push_back(event); }
    }
    while let Some(byte) = drain(&crate::io::MOUSE_BUFFER) {
        let (decoder, mf) = MOUSE.lock();
        let event = decoder.feed(byte);
        MOUSE.unlock(mf);
        let Some(event) = event else { continue };
        if let Some(last) = pending.back_mut() {
            if last.kind == INPUT_MOUSE && last.buttons == event.buttons && last.wheel == 0 && event.wheel == 0 {
                last.dx = last.dx.saturating_add(event.dx);
                last.dy = last.dy.saturating_add(event.dy);
                continue;
            }
        }
        pending.push_back(event);
    }
    while pending.len() > PENDING_MAX { pending.pop_front(); }
    let count = max.min(pending.len());
    let events = pending.drain(..count).collect();
    PENDING.unlock(flags);
    events
}

/// Forgets queued input (used when the display owner changes).
pub fn clear() {
    while drain(&crate::io::KEYBOARD_BUFFER).is_some() {}
    while drain(&crate::io::MOUSE_BUFFER).is_some() {}
    let (pending, flags) = PENDING.lock();
    pending.clear();
    PENDING.unlock(flags);
}
