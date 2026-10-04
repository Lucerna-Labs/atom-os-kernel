//! A multi-line text editing area: lines, a caret, selection with Shift or the mouse,
//! the shared clipboard (Ctrl+A/C/X/V), auto-indent, and scrolling. The Text Editor
//! wraps it with a toolbar and file handling; windowed programs get it as their
//! `textarea` element.
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use user_rt::abi::*;
use crate::font::MONO;
use crate::gfx::{Canvas, Rect};
use crate::theme::*;

static mut CLIPBOARD: String = String::new();
fn clipboard() -> &'static mut String { unsafe { &mut *(&raw mut CLIPBOARD) } }

/// Width of the line-number gutter, when shown.
const GUTTER: i32 = 48;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos { pub line: usize, pub col: usize }

/// What a key did.
#[derive(PartialEq, Eq)]
pub enum Edit { Ignored, Moved, Changed }

pub struct TextArea {
    pub lines: Vec<String>,
    pub cursor: Pos,
    anchor: Option<Pos>,
    scroll: usize,
    hscroll: i32,
    selecting: bool,
    /// Line numbers in a gutter on the left.
    pub numbers: bool,
}

impl TextArea {
    pub fn new(numbers: bool) -> Self {
        Self { lines: alloc::vec![String::new()], cursor: Pos { line: 0, col: 0 }, anchor: None, scroll: 0, hscroll: 0,
               selecting: false, numbers }
    }
    /// Replaces the text (tabs become four spaces) and moves to the top.
    pub fn set_text(&mut self, text: &str) {
        let text = text.replace('\r', "").replace('\t', "    ");
        self.lines = text.split('\n').map(String::from).collect();
        if self.lines.is_empty() { self.lines.push(String::new()); }
        self.cursor = Pos { line: 0, col: 0 };
        self.anchor = None; self.scroll = 0; self.hscroll = 0;
    }
    pub fn text(&self) -> String { self.lines.join("\n") }
    pub fn bytes(&self) -> usize { self.lines.iter().map(|l| l.len() + 1).sum::<usize>().saturating_sub(1) }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| if anchor < self.cursor { (anchor, self.cursor) } else { (self.cursor, anchor) })
    }
    fn selected_text(&self) -> String {
        let Some((a, b)) = self.selection() else { return String::new() };
        if a.line == b.line { return self.lines[a.line][a.col..b.col].to_string(); }
        let mut out = self.lines[a.line][a.col..].to_string();
        for line in &self.lines[a.line + 1..b.line] { out.push('\n'); out.push_str(line); }
        out.push('\n'); out.push_str(&self.lines[b.line][..b.col]);
        out
    }
    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else { self.anchor = None; return false };
        let tail = self.lines[b.line][b.col..].to_string();
        self.lines[a.line].truncate(a.col);
        self.lines[a.line].push_str(&tail);
        self.lines.drain(a.line + 1..=b.line);
        self.cursor = a; self.anchor = None;
        true
    }
    pub fn insert(&mut self, text: &str) {
        self.delete_selection();
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                let rest = self.lines[self.cursor.line].split_off(self.cursor.col);
                self.lines.insert(self.cursor.line + 1, rest);
                self.cursor = Pos { line: self.cursor.line + 1, col: 0 };
            }
            let clean: String = part.chars().filter(|c| (' '..='~').contains(c)).collect();
            self.lines[self.cursor.line].insert_str(self.cursor.col, &clean);
            self.cursor.col += clean.len();
        }
    }

    // ---- geometry (`rect` is the whole area, gutter included) -------------------

    /// Pixel offset of column `col`: the kit places monospace glyphs at fractional
    /// advances, so carets and selections use the same arithmetic.
    fn col_x(col: usize) -> i32 { (col as f32 * MONO.char_width('M') + 0.5) as i32 }
    fn line_height() -> i32 { MONO.line_height() + 3 }
    fn text_rect(&self, rect: Rect) -> Rect {
        if self.numbers { Rect::new(rect.x + GUTTER, rect.y, rect.w - GUTTER, rect.h) } else { rect }
    }
    pub fn visible_lines(&self, rect: Rect) -> usize { (self.text_rect(rect).h / Self::line_height()).max(1) as usize }
    fn keep_visible(&mut self, rect: Rect) {
        let v = self.visible_lines(rect);
        if self.cursor.line < self.scroll { self.scroll = self.cursor.line; }
        if self.cursor.line >= self.scroll + v { self.scroll = self.cursor.line + 1 - v; }
        let x = Self::col_x(self.cursor.col);
        let w = self.text_rect(rect).w - 16;
        if x < self.hscroll { self.hscroll = (x - 40).max(0); }
        if x > self.hscroll + w { self.hscroll = x - w + 40; }
    }
    fn pos_at(&self, x: i32, y: i32, rect: Rect) -> Pos {
        let text = self.text_rect(rect);
        let line = (self.scroll as i32 + (y - text.y - 4).max(0) / Self::line_height()) as usize;
        let line = line.min(self.lines.len() - 1);
        let col = (((x - text.x - 6 + self.hscroll).max(0) as f32) / MONO.char_width('M') + 0.5) as usize;
        Pos { line, col: col.min(self.lines[line].len()) }
    }

    pub fn draw(&self, c: &mut Canvas, rect: Rect, focused: bool) {
        let text = self.text_rect(rect);
        c.fill(text, WINDOW);
        if self.numbers { c.fill(Rect::new(rect.x, rect.y, GUTTER, rect.h), SURFACE); }
        let lh = Self::line_height();
        let selection = self.selection();
        let saved = c.clip();
        for (slot, index) in (self.scroll..self.lines.len()).take(self.visible_lines(rect) + 1).enumerate() {
            let y = text.y + 4 + slot as i32 * lh;
            if self.numbers {
                let number = alloc::format!("{}", index + 1);
                c.set_clip(Rect::new(rect.x, rect.y, GUTTER, rect.h).intersect(&saved));
                c.text(&MONO, rect.x + GUTTER - 8 - MONO.width(&number), y, &number, if index == self.cursor.line { TEXT } else { TEXT_MUTED });
            }
            c.set_clip(text.intersect(&saved));
            let x0 = text.x + 6 - self.hscroll;
            if let Some((a, b)) = selection {
                if index >= a.line && index <= b.line {
                    let from = if index == a.line { a.col } else { 0 } as i32;
                    let to = if index == b.line { b.col as i32 } else { self.lines[index].len() as i32 + 1 };
                    let (a, b) = (Self::col_x(from as usize), Self::col_x(to as usize));
                    c.fill(Rect::new(x0 + a, y - 1, b - a, lh), if focused { SELECTION } else { DIVIDER });
                }
            }
            c.text(&MONO, x0, y, &self.lines[index], TEXT);
            if focused && index == self.cursor.line {
                c.fill(Rect::new(x0 + Self::col_x(self.cursor.col), y - 1, 2, lh), ACCENT);
            }
        }
        c.set_clip(saved);
    }

    /// Editing and navigation keys, and Ctrl+A/C/X/V.
    pub fn key(&mut self, e: &InputEvent, rect: Rect) -> Edit {
        if e.pressed == 0 { return Edit::Ignored; }
        let shift = e.modifiers & MOD_SHIFT != 0;
        if e.modifiers & MOD_CTRL != 0 {
            let edit = match e.key {
                k if k == b'a' as u16 || k == b'A' as u16 => {
                    self.anchor = Some(Pos { line: 0, col: 0 });
                    let last = self.lines.len() - 1;
                    self.cursor = Pos { line: last, col: self.lines[last].len() };
                    Edit::Moved
                }
                k if k == b'c' as u16 || k == b'C' as u16 => { if self.selection().is_some() { *clipboard() = self.selected_text(); } Edit::Moved }
                k if k == b'x' as u16 || k == b'X' as u16 => {
                    if self.selection().is_none() { return Edit::Moved; }
                    *clipboard() = self.selected_text(); self.delete_selection(); Edit::Changed
                }
                k if k == b'v' as u16 || k == b'V' as u16 => { let text = clipboard().clone(); self.insert(&text); Edit::Changed }
                _ => return Edit::Ignored,
            };
            self.keep_visible(rect);
            return edit;
        }
        let navigation = matches!(e.key, KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN | KEY_HOME | KEY_END | KEY_PAGE_UP | KEY_PAGE_DOWN);
        if navigation {
            if shift { if self.anchor.is_none() { self.anchor = Some(self.cursor); } } else { self.anchor = None; }
            let page = self.visible_lines(rect);
            let Pos { line, col } = self.cursor;
            self.cursor = match e.key {
                KEY_LEFT if col > 0 => Pos { line, col: col - 1 },
                KEY_LEFT if line > 0 => Pos { line: line - 1, col: self.lines[line - 1].len() },
                KEY_RIGHT if col < self.lines[line].len() => Pos { line, col: col + 1 },
                KEY_RIGHT if line + 1 < self.lines.len() => Pos { line: line + 1, col: 0 },
                KEY_UP if line > 0 => Pos { line: line - 1, col: col.min(self.lines[line - 1].len()) },
                KEY_DOWN if line + 1 < self.lines.len() => Pos { line: line + 1, col: col.min(self.lines[line + 1].len()) },
                KEY_HOME => Pos { line, col: 0 },
                KEY_END => Pos { line, col: self.lines[line].len() },
                KEY_PAGE_UP => { let l = line.saturating_sub(page); Pos { line: l, col: col.min(self.lines[l].len()) } }
                KEY_PAGE_DOWN => { let l = (line + page).min(self.lines.len() - 1); Pos { line: l, col: col.min(self.lines[l].len()) } }
                _ => self.cursor,
            };
            self.keep_visible(rect);
            return Edit::Moved;
        }
        match e.key {
            8 => {
                if !self.delete_selection() {
                    let Pos { line, col } = self.cursor;
                    if col > 0 { self.lines[line].remove(col - 1); self.cursor.col -= 1; }
                    else if line > 0 {
                        let rest = self.lines.remove(line);
                        self.cursor = Pos { line: line - 1, col: self.lines[line - 1].len() };
                        self.lines[line - 1].push_str(&rest);
                    }
                }
            }
            KEY_DELETE => {
                if !self.delete_selection() {
                    let Pos { line, col } = self.cursor;
                    if col < self.lines[line].len() { self.lines[line].remove(col); }
                    else if line + 1 < self.lines.len() { let next = self.lines.remove(line + 1); self.lines[line].push_str(&next); }
                }
            }
            9 => self.insert("    "),
            k if k == b'\n' as u16 => {
                // Keep the current line's indentation.
                let indent: String = self.lines[self.cursor.line].chars().take_while(|c| *c == ' ').collect();
                self.insert("\n");
                self.insert(&indent);
            }
            k if (32..127).contains(&k) => { let s = [k as u8]; self.insert(core::str::from_utf8(&s).unwrap()); }
            _ => return Edit::Ignored,
        }
        self.keep_visible(rect);
        Edit::Changed
    }

    /// Places the caret (a double click selects the word); true when inside.
    pub fn mouse_down(&mut self, x: i32, y: i32, rect: Rect, double: bool) -> bool {
        if !self.text_rect(rect).contains(x, y) { return false; }
        self.cursor = self.pos_at(x, y, rect);
        if double {
            let line = &self.lines[self.cursor.line];
            let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
            let bytes = line.as_bytes();
            let mut start = self.cursor.col.min(bytes.len());
            while start > 0 && word(bytes[start - 1]) { start -= 1; }
            let mut end = self.cursor.col.min(bytes.len());
            while end < bytes.len() && word(bytes[end]) { end += 1; }
            self.anchor = Some(Pos { line: self.cursor.line, col: start });
            self.cursor.col = end;
        } else {
            self.anchor = Some(self.cursor);
            self.selecting = true;
        }
        true
    }
    pub fn mouse_drag(&mut self, x: i32, y: i32, rect: Rect) -> bool {
        if !self.selecting { return false; }
        self.cursor = self.pos_at(x, y, rect);
        self.keep_visible(rect);
        true
    }
    pub fn mouse_up(&mut self) { self.selecting = false; }
    pub fn wheel(&mut self, delta: i32, rect: Rect) {
        let max = self.lines.len().saturating_sub(self.visible_lines(rect));
        self.scroll = (self.scroll as i32 - delta * 3).clamp(0, max as i32) as usize;
    }
}
