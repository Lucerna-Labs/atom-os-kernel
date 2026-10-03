//! Reusable widgets: buttons, single-line text fields and list views.
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::abi::*;
use crate::font_data::{UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::theme::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style { Normal, Primary, Danger }

pub fn button(c: &mut Canvas, rect: Rect, label: &str, style: Style, enabled: bool) {
    let (bg, fg, border) = match (style, enabled) {
        (_, false) => (SURFACE, TEXT_MUTED, DIVIDER),
        (Style::Primary, _) => (ACCENT, TEXT_ON_ACCENT, ACCENT_DARK),
        (Style::Danger, _) => (DANGER, TEXT_ON_ACCENT, DANGER),
        (Style::Normal, _) => (WINDOW, TEXT, BORDER),
    };
    c.round_rect(rect, 6, bg, 255);
    c.round_outline(rect, 6, border, 255);
    let font = if style == Style::Normal { &UI } else { &UI_BOLD };
    c.text_centered(font, rect, label, fg);
}

/// A left-to-right row of toolbar buttons; returns their rectangles.
pub fn toolbar_layout(area: Rect, labels: &[&str]) -> Vec<Rect> {
    let mut x = area.x + 8;
    labels.iter().map(|label| {
        let w = UI.width(label) + 24;
        let rect = Rect::new(x, area.y + 7, w, 28);
        x += w + 6;
        rect
    }).collect()
}

pub enum FieldEvent { None, Changed, Submit, Cancel }

#[derive(Default)]
pub struct TextField { pub text: String, pub cursor: usize, pub selected_all: bool }

impl TextField {
    pub fn new(text: &str) -> Self { Self { text: String::from(text), cursor: text.len(), selected_all: false } }
    /// Starts with the whole text selected, so typing replaces it.
    pub fn selected(text: &str) -> Self { Self { selected_all: !text.is_empty(), ..Self::new(text) } }
    pub fn key(&mut self, e: &InputEvent) -> FieldEvent {
        if e.pressed == 0 { return FieldEvent::None; }
        if self.selected_all {
            self.selected_all = false;
            let replaces = matches!(e.key, 8 | KEY_DELETE) || ((32..127).contains(&e.key) && e.modifiers & (MOD_CTRL | MOD_ALT) == 0);
            if replaces { self.text.clear(); self.cursor = 0; if matches!(e.key, 8 | KEY_DELETE) { return FieldEvent::Changed; } }
        }
        match e.key {
            k if k == b'\n' as u16 => FieldEvent::Submit,
            27 => FieldEvent::Cancel,
            8 => { if self.cursor > 0 { self.cursor -= 1; self.text.remove(self.cursor); } FieldEvent::Changed }
            KEY_DELETE => { if self.cursor < self.text.len() { self.text.remove(self.cursor); } FieldEvent::Changed }
            KEY_LEFT => { self.cursor = self.cursor.saturating_sub(1); FieldEvent::None }
            KEY_RIGHT => { self.cursor = (self.cursor + 1).min(self.text.len()); FieldEvent::None }
            KEY_HOME => { self.cursor = 0; FieldEvent::None }
            KEY_END => { self.cursor = self.text.len(); FieldEvent::None }
            k if (32..127).contains(&k) && e.modifiers & (MOD_CTRL | MOD_ALT) == 0 && self.text.len() < 63 => {
                self.text.insert(self.cursor, k as u8 as char); self.cursor += 1; FieldEvent::Changed
            }
            _ => FieldEvent::None,
        }
    }
    pub fn click(&mut self, x: i32, rect: Rect) {
        self.selected_all = false;
        let mut best = 0;
        for i in 0..=self.text.len() {
            if rect.x + 8 + UI.width(&self.text[..i]) <= x + 3 { best = i; }
        }
        self.cursor = best;
    }
    pub fn draw(&self, c: &mut Canvas, rect: Rect, focused: bool) {
        c.round_rect(rect, 6, WINDOW, 255);
        c.round_outline(rect, 6, if focused { ACCENT } else { BORDER }, 255);
        let ty = rect.y + (rect.h - UI.line_height()) / 2;
        let saved = c.clip();
        c.set_clip(rect.inset(2).intersect(&saved));
        if self.selected_all && focused {
            c.fill(Rect::new(rect.x + 7, ty, UI.width(&self.text) + 2, UI.line_height()), SELECTION);
        }
        c.text(&UI, rect.x + 8, ty, &self.text, TEXT);
        if focused && !self.selected_all {
            let cx = rect.x + 8 + UI.width(&self.text[..self.cursor]);
            c.fill(Rect::new(cx, ty, 1, UI.line_height()), ACCENT);
        }
        c.set_clip(saved);
    }
}

/// A scrolling, selectable table.
pub struct ListView { pub rows: Vec<Vec<String>>, pub widths: Vec<i32>, pub selected: Option<usize>, pub scroll: usize }

impl ListView {
    pub fn new(widths: &[i32]) -> Self { Self { rows: Vec::new(), widths: widths.to_vec(), selected: None, scroll: 0 } }
    fn visible(&self, rect: Rect) -> usize { ((rect.h - ROW_HEIGHT) / ROW_HEIGHT).max(1) as usize }
    pub fn ensure_visible(&mut self, rect: Rect) {
        if let Some(s) = self.selected {
            let v = self.visible(rect);
            if s < self.scroll { self.scroll = s; }
            if s >= self.scroll + v { self.scroll = s + 1 - v; }
        }
    }
    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        if let Some(s) = self.selected { if s >= self.rows.len() { self.selected = self.rows.len().checked_sub(1); } }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
    }
    pub fn select_where(&mut self, column: usize, value: &str) {
        self.selected = self.rows.iter().position(|r| r.get(column).map(|s| s.as_str()) == Some(value));
    }
    /// Row index under (x, y), if any.
    pub fn hit(&self, rect: Rect, x: i32, y: i32) -> Option<usize> {
        if !rect.contains(x, y) || y < rect.y + ROW_HEIGHT { return None; }
        let index = self.scroll + ((y - rect.y - ROW_HEIGHT) / ROW_HEIGHT) as usize;
        (index < self.rows.len()).then_some(index)
    }
    pub fn wheel(&mut self, delta: i32, rect: Rect) {
        let max = self.rows.len().saturating_sub(self.visible(rect));
        self.scroll = (self.scroll as i32 - delta * 3).clamp(0, max as i32) as usize;
    }
    /// Arrow/Home/End/Page navigation; true when the selection moved.
    pub fn key(&mut self, e: &InputEvent, rect: Rect) -> bool {
        if e.pressed == 0 || self.rows.is_empty() { return false; }
        let last = self.rows.len() - 1;
        let page = self.visible(rect);
        let current = self.selected;
        let next = match (e.key, current) {
            (KEY_DOWN, None) | (KEY_HOME, _) => 0,
            (KEY_UP, None) | (KEY_END, _) => last,
            (KEY_DOWN, Some(s)) => (s + 1).min(last),
            (KEY_UP, Some(s)) => s.saturating_sub(1),
            (KEY_PAGE_DOWN, s) => (s.unwrap_or(0) + page).min(last),
            (KEY_PAGE_UP, s) => s.unwrap_or(0).saturating_sub(page),
            _ => return false,
        };
        self.selected = Some(next);
        self.ensure_visible(rect);
        current != self.selected
    }
    pub fn draw(&self, c: &mut Canvas, rect: Rect, headers: &[&str], focused: bool) {
        c.fill(rect, WINDOW);
        let header = Rect::new(rect.x, rect.y, rect.w, ROW_HEIGHT);
        c.fill(header, SURFACE);
        c.fill(Rect::new(rect.x, header.bottom() - 1, rect.w, 1), DIVIDER);
        let saved = c.clip();
        c.set_clip(rect.intersect(&saved));
        let mut x = rect.x + 12;
        for (i, h) in headers.iter().enumerate() {
            c.text(&UI_BOLD, x, rect.y + (ROW_HEIGHT - UI.line_height()) / 2, h, TEXT_MUTED);
            x += self.widths.get(i).copied().unwrap_or(120);
        }
        let visible = self.visible(rect);
        for (slot, index) in (self.scroll..self.rows.len()).take(visible + 1).enumerate() {
            let y = rect.y + ROW_HEIGHT + slot as i32 * ROW_HEIGHT;
            let row_rect = Rect::new(rect.x, y, rect.w, ROW_HEIGHT);
            let selected = self.selected == Some(index);
            if selected { c.fill(row_rect, if focused { SELECTION } else { DIVIDER }); }
            else if index % 2 == 1 { c.fill(row_rect, ROW_ALT); }
            let mut x = rect.x + 12;
            for (col, text) in self.rows[index].iter().enumerate() {
                let w = self.widths.get(col).copied().unwrap_or(120);
                c.text_fit(&UI, x, y + (ROW_HEIGHT - UI.line_height()) / 2, text, w - 10, TEXT);
                x += w;
            }
        }
        if self.rows.len() > visible {
            let track = Rect::new(rect.right() - 6, rect.y + ROW_HEIGHT + 2, 4, rect.h - ROW_HEIGHT - 4);
            let thumb_h = (track.h * visible as i32 / self.rows.len() as i32).max(20);
            let thumb_y = track.y + (track.h - thumb_h) * self.scroll as i32 / (self.rows.len() - visible).max(1) as i32;
            c.round_rect(Rect::new(track.x, thumb_y, 4, thumb_h), 2, BORDER, 255);
        }
        c.set_clip(saved);
    }
}

pub fn format_size(bytes: u32) -> String {
    if bytes < 1024 { alloc::format!("{} B", bytes) }
    else if bytes < 1024 * 1024 { alloc::format!("{}.{} KB", bytes / 1024, bytes % 1024 * 10 / 1024) }
    else { alloc::format!("{}.{} MB", bytes / 1048576, bytes % 1048576 * 10 / 1048576) }
}
