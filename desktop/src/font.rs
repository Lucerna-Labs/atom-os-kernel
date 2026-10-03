//! Pre-rendered anti-aliased fonts (see scripts/gen-font.py).
pub struct Glyph { pub offset: u32, pub width: u8, pub height: u8, pub x: i8, pub y: i8, pub advance: u8 }
/// ASCII glyphs from `first`, followed by the characters of `extra` in order.
pub struct Font { pub ascent: i32, pub descent: i32, pub first: u8, pub glyphs: &'static [Glyph], pub bitmap: &'static [u8], pub extra: &'static str }

impl Font {
    pub fn glyph(&self, ch: char) -> &Glyph {
        let code = ch as u32;
        let ascii = 127 - self.first as u32;
        let index = if code >= self.first as u32 && code < 127 {
            code - self.first as u32
        } else if let Some(i) = self.extra.chars().position(|c| c == ch) {
            ascii + i as u32
        } else { b'?' as u32 - self.first as u32 };
        &self.glyphs[index as usize]
    }
    pub fn line_height(&self) -> i32 { self.ascent + self.descent }
    pub fn width(&self, text: &str) -> i32 { text.chars().map(|c| self.glyph(c).advance as i32).sum() }
    /// Longest prefix of `text` (in bytes) that fits in `max` pixels.
    pub fn fit(&self, text: &str, max: i32) -> usize {
        let mut width = 0;
        for (index, ch) in text.char_indices() {
            width += self.glyph(ch).advance as i32;
            if width > max { return index; }
        }
        text.len()
    }
}
