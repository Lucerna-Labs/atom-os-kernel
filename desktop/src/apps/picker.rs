//! The file picker: a modal Open / Save As dialog that browses folders.
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::files::{icon, kind, size_text};
use super::{Action, App, DialogResult};
use crate::font::{UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::ui::{button, FieldEvent, ListView, Style, TextField};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode { Open, Save }

pub struct Picker { mode: Mode, dir: String, list: ListView, field: TextField, focus_field: bool, entries: Vec<rt::Entry>, error: String }

impl Picker {
    /// `initial` is a path (or a bare name for the root folder): the picker opens its
    /// folder with its name in the file-name field. A path ending in `/` opens that
    /// folder with an empty name.
    pub fn new(mode: Mode, initial: &str) -> Self {
        let full = rt::path::join("/", initial);
        let (folder, name) = if initial.is_empty() || initial.ends_with('/') { (full.clone(), String::new()) }
            else { (rt::path::parent(&full), String::from(rt::path::name(&full))) };
        let dir = if rt::stat(&folder).is_ok_and(|e| e.dir) { folder } else { String::from("/") };
        let name = name.as_str();
        let mut picker = Self { mode, dir, list: ListView::new(&[290, 90, 90]),
            field: if mode == Mode::Save { TextField::selected(name) } else { TextField::new(name) },
            focus_field: mode == Mode::Save, entries: Vec::new(), error: String::new() };
        picker.refresh();
        if !name.is_empty() { picker.list.select_where(0, name); }
        picker
    }
    fn refresh(&mut self) {
        let save = self.mode == Mode::Save;
        self.entries = rt::read_dir(&self.dir).unwrap_or_default().into_iter().filter(|e| !save || !e.builtin).collect();
        self.list.set_rows(self.entries.iter().map(|e| alloc::vec![e.name.clone(), size_text(e), String::from(kind(e))]).collect());
    }
    fn enter(&mut self, dir: String) {
        self.dir = dir;
        self.list.selected = None;
        self.list.scroll = 0;
        self.error.clear();
        self.refresh();
        rt::console_print(&alloc::format!("PICKER_DIR {}\n", self.dir));
    }
    fn top(area: Rect) -> (Rect, Rect) {
        let up = Rect::new(area.x + 12, area.y + 12, 52, 30);
        (up, Rect::new(area.right() - 124, area.y + 12, 112, 30))
    }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x + 12, area.y + 54, area.w - 24, area.h - 166) }
    /// The table itself, right of the icon gutter.
    fn table_rect(area: Rect) -> Rect { let l = Self::list_rect(area); Rect::new(l.x + 26, l.y, l.w - 26, l.h) }
    fn field_rect(area: Rect) -> Rect { Rect::new(area.x + 100, area.bottom() - 100, area.w - 112, 32) }
    fn buttons(area: Rect) -> (Rect, Rect) {
        let ok = Rect::new(area.right() - 108, area.bottom() - 48, 96, 32);
        (ok, ok.offset(-104, 0))
    }
    fn up(&mut self) -> Action {
        if self.dir != "/" {
            let from = String::from(rt::path::name(&self.dir));
            self.enter(rt::path::parent(&self.dir));
            self.list.select_where(0, &from);
        }
        Action::Redraw
    }
    /// The typed name, resolved against the shown folder: a folder is entered, a file
    /// is the result (it must exist to open; its folder must exist to save).
    fn choose(&mut self) -> Action {
        let name = String::from(self.field.text.trim());
        if name.is_empty() { self.error = "Enter a file name.".into(); return Action::Redraw; }
        let path = rt::path::join(&self.dir, &name);
        match rt::stat(&path) {
            Ok(e) if e.dir => { self.enter(path); self.field = TextField::new(""); return Action::Redraw; }
            Ok(e) if self.mode == Mode::Save && e.builtin => { self.error = "Built-in programs cannot be replaced.".into(); return Action::Redraw; }
            Ok(_) => {}
            Err(_) if self.mode == Mode::Open => { self.error = "That file does not exist.".into(); return Action::Redraw; }
            Err(_) => {
                if !rt::stat(&rt::path::parent(&path)).is_ok_and(|e| e.dir) { self.error = "That folder does not exist.".into(); return Action::Redraw; }
                if rt::path::name(&path).len() > 255 { self.error = "Names are at most 255 characters.".into(); return Action::Redraw; }
            }
        }
        Action::Finish(DialogResult::Text(path))
    }
    fn new_folder(&mut self) -> Action {
        let mut n = 1;
        let mut name = String::from("New folder");
        while rt::stat(&rt::path::join(&self.dir, &name)).is_ok() { n += 1; name = alloc::format!("New folder {}", n); }
        match rt::mkdir(&rt::path::join(&self.dir, &name)) {
            Ok(()) => { self.refresh(); self.list.select_where(0, &name); self.field = TextField::selected(&name); self.focus_field = true; }
            Err(e) => self.error = alloc::format!("Could not create a folder: {}", e.message()),
        }
        Action::Redraw
    }
}

impl App for Picker {
    fn title(&self) -> String { String::from(if self.mode == Mode::Open { "Open File" } else { "Save As" }) }
    fn icon(&self) -> Icon { Icon::Folder }
    fn size(&self) -> (i32, i32) { (600, 460) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, SURFACE);
        let (up, folder) = Self::top(area);
        button(c, up, "Up", Style::Normal, self.dir != "/");
        if self.mode == Mode::Save { button(c, folder, "New folder", Style::Normal, true); }
        icons::draw(c, Icon::Folder, up.right() + 10, up.y + 6, 18);
        let path_w = if self.mode == Mode::Save { folder.x - up.right() - 48 } else { area.right() - up.right() - 48 };
        c.text_fit(&UI_BOLD, up.right() + 36, up.y + (up.h - UI.line_height()) / 2, &self.dir, path_w, TEXT);
        let list = Self::list_rect(area);
        c.fill(list, WINDOW);
        c.fill(Rect::new(list.x, list.y, 26, ROW_HEIGHT), SURFACE);
        self.list.draw(c, Self::table_rect(area), &["Name", "Size", "Type"], focused && !self.focus_field);
        c.round_outline(list, 4, BORDER, 255);
        // Type icons in a gutter left of the names.
        for (slot, index) in (self.list.scroll..self.entries.len()).enumerate() {
            let y = list.y + ROW_HEIGHT + slot as i32 * ROW_HEIGHT;
            if y + ROW_HEIGHT > list.bottom() { break; }
            icons::draw(c, icon(&self.entries[index]), list.x + 6, y + 4, 18);
        }
        if self.entries.is_empty() {
            c.text_centered(&UI, Rect::new(list.x, list.y + ROW_HEIGHT, list.w, 50), "This folder is empty", TEXT_MUTED);
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
        if e.key == KEY_UP && e.modifiers & MOD_ALT != 0 { return self.up(); }
        if !self.focus_field && e.key == 8 { return self.up(); }
        // Typing a name goes to the file-name field wherever the focus is.
        if !self.focus_field && (32..127).contains(&e.key) && e.modifiers & (MOD_CTRL | MOD_ALT) == 0 {
            self.focus_field = true;
            self.field = TextField::new("");
        }
        if !self.focus_field || matches!(e.key, KEY_UP | KEY_DOWN | KEY_PAGE_UP | KEY_PAGE_DOWN) {
            if self.list.key(e, Self::table_rect(area)) {
                if let Some(s) = self.list.selected { self.field = TextField::new(&self.entries[s].name); }
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
        let (up, folder) = Self::top(area);
        if up.contains(x, y) { return self.up(); }
        if self.mode == Mode::Save && folder.contains(x, y) { return self.new_folder(); }
        let field = Self::field_rect(area);
        if field.contains(x, y) { self.focus_field = true; self.field.click(x, field); return Action::Redraw; }
        if let Some(index) = self.list.hit(Self::table_rect(area), x.max(Self::table_rect(area).x), y) {
            self.list.selected = Some(index);
            let entry = self.entries[index].clone();
            self.focus_field = false;
            self.error.clear();
            if double && entry.dir { let path = rt::path::join(&self.dir, &entry.name); self.enter(path); return Action::Redraw; }
            if !entry.dir { self.field = TextField::new(&entry.name); }
            if double { return self.choose(); }
        }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action { self.list.wheel(delta, Self::table_rect(area)); Action::Redraw }
}
