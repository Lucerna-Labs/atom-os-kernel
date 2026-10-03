//! Text editor with selection, clipboard, and Open / Save As via the picker.
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, picker::{Mode, Picker}, Action, App, DialogResult};
use crate::font_data::{MONO, UI};
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;
use crate::ui::{button, toolbar_layout, Style};

const TOOLBAR: [&str; 4] = ["New", "Open...", "Save", "Save As..."];
const OPEN: u32 = 1;
const SAVE_AS: u32 = 2;
const DISCARD_THEN_OPEN: u32 = 3;
const MAX_BYTES: usize = 65536;

static mut CLIPBOARD: String = String::new();
fn clipboard() -> &'static mut String { unsafe { &mut *(&raw mut CLIPBOARD) } }

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Pos { line: usize, col: usize }

pub struct Editor { name: Option<String>, lines: Vec<String>, cursor: Pos, anchor: Option<Pos>, scroll: usize, hscroll: i32, modified: bool, selecting: bool }

impl Editor {
    pub fn new() -> Self {
        Self { name: None, lines: alloc::vec![String::new()], cursor: Pos { line: 0, col: 0 }, anchor: None,
            scroll: 0, hscroll: 0, modified: false, selecting: false }
    }
    pub fn open(name: &str) -> Self {
        let mut editor = Self::new();
        editor.load(name);
        editor
    }
    fn load(&mut self, name: &str) {
        let bytes = rt::read_file(name).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).replace('\r', "").replace('\t', "    ");
        self.lines = text.split('\n').map(String::from).collect();
        if self.lines.is_empty() { self.lines.push(String::new()); }
        rt::console_print(&alloc::format!("EDITOR_OPEN {} {} bytes\n", name, bytes.len()));
        self.name = Some(name.to_string());
        self.cursor = Pos { line: 0, col: 0 };
        self.anchor = None; self.scroll = 0; self.hscroll = 0; self.modified = false;
    }
    fn text(&self) -> String { self.lines.join("\n") }
    fn save_to(&mut self, name: &str) -> Action {
        let text = self.text();
        if text.len() > MAX_BYTES { return Action::Toast("Files are limited to 64 KiB".to_string()); }
        if rt::write_file(name, text.as_bytes()) {
            self.name = Some(name.to_string());
            self.modified = false;
            Action::Toast(alloc::format!("Saved {} (use Save to disk in Files to make it permanent)", name))
        } else { Action::Toast(alloc::format!("Could not save {}", name)) }
    }
    fn save(&mut self) -> Action {
        match self.name.clone() {
            Some(name) if !crate::is_builtin(&name) => self.save_to(&name),
            _ => Action::Dialog(Box::new(Picker::new(Mode::Save, self.name.as_deref().unwrap_or("untitled.txt"))), SAVE_AS),
        }
    }
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
        self.cursor = a; self.anchor = None; self.modified = true;
        true
    }
    fn insert(&mut self, text: &str) {
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
        self.modified = true;
    }
    fn char_width() -> i32 { MONO.glyph('M').advance as i32 }
    fn line_height() -> i32 { MONO.line_height() + 3 }
    fn text_rect(area: Rect) -> Rect { Rect::new(area.x + 48, area.y + 42, area.w - 48, area.h - 42 - 26) }
    fn visible_lines(area: Rect) -> usize { (Self::text_rect(area).h / Self::line_height()).max(1) as usize }
    fn keep_visible(&mut self, area: Rect) {
        let v = Self::visible_lines(area);
        if self.cursor.line < self.scroll { self.scroll = self.cursor.line; }
        if self.cursor.line >= self.scroll + v { self.scroll = self.cursor.line + 1 - v; }
        let x = self.cursor.col as i32 * Self::char_width();
        let w = Self::text_rect(area).w - 16;
        if x < self.hscroll { self.hscroll = (x - 40).max(0); }
        if x > self.hscroll + w { self.hscroll = x - w + 40; }
    }
    fn pos_at(&self, x: i32, y: i32, area: Rect) -> Pos {
        let rect = Self::text_rect(area);
        let line = (self.scroll as i32 + (y - rect.y - 4).max(0) / Self::line_height()) as usize;
        let line = line.min(self.lines.len() - 1);
        let col = ((x - rect.x - 6 + self.hscroll + Self::char_width() / 2).max(0) / Self::char_width()) as usize;
        Pos { line, col: col.min(self.lines[line].len()) }
    }
    fn open_flow(&mut self) -> Action {
        if self.modified {
            return Action::Dialog(Box::new(Dialog::confirm("Unsaved Changes", "Discard your unsaved changes and open another file?", "Discard", true)), DISCARD_THEN_OPEN);
        }
        Action::Dialog(Box::new(Picker::new(Mode::Open, "")), OPEN)
    }
    fn command(&mut self, index: usize) -> Action {
        match index {
            0 => Action::Open(Box::new(Editor::new())),
            1 => self.open_flow(),
            2 => self.save(),
            3 => Action::Dialog(Box::new(Picker::new(Mode::Save, self.name.as_deref().unwrap_or("untitled.txt"))), SAVE_AS),
            _ => Action::None,
        }
    }
}

impl App for Editor {
    fn title(&self) -> String {
        alloc::format!("{}{} - Text Editor", if self.modified { "* " } else { "" }, self.name.as_deref().unwrap_or("Untitled"))
    }
    fn icon(&self) -> Icon { Icon::Document }
    fn size(&self) -> (i32, i32) { (680, 480) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(Rect::new(area.x, area.y, area.w, 42), SURFACE);
        for (i, rect) in toolbar_layout(area, &TOOLBAR).iter().enumerate() {
            button(c, *rect, TOOLBAR[i], if i == 2 && self.modified { Style::Primary } else { Style::Normal }, true);
        }
        c.fill(Rect::new(area.x, area.y + 41, area.w, 1), DIVIDER);
        let rect = Self::text_rect(area);
        c.fill(Rect::new(area.x, rect.y, area.w, rect.h), WINDOW);
        c.fill(Rect::new(area.x, rect.y, 48, rect.h), SURFACE);
        let (lh, cw) = (Self::line_height(), Self::char_width());
        let selection = self.selection();
        let saved = c.clip();
        for (slot, index) in (self.scroll..self.lines.len()).take(Self::visible_lines(area) + 1).enumerate() {
            let y = rect.y + 4 + slot as i32 * lh;
            let number = alloc::format!("{}", index + 1);
            c.set_clip(Rect::new(area.x, rect.y, 48, rect.h).intersect(&saved));
            c.text(&MONO, area.x + 40 - MONO.width(&number), y, &number, if index == self.cursor.line { TEXT } else { TEXT_MUTED });
            c.set_clip(rect.intersect(&saved));
            let x0 = rect.x + 6 - self.hscroll;
            if let Some((a, b)) = selection {
                if index >= a.line && index <= b.line {
                    let from = if index == a.line { a.col } else { 0 } as i32;
                    let to = if index == b.line { b.col as i32 } else { self.lines[index].len() as i32 + 1 };
                    c.fill(Rect::new(x0 + from * cw, y - 1, (to - from) * cw, lh), if focused { SELECTION } else { DIVIDER });
                }
            }
            c.text(&MONO, x0, y, &self.lines[index], TEXT);
            if focused && index == self.cursor.line {
                c.fill(Rect::new(x0 + self.cursor.col as i32 * cw, y - 1, 2, lh), ACCENT);
            }
        }
        c.set_clip(saved);
        let status = Rect::new(area.x, area.bottom() - 26, area.w, 26);
        c.fill(status, SURFACE);
        c.fill(Rect::new(area.x, status.y, area.w, 1), DIVIDER);
        let bytes = self.lines.iter().map(|l| l.len() + 1).sum::<usize>().saturating_sub(1);
        c.text(&UI, area.x + 12, status.y + 6, &alloc::format!("Ln {}, Col {}   ·   {} lines   ·   {} bytes{}",
            self.cursor.line + 1, self.cursor.col + 1, self.lines.len(), bytes,
            if self.modified { "   ·   unsaved" } else { "" }), TEXT_MUTED);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        let shift = e.modifiers & MOD_SHIFT != 0;
        let ctrl = e.modifiers & MOD_CTRL != 0;
        if ctrl {
            match e.key {
                k if k == b's' as u16 || k == b'S' as u16 => return self.save(),
                k if k == b'o' as u16 || k == b'O' as u16 => return self.open_flow(),
                k if k == b'n' as u16 || k == b'N' as u16 => return self.command(0),
                k if k == b'a' as u16 || k == b'A' as u16 => {
                    self.anchor = Some(Pos { line: 0, col: 0 });
                    let last = self.lines.len() - 1;
                    self.cursor = Pos { line: last, col: self.lines[last].len() };
                }
                k if k == b'c' as u16 || k == b'C' as u16 => { if self.selection().is_some() { *clipboard() = self.selected_text(); } }
                k if k == b'x' as u16 || k == b'X' as u16 => { if self.selection().is_some() { *clipboard() = self.selected_text(); self.delete_selection(); } }
                k if k == b'v' as u16 || k == b'V' as u16 => { let text = clipboard().clone(); self.insert(&text); }
                _ => return Action::None,
            }
            self.keep_visible(area);
            return Action::Redraw;
        }
        let navigation = matches!(e.key, KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN | KEY_HOME | KEY_END | KEY_PAGE_UP | KEY_PAGE_DOWN);
        if navigation {
            if shift { if self.anchor.is_none() { self.anchor = Some(self.cursor); } } else { self.anchor = None; }
            let page = Self::visible_lines(area);
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
            self.keep_visible(area);
            return Action::Redraw;
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
                    self.modified = true;
                }
            }
            KEY_DELETE => {
                if !self.delete_selection() {
                    let Pos { line, col } = self.cursor;
                    if col < self.lines[line].len() { self.lines[line].remove(col); }
                    else if line + 1 < self.lines.len() { let next = self.lines.remove(line + 1); self.lines[line].push_str(&next); }
                    self.modified = true;
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
            _ => return Action::None,
        }
        self.keep_visible(area);
        Action::Redraw
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, double: bool) -> Action {
        for (i, rect) in toolbar_layout(area, &TOOLBAR).iter().enumerate() {
            if rect.contains(x, y) { return self.command(i); }
        }
        if Self::text_rect(area).contains(x, y) {
            self.cursor = self.pos_at(x, y, area);
            if double {
                // Select the word under the pointer.
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
            return Action::Redraw;
        }
        Action::None
    }
    fn mouse_drag(&mut self, x: i32, y: i32, area: Rect) -> Action {
        if !self.selecting { return Action::None; }
        self.cursor = self.pos_at(x, y, area);
        self.keep_visible(area);
        Action::Redraw
    }
    fn mouse_up(&mut self, _x: i32, _y: i32, _area: Rect) -> Action { self.selecting = false; Action::None }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action {
        let max = self.lines.len().saturating_sub(Self::visible_lines(area));
        self.scroll = (self.scroll as i32 - delta * 3).clamp(0, max as i32) as usize;
        Action::Redraw
    }
    fn dialog_result(&mut self, tag: u32, result: DialogResult) -> Action {
        match (tag, result) {
            (DISCARD_THEN_OPEN, DialogResult::Ok) => { self.modified = false; self.open_flow() }
            (OPEN, DialogResult::Text(name)) => { self.load(&name); Action::Redraw }
            (SAVE_AS, DialogResult::Text(name)) => {
                if crate::is_builtin(&name) { return Action::Toast("Built-in programs cannot be overwritten".to_string()); }
                self.save_to(&name)
            }
            _ => Action::Redraw,
        }
    }
}
