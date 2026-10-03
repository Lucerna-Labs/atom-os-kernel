//! The file picker: a modal Open / Save As dialog over the file system.
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{Action, App, DialogResult};
use crate::font_data::UI;
use crate::gfx::{Canvas, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::ui::{button, format_size, FieldEvent, ListView, Style, TextField};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode { Open, Save }

pub struct Picker { mode: Mode, list: ListView, field: TextField, focus_field: bool, names: Vec<String>, error: String }

impl Picker {
    pub fn new(mode: Mode, initial: &str) -> Self {
        let mut picker = Self { mode, list: ListView::new(&[300, 100, 120]), field: if mode == Mode::Save { TextField::selected(initial) } else { TextField::new(initial) },
            focus_field: mode == Mode::Save, names: Vec::new(), error: String::new() };
        picker.refresh();
        if !initial.is_empty() { picker.list.select_where(0, initial); }
        picker
    }
    fn refresh(&mut self) {
        let mut files: Vec<_> = rt::list_files().into_iter()
            .filter(|f| self.mode == Mode::Open || !f.builtin).collect();
        files.sort_by(|a, b| a.name.cmp(&b.name));
        self.names = files.iter().map(|f| f.name.clone()).collect();
        self.list.set_rows(files.iter().map(|f| alloc::vec![f.name.clone(), format_size(f.size),
            String::from(if f.builtin { "Program" } else if f.name.ends_with(".elf") { "Program" } else { "Document" })]).collect());
    }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x + 12, area.y + 12, area.w - 24, area.h - 124) }
    /// The table itself, right of the icon gutter.
    fn table_rect(area: Rect) -> Rect { let l = Self::list_rect(area); Rect::new(l.x + 26, l.y, l.w - 26, l.h) }
    fn field_rect(area: Rect) -> Rect { Rect::new(area.x + 100, area.bottom() - 100, area.w - 112, 32) }
    fn buttons(area: Rect) -> (Rect, Rect) {
        let ok = Rect::new(area.right() - 108, area.bottom() - 48, 96, 32);
        (ok, ok.offset(-104, 0))
    }
    fn choose(&mut self) -> Action {
        let name = String::from(self.field.text.trim());
        if name.is_empty() { self.error = "Enter a file name.".into(); return Action::Redraw; }
        if name.contains('/') || name.len() > 63 { self.error = "Names are flat and at most 63 characters.".into(); return Action::Redraw; }
        if self.mode == Mode::Open && !self.names.contains(&name) { self.error = "That file does not exist.".into(); return Action::Redraw; }
        Action::Finish(DialogResult::Text(name))
    }
}

impl App for Picker {
    fn title(&self) -> String { String::from(if self.mode == Mode::Open { "Open File" } else { "Save As" }) }
    fn icon(&self) -> Icon { Icon::Folder }
    fn size(&self) -> (i32, i32) { (560, 420) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, SURFACE);
        let list = Self::list_rect(area);
        c.fill(list, WINDOW);
        c.fill(Rect::new(list.x, list.y, 26, ROW_HEIGHT), SURFACE);
        self.list.draw(c, Self::table_rect(area), &["Name", "Size", "Type"], focused && !self.focus_field);
        c.round_outline(list, 4, BORDER, 255);
        // Type icons in a gutter left of the names.
        for (slot, index) in (self.list.scroll..self.names.len()).enumerate() {
            let y = list.y + ROW_HEIGHT + slot as i32 * ROW_HEIGHT;
            if y + ROW_HEIGHT > list.bottom() { break; }
            let icon = if self.names[index].ends_with(".elf") { Icon::Program } else { Icon::Document };
            icons::draw(c, icon, list.x + 6, y + 4, 18);
        }
        let field = Self::field_rect(area);
        c.text(&UI, area.x + 16, field.y + 8, "File name:", TEXT);
        self.field.draw(c, field, focused && self.focus_field);
        if !self.error.is_empty() { c.text(&UI, area.x + 16, area.bottom() - 40, &self.error, DANGER); }
        let (ok, cancel) = Self::buttons(area);
        button(c, ok, if self.mode == Mode::Open { "Open" } else { "Save" }, Style::Primary, !self.field.text.trim().is_empty());
        button(c, cancel, "Cancel", Style::Normal, true);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        if e.key == 27 { return Action::Finish(DialogResult::Cancel); }
        if e.key == 9 { self.focus_field = !self.focus_field; return Action::Redraw; }
        if e.key == b'\n' as u16 { return self.choose(); }
        // Typing a name goes to the file-name field wherever the focus is.
        if !self.focus_field && (32..127).contains(&e.key) && e.modifiers & (MOD_CTRL | MOD_ALT) == 0 {
            self.focus_field = true;
            self.field = TextField::new("");
        }
        if !self.focus_field || matches!(e.key, KEY_UP | KEY_DOWN | KEY_PAGE_UP | KEY_PAGE_DOWN) {
            if self.list.key(e, Self::table_rect(area)) {
                if let Some(s) = self.list.selected { self.field = TextField::new(&self.names[s]); }
                self.error.clear();
            }
            return Action::Redraw;
        }
        self.error.clear();
        let _ = matches!(self.field.key(e), FieldEvent::Changed);
        Action::Redraw
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, double: bool) -> Action {
        let (ok, cancel) = Self::buttons(area);
        if ok.contains(x, y) { return self.choose(); }
        if cancel.contains(x, y) { return Action::Finish(DialogResult::Cancel); }
        let field = Self::field_rect(area);
        if field.contains(x, y) { self.focus_field = true; self.field.click(x, field); return Action::Redraw; }
        if let Some(index) = self.list.hit(Self::table_rect(area), x.max(Self::table_rect(area).x), y) {
            self.list.selected = Some(index);
            self.field = TextField::new(&self.names[index]);
            self.focus_field = false;
            self.error.clear();
            if double { return self.choose(); }
        }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action { self.list.wheel(delta, Self::table_rect(area)); Action::Redraw }
}
