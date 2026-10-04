//! The desktop's drawing surface. All rasterization is done by pmre-kit (the Atom
//! Rendering Engine): shapes are `DrawCmd`s scan-converted from signed-distance fields,
//! outlines are kit path strokes, text is the kit's TrueType rasterizer. This module only
//! owns the 0x00RRGGBB back buffer the kit draws into (as a `pmre_kit::Surface`) and the
//! clip rectangle; it decides nothing about coverage.
//!
//! HiDPI: the desktop works in logical pixels and the canvas has an integer `scale`
//! (1 at Full HD and below, 2 at 4K, 3 at 6K). Every drawing call multiplies its
//! geometry by the scale before it reaches the kit, so shapes keep the kit's pure
//! translation fast path and text is rasterized at the larger size, staying sharp.
use alloc::vec;
use alloc::vec::Vec;
use pmre_kit::{Affine, Bounds, DrawCmd, Paint, Rgba, Shape, Surface, Vec2};
use crate::font::{Font, Weight};

pub type Color = u32;
pub const fn rgb(r: u8, g: u8, b: u8) -> Color { (r as u32) << 16 | (g as u32) << 8 | b as u32 }

/// A desktop colour with `alpha` (0..=255) as a kit colour.
pub fn rgba(color: Color, alpha: u32) -> Rgba {
    Rgba::new(((color >> 16) & 255) as f32 / 255.0, ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0, alpha.min(255) as f32 / 255.0)
}

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
    /// `self` minus `cut`, as up to four non-overlapping rectangles.
    pub fn subtract(&self, cut: &Rect) -> Vec<Rect> {
        let i = self.intersect(cut);
        if i.is_empty() { return vec![*self]; }
        let mut out = Vec::new();
        if i.y > self.y { out.push(Rect::new(self.x, self.y, self.w, i.y - self.y)); }
        if i.bottom() < self.bottom() { out.push(Rect::new(self.x, i.bottom(), self.w, self.bottom() - i.bottom())); }
        if i.x > self.x { out.push(Rect::new(self.x, i.y, i.x - self.x, i.h)); }
        if i.right() < self.right() { out.push(Rect::new(i.right(), i.y, self.right() - i.right(), i.h)); }
        out
    }
    pub fn inset(&self, d: i32) -> Rect { Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d) }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect { Rect::new(self.x + dx, self.y + dy, self.w, self.h) }
    fn bounds(&self) -> Bounds {
        Bounds { min: Vec2::new(self.x as f32, self.y as f32), max: Vec2::new(self.right() as f32, self.bottom() as f32) }
    }
    fn center(&self) -> Affine { Affine::translate(self.x as f32 + self.w as f32 / 2.0, self.y as f32 + self.h as f32 / 2.0) }
    fn half(&self) -> Vec2 { Vec2::new(self.w as f32 / 2.0, self.h as f32 / 2.0) }
}

/// `width`, `height`, `pixels` and the clip are physical; the drawing API is logical.
pub struct Canvas { pub width: i32, pub height: i32, pub pixels: Vec<Color>, clip: Rect, pub scale: i32 }

/// A kit shape with its geometry multiplied by `k`.
fn scaled(shape: Shape, k: f32) -> Shape {
    match shape {
        Shape::Rect { half } => Shape::Rect { half: half.scale(k) },
        Shape::RoundedRect { half, radius } => Shape::RoundedRect { half: half.scale(k), radius: radius * k },
        Shape::Circle { radius } => Shape::Circle { radius: radius * k },
        Shape::Line { a, b, width } => Shape::Line { a: a.scale(k), b: b.scale(k), width: width * k },
        Shape::RoundedRectOutline { half, radius, width } =>
            Shape::RoundedRectOutline { half: half.scale(k), radius: radius * k, width: width * k },
        Shape::CircleOutline { radius, width } => Shape::CircleOutline { radius: radius * k, width: width * k },
    }
}

impl Surface for Canvas {
    fn width(&self) -> u32 { self.width as u32 }
    fn height(&self) -> u32 { self.height as u32 }
    /// Rows outside the clip are skipped by the kit's rasterizers.
    fn row_range(&self) -> (u32, u32) { (self.clip.y as u32, self.clip.bottom() as u32) }
    /// The kit hands over straight-alpha colour with coverage folded into alpha; the back
    /// buffer is opaque, so "over" reduces to a lerp.
    fn blend_over(&mut self, x: u32, y: u32, src: Rgba) {
        let (x, y) = (x as i32, y as i32);
        if !self.clip.contains(x, y) || src.a <= 0.0 { return; }
        let p = &mut self.pixels[(y * self.width + x) as usize];
        let a = src.a.min(1.0);
        let channel = |shift: u32, s: f32| {
            let d = ((*p >> shift) & 255) as f32;
            ((d + (s * 255.0 - d) * a + 0.5) as u32).min(255) << shift
        };
        *p = channel(16, src.r) | channel(8, src.g) | channel(0, src.b);
    }
    /// Fully covered interior runs: one colour computed once, then a bulk row write when
    /// opaque, or the same lerp as `blend_over` without the per-pixel clip test.
    fn fill_span(&mut self, y: u32, x0: u32, x1: u32, src: Rgba) {
        let y = y as i32;
        if y < self.clip.y || y >= self.clip.bottom() || src.a <= 0.0 { return; }
        let x0 = (x0 as i32).max(self.clip.x);
        let x1 = (x1 as i32).min(self.clip.right());
        if x0 >= x1 { return; }
        let row = (y * self.width) as usize;
        let span = &mut self.pixels[row + x0 as usize..row + x1 as usize];
        let a = src.a.min(1.0);
        if a >= 1.0 {
            let c = |s: f32| ((s * 255.0 + 0.5) as u32).min(255);
            span.fill(c(src.r) << 16 | c(src.g) << 8 | c(src.b));
            return;
        }
        for p in span {
            let channel = |shift: u32, s: f32| {
                let d = ((*p >> shift) & 255) as f32;
                ((d + (s * 255.0 - d) * a + 0.5) as u32).min(255) << shift
            };
            *p = channel(16, src.r) | channel(8, src.g) | channel(0, src.b);
        }
    }
}

impl Canvas {
    /// A canvas of `width` x `height` physical pixels drawn at `scale`.
    pub fn new(width: i32, height: i32, scale: i32) -> Self {
        let scale = scale.max(1);
        Self { width, height, pixels: vec![0; (width * height) as usize], clip: Rect::new(0, 0, width, height), scale }
    }
    fn physical_bounds(&self) -> Rect { Rect::new(0, 0, self.width, self.height) }
    /// Logical size of the canvas.
    pub fn bounds(&self) -> Rect { Rect::new(0, 0, self.width / self.scale, self.height / self.scale) }
    /// A logical rectangle in physical pixels.
    pub fn physical(&self, r: Rect) -> Rect {
        let k = self.scale;
        Rect::new(r.x * k, r.y * k, r.w * k, r.h * k).intersect(&self.physical_bounds())
    }
    pub fn set_clip(&mut self, rect: Rect) { self.clip = self.physical(rect); }
    /// The clip in logical pixels (the smallest logical rectangle covering it).
    pub fn clip(&self) -> Rect {
        let (k, c) = (self.scale, self.clip);
        let (x, y) = (c.x / k, c.y / k);
        Rect::new(x, y, (c.right() + k - 1) / k - x, (c.bottom() + k - 1) / k - y)
    }
    pub fn reset_clip(&mut self) { self.clip = self.physical_bounds(); }
    fn k(&self) -> f32 { self.scale as f32 }

    fn draw(&mut self, cmd: DrawCmd) {
        if self.clip.is_empty() { return; }
        let clip = self.clip.bounds();
        pmre_kit::raster::scan_convert(&cmd, self, Some(clip));
    }
    /// `at` is a logical translation; shape, paint and softness are scaled with it.
    fn shape(&mut self, shape: Shape, at: Affine, paint: Paint, soft: f32) {
        let k = self.k();
        let paint = match paint {
            Paint::Linear { from, to, c0, c1 } => Paint::Linear { from: from.scale(k), to: to.scale(k), c0, c1 },
            other => other,
        };
        let transform = Affine::translate(at.e * k, at.f * k);
        self.draw(DrawCmd { shape: scaled(shape, k), paint, transform, soft: soft * k });
    }

    pub fn fill(&mut self, rect: Rect, color: Color) { self.fill_alpha(rect, color, 255); }
    pub fn fill_alpha(&mut self, rect: Rect, color: Color, alpha: u32) {
        if rect.is_empty() { return; }
        self.shape(Shape::Rect { half: rect.half() }, rect.center(), Paint::Solid(rgba(color, alpha)), 0.0);
    }
    pub fn gradient_v(&mut self, rect: Rect, top: Color, bottom: Color) {
        let h = rect.half();
        self.shape(Shape::Rect { half: h }, rect.center(),
            Paint::Linear { from: Vec2::new(0.0, -h.y), to: Vec2::new(0.0, h.y), c0: rgba(top, 255), c1: rgba(bottom, 255) }, 0.0);
    }
    /// Linear gradient from the top-left corner to the bottom-right.
    pub fn diagonal(&mut self, rect: Rect, from: Color, to: Color) {
        let h = rect.half();
        self.shape(Shape::Rect { half: h }, rect.center(),
            Paint::Linear { from: Vec2::new(-h.x, -h.y), to: Vec2::new(h.x, h.y), c0: rgba(from, 255), c1: rgba(to, 255) }, 0.0);
    }
    pub fn outline(&mut self, rect: Rect, color: Color) {
        self.fill(Rect::new(rect.x, rect.y, rect.w, 1), color);
        self.fill(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), color);
        self.fill(Rect::new(rect.x, rect.y, 1, rect.h), color);
        self.fill(Rect::new(rect.right() - 1, rect.y, 1, rect.h), color);
    }
    pub fn round_rect(&mut self, rect: Rect, radius: i32, color: Color, alpha: u32) {
        if rect.is_empty() { return; }
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0) as f32;
        self.shape(Shape::RoundedRect { half: rect.half(), radius }, rect.center(), Paint::Solid(rgba(color, alpha)), 0.0);
    }
    /// A one-pixel rounded outline: the kit's SDF outline shape, a band just inside the edge.
    pub fn round_outline(&mut self, rect: Rect, radius: i32, color: Color, alpha: u32) {
        if rect.is_empty() { return; }
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0) as f32;
        let shape = Shape::RoundedRectOutline { half: rect.half(), radius, width: 1.0 };
        self.shape(shape, rect.center(), Paint::Solid(rgba(color, alpha)), 0.0);
    }
    /// A soft drop shadow: the kit's widened anti-aliasing band (`soft`) on a rounded rect.
    pub fn shadow(&mut self, rect: Rect, radius: i32, spread: i32, strength: u32) {
        let r = rect.offset(0, spread / 3);
        let radius = radius.min(r.w / 2).min(r.h / 2).max(0) as f32;
        self.shape(Shape::RoundedRect { half: r.half(), radius }, r.center(), Paint::Solid(rgba(0, strength)), spread as f32);
    }
    pub fn circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color, alpha: u32) {
        self.shape(Shape::Circle { radius: radius as f32 }, Affine::translate(cx as f32, cy as f32), Paint::Solid(rgba(color, alpha)), 0.0);
    }
    /// A soft-edged disc (for glows and the wallpaper).
    pub fn glow(&mut self, cx: i32, cy: i32, radius: i32, color: Color, alpha: u32, soft: f32) {
        self.shape(Shape::Circle { radius: radius as f32 }, Affine::translate(cx as f32, cy as f32), Paint::Solid(rgba(color, alpha)), soft);
    }
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        self.shape(Shape::Line { a: Vec2::new(x0, y0), b: Vec2::new(x1, y1), width }, Affine::translate(0.0, 0.0), Paint::Solid(rgba(color, 255)), 0.0);
    }
    /// A stroked polyline (round joins), through the kit's path stroker.
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color, closed: bool) {
        let k = self.k();
        let points: Vec<Vec2> = points.iter().map(|&(x, y)| Vec2::new(x * k, y * k)).collect();
        let clip = self.clip.bounds();
        pmre_kit::path::stroke(self, &[points], width * k, Paint::Solid(rgba(color, 255)), Some(clip), closed);
    }
    /// Text with the top of its line box at (x, y); returns the advance width.
    pub fn text(&mut self, font: &Font, x: i32, y: i32, text: &str, color: Color) -> i32 {
        if self.clip.is_empty() { return font.width(text); }
        let clip = Some(self.clip.bounds());
        let k = self.k();
        let origin = Vec2::new(x as f32 * k, y as f32 * k);
        let px = font.px() * k;
        match font.weight {
            Weight::Mono => pmre_kit::text::draw_face(self, Font::mono_face(), text, origin, px, rgba(color, 255), clip, false),
            w => pmre_kit::text::draw_styled(self, text, origin, px, rgba(color, 255), clip, w == Weight::Bold, false),
        }
        font.width(text)
    }
    /// Text truncated with an ellipsis to fit `max` pixels.
    pub fn text_fit(&mut self, font: &Font, x: i32, y: i32, text: &str, max: i32, color: Color) {
        if font.width(text) <= max { self.text(font, x, y, text, color); return; }
        let cut = font.fit(text, max - font.width("\u{2026}"));
        let w = self.text(font, x, y, &text[..cut], color);
        self.text(font, x + w, y, "\u{2026}", color);
    }
    pub fn text_centered(&mut self, font: &Font, rect: Rect, text: &str, color: Color) {
        let w = font.width(text);
        self.text(font, rect.x + (rect.w - w) / 2, rect.y + (rect.h - font.line_height()) / 2, text, color);
    }
    /// Copies already-rendered physical pixels (the cached wallpaper, `src_width` physical
    /// pixels wide) to the logical rectangle `dst`.
    pub fn blit(&mut self, src: &[Color], src_width: i32, dst: Rect) {
        let dst = self.physical(dst);
        let r = dst.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let s = ((y - dst.y) * src_width + (r.x - dst.x)) as usize;
            let d = (y * self.width + r.x) as usize;
            self.pixels[d..d + r.w as usize].copy_from_slice(&src[s..s + r.w as usize]);
        }
    }

    /// The physical pixels of the two bottom corner squares (`radius` logical pixels) of
    /// `r`, saved before content is drawn over them.
    pub fn save_corners(&self, r: Rect, radius: i32) -> [Vec<Color>; 2] {
        let k = self.scale;
        let n = radius * k;
        let pr = Rect::new(r.x * k, r.y * k, r.w * k, r.h * k);
        let bounds = self.physical_bounds();
        let corner = |cx: i32, cy: i32| {
            let mut out = vec![0; (n * n) as usize];
            for yy in 0..n { for xx in 0..n {
                if bounds.contains(cx + xx, cy + yy) { out[(yy * n + xx) as usize] = self.pixels[((cy + yy) * self.width + cx + xx) as usize]; }
            } }
            out
        };
        [corner(pr.x, pr.bottom() - n), corner(pr.right() - n, pr.bottom() - n)]
    }
    /// Puts back the saved corner pixels outside the rounded edge of `r`, blended by the
    /// kit's coverage of the rounded-rect signed-distance field, so square content never
    /// pokes past the rounded window outline.
    pub fn restore_corners(&mut self, r: Rect, radius: i32, saved: &[Vec<Color>; 2]) {
        let k = self.scale;
        let n = radius * k;
        let pr = Rect::new(r.x * k, r.y * k, r.w * k, r.h * k);
        let shape = Shape::RoundedRect { half: pr.half(), radius: n as f32 };
        let (wcx, wcy) = (pr.x as f32 + pr.w as f32 / 2.0, pr.y as f32 + pr.h as f32 / 2.0);
        for (c, (cx, cy)) in [(pr.x, pr.bottom() - n), (pr.right() - n, pr.bottom() - n)].into_iter().enumerate() {
            for yy in 0..n { for xx in 0..n {
                let (px, py) = (cx + xx, cy + yy);
                let local = Vec2::new(px as f32 + 0.5 - wcx, py as f32 + 0.5 - wcy);
                let inside = pmre_kit::raster::coverage(pmre_kit::raster::signed_distance(&shape, local), 0.5);
                if inside < 1.0 && px >= 0 && py >= 0 {
                    let behind = saved[c][(yy * n + xx) as usize];
                    self.blend_over(px as u32, py as u32, rgba(behind, ((1.0 - inside) * 255.0) as u32));
                }
            } }
        }
    }
}
