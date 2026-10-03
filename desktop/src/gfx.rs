//! Software rendering into a 0x00RRGGBB back buffer.
use alloc::vec;
use alloc::vec::Vec;
use crate::font::Font;

pub type Color = u32;
pub const fn rgb(r: u8, g: u8, b: u8) -> Color { (r as u32) << 16 | (g as u32) << 8 | b as u32 }

pub fn sqrt(value: f32) -> f32 {
    use core::arch::x86_64::{_mm_cvtss_f32, _mm_set_ss, _mm_sqrt_ss};
    unsafe { _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(value))) }
}

/// Mixes `src` over `dst` with coverage `alpha` (0..=255).
#[inline]
pub fn blend(dst: Color, src: Color, alpha: u32) -> Color {
    if alpha >= 255 { return src; }
    if alpha == 0 { return dst; }
    let inv = 255 - alpha;
    let rb = ((dst & 0xff00ff) * inv + (src & 0xff00ff) * alpha + 0x800080) >> 8 & 0xff00ff;
    let g = ((dst & 0xff00) * inv + (src & 0xff00) * alpha + 0x8000) >> 8 & 0xff00;
    rb | g
}
pub fn mix(a: Color, b: Color, t: u32) -> Color { blend(a, b, t) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self { Self { x, y, w, h } }
    pub fn right(&self) -> i32 { self.x + self.w }
    pub fn bottom(&self) -> i32 { self.y + self.h }
    pub fn is_empty(&self) -> bool { self.w <= 0 || self.h <= 0 }
    pub fn contains(&self, x: i32, y: i32) -> bool { x >= self.x && y >= self.y && x < self.right() && y < self.bottom() }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x); let y = self.y.max(o.y);
        Rect::new(x, y, (self.right().min(o.right()) - x).max(0), (self.bottom().min(o.bottom()) - y).max(0))
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() { return *o; }
        if o.is_empty() { return *self; }
        let x = self.x.min(o.x); let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
    pub fn inset(&self, d: i32) -> Rect { Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d) }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect { Rect::new(self.x + dx, self.y + dy, self.w, self.h) }
}

pub struct Canvas { pub width: i32, pub height: i32, pub pixels: Vec<Color>, clip: Rect }

impl Canvas {
    pub fn new(width: i32, height: i32) -> Self {
        Self { width, height, pixels: vec![0; (width * height) as usize], clip: Rect::new(0, 0, width, height) }
    }
    pub fn bounds(&self) -> Rect { Rect::new(0, 0, self.width, self.height) }
    pub fn set_clip(&mut self, rect: Rect) { self.clip = rect.intersect(&self.bounds()); }
    pub fn clip(&self) -> Rect { self.clip }
    pub fn reset_clip(&mut self) { self.clip = self.bounds(); }

    #[inline]
    pub fn pixel(&mut self, x: i32, y: i32, color: Color, alpha: u32) {
        if self.clip.contains(x, y) {
            let p = &mut self.pixels[(y * self.width + x) as usize];
            *p = blend(*p, color, alpha);
        }
    }
    pub fn fill(&mut self, rect: Rect, color: Color) {
        let r = rect.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = (y * self.width) as usize;
            self.pixels[row + r.x as usize..row + r.right() as usize].fill(color);
        }
    }
    pub fn fill_alpha(&mut self, rect: Rect, color: Color, alpha: u32) {
        let r = rect.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = (y * self.width) as usize;
            for p in &mut self.pixels[row + r.x as usize..row + r.right() as usize] { *p = blend(*p, color, alpha); }
        }
    }
    pub fn gradient_v(&mut self, rect: Rect, top: Color, bottom: Color) {
        let r = rect.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let t = ((y - rect.y) * 255 / rect.h.max(1)) as u32;
            let color = mix(top, bottom, t);
            let row = (y * self.width) as usize;
            self.pixels[row + r.x as usize..row + r.right() as usize].fill(color);
        }
    }
    pub fn outline(&mut self, rect: Rect, color: Color) {
        self.fill(Rect::new(rect.x, rect.y, rect.w, 1), color);
        self.fill(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), color);
        self.fill(Rect::new(rect.x, rect.y, 1, rect.h), color);
        self.fill(Rect::new(rect.right() - 1, rect.y, 1, rect.h), color);
    }

    /// Anti-aliased rounded rectangle, optionally translucent.
    pub fn round_rect(&mut self, rect: Rect, radius: i32, color: Color, alpha: u32) {
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        let r = rect.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = (y * self.width) as usize;
            for x in r.x..r.right() {
                let cx = if x < rect.x + radius { rect.x + radius } else if x >= rect.right() - radius { rect.right() - radius - 1 } else { x };
                let cy = if y < rect.y + radius { rect.y + radius } else if y >= rect.bottom() - radius { rect.bottom() - radius - 1 } else { y };
                let coverage = if cx == x || cy == y { 255 } else {
                    let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
                    let d = sqrt(dx * dx + dy * dy);
                    let c = radius as f32 - d + 0.5;
                    if c <= 0.0 { 0 } else if c >= 1.0 { 255 } else { (c * 255.0) as u32 }
                };
                if coverage > 0 {
                    let p = &mut self.pixels[row + x as usize];
                    *p = blend(*p, color, coverage * alpha / 255);
                }
            }
        }
    }
    /// Rounded outline one pixel wide.
    pub fn round_outline(&mut self, rect: Rect, radius: i32, color: Color, alpha: u32) {
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        let r = rect.intersect(&self.clip);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let cx = if x < rect.x + radius { rect.x + radius } else if x >= rect.right() - radius { rect.right() - radius - 1 } else { x };
                let cy = if y < rect.y + radius { rect.y + radius } else if y >= rect.bottom() - radius { rect.bottom() - radius - 1 } else { y };
                let coverage = if cx == x || cy == y {
                    if x == rect.x || y == rect.y || x == rect.right() - 1 || y == rect.bottom() - 1 { 255 } else { 0 }
                } else {
                    let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
                    let d = sqrt(dx * dx + dy * dy);
                    let e = 1.0 - ((radius as f32 - 0.5) - d).abs();
                    if e <= 0.0 { 0 } else { (e.min(1.0) * 255.0) as u32 }
                };
                if coverage > 0 { self.pixel(x, y, color, coverage * alpha / 255); }
            }
        }
    }
    /// Soft drop shadow around `rect`.
    pub fn shadow(&mut self, rect: Rect, radius: i32, spread: i32, strength: u32) {
        for step in 0..spread {
            let alpha = strength * (spread - step) as u32 / (spread as u32 * spread as u32 / 2).max(1);
            self.round_outline(rect.inset(-step).offset(0, spread / 3), radius + step, 0, alpha.min(255));
        }
    }
    pub fn circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color, alpha: u32) {
        self.round_rect(Rect::new(cx - radius, cy - radius, radius * 2, radius * 2), radius, color, alpha);
    }
    /// Anti-aliased line of the given width.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        let (minx, maxx) = (x0.min(x1) - width, x0.max(x1) + width);
        let (miny, maxy) = (y0.min(y1) - width, y0.max(y1) + width);
        let (vx, vy) = (x1 - x0, y1 - y0);
        let len2 = (vx * vx + vy * vy).max(0.0001);
        for y in miny as i32..=maxy as i32 {
            for x in minx as i32..=maxx as i32 {
                let (px, py) = (x as f32 + 0.5 - x0, y as f32 + 0.5 - y0);
                let t = ((px * vx + py * vy) / len2).clamp(0.0, 1.0);
                let (dx, dy) = (px - t * vx, py - t * vy);
                let c = width / 2.0 - sqrt(dx * dx + dy * dy) + 0.5;
                if c > 0.0 { self.pixel(x, y, color, (c.min(1.0) * 255.0) as u32); }
            }
        }
    }

    /// Draws text with its top-left at (x, y); returns the advance width.
    pub fn text(&mut self, font: &Font, x: i32, y: i32, text: &str, color: Color) -> i32 {
        let mut pen = x;
        for ch in text.chars() {
            let g = font.glyph(ch);
            let gx = pen + g.x as i32;
            let gy = y + g.y as i32;
            if gx < self.clip.right() && gx + (g.width as i32) > self.clip.x && gy < self.clip.bottom() && gy + (g.height as i32) > self.clip.y {
                for row in 0..g.height as i32 {
                    let base = g.offset as usize + (row * g.width as i32) as usize;
                    for col in 0..g.width as i32 {
                        let a = font.bitmap[base + col as usize] as u32;
                        if a != 0 { self.pixel(gx + col, gy + row, color, a); }
                    }
                }
            }
            pen += g.advance as i32;
        }
        pen - x
    }
    /// Text truncated with an ellipsis to fit `max` pixels.
    pub fn text_fit(&mut self, font: &Font, x: i32, y: i32, text: &str, max: i32, color: Color) {
        if font.width(text) <= max { self.text(font, x, y, text, color); return; }
        let cut = font.fit(text, max - font.width("..."));
        let w = self.text(font, x, y, &text[..cut], color);
        self.text(font, x + w, y, "...", color);
    }
    pub fn text_centered(&mut self, font: &Font, rect: Rect, text: &str, color: Color) {
        let w = font.width(text);
        self.text(font, rect.x + (rect.w - w) / 2, rect.y + (rect.h - font.line_height()) / 2, text, color);
    }
    pub fn blit(&mut self, src: &[Color], src_width: i32, dst: Rect) {
        let r = dst.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let s = ((y - dst.y) * src_width + (r.x - dst.x)) as usize;
            let d = (y * self.width + r.x) as usize;
            self.pixels[d..d + r.w as usize].copy_from_slice(&src[s..s + r.w as usize]);
        }
    }
}
