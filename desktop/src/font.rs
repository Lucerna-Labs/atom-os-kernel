//! Text styles over pmre-kit's TrueType engine. The kit parses the embedded DejaVu
//! subsets and rasterizes glyphs from their outlines; this module only names the
//! styles the interface uses and measures text through the kit.
use pmre_kit::font::Font as Face;
use pmre_kit::sync::OnceLock;

static MONO_FACE: OnceLock<Face> = OnceLock::new();

/// Installs the embedded faces. Call once before drawing text.
pub fn install() {
    let parse = |bytes: &[u8]| Face::parse(bytes.to_vec()).expect("embedded font");
    pmre_kit::font::install(parse(include_bytes!("../fonts/DejaVuSans.ttf")),
        Some(parse(include_bytes!("../fonts/DejaVuSans-Bold.ttf"))));
    let _ = MONO_FACE.set(parse(include_bytes!("../fonts/DejaVuSansMono.ttf")));
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight { Regular, Bold, Mono }

/// A text style: face and pixel size.
pub struct Font { pub weight: Weight, pub size: u32 }

impl Font {
    pub fn px(&self) -> f32 { self.size as f32 }
    pub fn mono_face() -> &'static Face { MONO_FACE.get().expect("fonts installed") }
    fn metrics(&self) -> (f32, f32) {
        match self.weight {
            Weight::Mono => (Self::mono_face().ascent(self.px()), Self::mono_face().descent(self.px())),
            w => pmre_kit::text::v_metrics_styled(self.px(), w == Weight::Bold),
        }
    }
    pub fn line_height(&self) -> i32 { let (a, d) = self.metrics(); libm_ceil(a + d) }
    pub fn width_f(&self, text: &str) -> f32 {
        match self.weight {
            Weight::Mono => pmre_kit::text::advance_face(Self::mono_face(), text, self.px()),
            w => pmre_kit::text::advance_styled(text, self.px(), w == Weight::Bold),
        }
    }
    pub fn width(&self, text: &str) -> i32 { libm_ceil(self.width_f(text)) }
    /// Advance of one character (the cell width for the monospace face).
    pub fn char_width(&self, ch: char) -> f32 {
        let mut buf = [0u8; 4];
        self.width_f(ch.encode_utf8(&mut buf))
    }
    /// Longest prefix of `text` (in bytes) that fits in `max` pixels.
    pub fn fit(&self, text: &str, max: i32) -> usize {
        let mut width = 0.0;
        for (index, ch) in text.char_indices() {
            width += self.char_width(ch);
            if width > max as f32 { return index; }
        }
        text.len()
    }
}

fn libm_ceil(v: f32) -> i32 { let t = v as i32; if (t as f32) < v { t + 1 } else { t } }

pub const UI: Font = Font { weight: Weight::Regular, size: 13 };
pub const UI_BOLD: Font = Font { weight: Weight::Bold, size: 13 };
pub const MONO: Font = Font { weight: Weight::Mono, size: 13 };
pub const TITLE: Font = Font { weight: Weight::Bold, size: 20 };
