//! File manager: browse folders; open, create, rename, delete; save to disk.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, editor::Editor, terminal::Terminal, Action, App, DialogResult};
use crate::font::{UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::ui::{button, format_size, toolbar_layout, ListView, Style};

const TOOLBAR: [&str; 7] = ["Up", "New file", "New folder", "Open", "Rename", "Delete", "Save to disk"];
const NEW_FILE: u32 = 1;
const NEW_FOLDER: u32 = 2;
const RENAME: u32 = 3;
const DELETE: u32 = 4;
const BAR: i32 = 42;
const PATH_BAR: i32 = 30;

pub struct Files { dir: String, list: ListView, entries: Vec<rt::Entry>, status: String, last_refresh: u64 }

pub fn open_file(path: &str) -> Action {
    if path.ends_with(".elf") { Action::Open(Box::new(Terminal::run(path, ""))) }
    else { Action::Open(Box::new(Editor::open(path))) }
}

pub fn kind(entry: &rt::Entry) -> &'static str {
    if entry.dir { "Folder" } else if entry.name.ends_with(".elf") { "Program" } else { "Document" }
}
pub fn icon(entry: &rt::Entry) -> Icon {
    if entry.dir { Icon::Folder } else if entry.name.ends_with(".elf") { Icon::Program } else { Icon::Document }
}
pub fn size_text(entry: &rt::Entry) -> String {
    if entry.dir { alloc::format!("{} item{}", entry.size, if entry.size == 1 { "" } else { "s" }) } else { format_size(entry.size) }
}

impl Files {
    pub fn new() -> Self {
        let mut f = Self { dir: String::from("/"), list: ListView::new(&[280, 90, 90, 80]), entries: Vec::new(),
                           status: String::new(), last_refresh: 0 };
        f.refresh();
        f
    }
    fn path(&self, name: &str) -> String { rt::path::join(&self.dir, name) }
    fn refresh(&mut self) {
        let selected = self.selected().map(|e| e.name.clone());
        self.entries = rt::read_dir(&self.dir).unwrap_or_default();
        self.list.set_rows(self.entries.iter().map(|e| alloc::vec![e.name.clone(), size_text(e), String::from(kind(e)),
            String::from(if e.builtin { "Built in" } else if e.unsaved { "Unsaved" } else { "" })]).collect());
        if let Some(name) = selected { self.list.select_where(0, &name); }
        let info = rt::fs_info();
        let disk = if info.disk == 0 { String::from("no data disk") }
            else { alloc::format!("{} of {} used", format_size(info.needed_bytes), format_size(info.capacity_bytes)) };
        self.status = alloc::format!("{} items  ·  {}{}", self.entries.len(), disk,
            if info.unsaved != 0 { "  ·  unsaved changes" } else { "" });
    }
    fn enter(&mut self, dir: String) -> Action {
        self.dir = dir;
        self.list.selected = None;
        self.list.scroll = 0;
        self.refresh();
        rt::console_print(&alloc::format!("FILES_DIR {}\n", self.dir));
        Action::Redraw
    }
    fn selected(&self) -> Option<&rt::Entry> { self.list.selected.and_then(|i| self.entries.get(i)) }
    fn selected_locked(&self) -> bool { self.selected().is_some_and(|e| e.builtin || (self.dir == "/" && e.name == "bin")) }
    fn toolbar(area: Rect) -> Vec<Rect> { toolbar_layout(Rect::new(area.x, area.y, area.w, BAR), &TOOLBAR) }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x, area.y + BAR + PATH_BAR, area.w, area.h - BAR - PATH_BAR - 28) }
    fn table_rect(area: Rect) -> Rect { let l = Self::list_rect(area); Rect::new(l.x + 26, l.y, l.w - 26, l.h) }
    fn open_selected(&mut self) -> Action {
        let Some(entry) = self.selected().cloned() else { return Action::None };
        if entry.dir { self.enter(self.path(&entry.name)) } else { open_file(&self.path(&entry.name)) }
    }
    fn command(&mut self, index: usize) -> Action {
        let name = self.selected().map(|e| e.name.clone());
        let locked = self.selected_locked();
        match index {
            0 if self.dir != "/" => { let up = rt::path::parent(&self.dir); let from = String::from(rt::path::name(&self.dir));
                                      let action = self.enter(up); self.list.select_where(0, &from); action }
            1 => Action::Dialog(Box::new(Dialog::prompt("New File", "Name for the new file:", "untitled.txt", "Create")), NEW_FILE),
            2 => Action::Dialog(Box::new(Dialog::prompt("New Folder", "Name for the new folder:", "New folder", "Create")), NEW_FOLDER),
            3 => self.open_selected(),
            4 if !locked => name.map_or(Action::None, |n|
                Action::Dialog(Box::new(Dialog::prompt("Rename", &alloc::format!("New name for \"{}\":", n), &n, "Rename")), RENAME)),
            5 if !locked => name.map_or(Action::None, |n|
                Action::Dialog(Box::new(Dialog::confirm("Delete", &alloc::format!("Delete \"{}\"? It is removed from disk at the next save.", n), "Delete", true)), DELETE)),
            6 => {
                let result = rt::sync_status();
                self.refresh();
                Action::Toast(match result {
                    Ok(()) => String::from("All files saved to disk"),
                    Err(e) => alloc::format!("Save failed: {}", if e == rt::FsError::NotFound { "no data disk" } else { e.message() }),
                })
            }
            _ => Action::None,
        }
    }
}

impl App for Files {
    fn title(&self) -> String { "Files".into() }
    fn icon(&self) -> Icon { Icon::Folder }
    fn size(&self) -> (i32, i32) { (700, 460) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(Rect::new(area.x, area.y, area.w, BAR), SURFACE);
        let some = self.list.selected.is_some();
        let enabled = [self.dir != "/", true, true, some, some && !self.selected_locked(), some && !self.selected_locked(), true];
        for (i, rect) in Self::toolbar(area).iter().enumerate() {
            button(c, *rect, TOOLBAR[i], if i == 6 { Style::Primary } else { Style::Normal }, enabled[i]);
        }
        // Path bar: the folder being shown.
        let path = Rect::new(area.x, area.y + BAR, area.w, PATH_BAR);
        c.fill(path, WINDOW);
        c.fill(Rect::new(area.x, path.bottom() - 1, area.w, 1), DIVIDER);
        icons::draw(c, Icon::Folder, area.x + 10, path.y + 6, 18);
        c.text_fit(&UI_BOLD, area.x + 36, path.y + (PATH_BAR - UI.line_height()) / 2, &self.dir, area.w - 48, TEXT);
        let list = Self::list_rect(area);
        c.fill(list, WINDOW);
        self.list.draw(c, Self::table_rect(area), &["Name", "Size", "Type", ""], focused);
        // Type icons in the gutter, aligned with the visible rows.
        for (slot, index) in (self.list.scroll..self.entries.len()).enumerate() {
            let y = list.y + ROW_HEIGHT + slot as i32 * ROW_HEIGHT;
            if y + ROW_HEIGHT > list.bottom() { break; }
            if self.list.selected == Some(index) { c.fill(Rect::new(list.x, y, 26, ROW_HEIGHT), if focused { SELECTION } else { DIVIDER }); }
            icons::draw(c, icon(&self.entries[index]), list.x + 7, y + 4, 18);
        }
        if self.entries.is_empty() {
            c.text_centered(&UI, Rect::new(list.x, list.y + ROW_HEIGHT, list.w, 60), "This folder is empty", TEXT_MUTED);
        }
        let status = Rect::new(area.x, area.bottom() - 28, area.w, 28);
        c.fill(status, SURFACE);
        c.fill(Rect::new(area.x, status.y, area.w, 1), DIVIDER);
        c.text(&UI, area.x + 12, status.y + 7, &self.status, TEXT_MUTED);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        match e.key {
            k if k == b'\n' as u16 => self.command(3),
            8 => self.command(0),
            KEY_UP if e.modifiers & MOD_ALT != 0 => self.command(0),
            KEY_DELETE => self.command(5),
            k if k == KEY_F1 + 1 => self.command(4),
            k if k == KEY_F1 + 4 => { self.refresh(); Action::Redraw }
            _ => { self.list.key(e, Self::table_rect(area)); Action::Redraw }
        }
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, double: bool) -> Action {
        for (i, rect) in Self::toolbar(area).iter().enumerate() {
            if rect.contains(x, y) { return self.command(i); }
        }
        let list = Self::list_rect(area);
        let table = Self::table_rect(area);
        let hit = self.list.hit(table, x.max(table.x), y);
        if list.contains(x, y) { self.list.selected = hit; }
        if double && hit.is_some() { return self.command(3); }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action { self.list.wheel(delta, Self::table_rect(area)); Action::Redraw }
    fn tick(&mut self, ticks: u64) -> Action {
        // Pick up changes made by other windows or programs.
        if ticks.saturating_sub(self.last_refresh) >= 200 {
            self.last_refresh = ticks;
            let now = rt::read_dir(&self.dir);
            let same = now.as_ref().is_ok_and(|now| now.len() == self.entries.len()
                && now.iter().zip(&self.entries).all(|(a, b)| a.name == b.name && a.size == b.size && a.unsaved == b.unsaved));
            if now.is_err() { return self.enter(String::from("/")); }
            if !same { self.refresh(); return Action::Redraw; }
        }
        Action::None
    }
    fn dialog_result(&mut self, tag: u32, result: DialogResult) -> Action {
        let DialogResult::Text(text) = result else {
            if tag == DELETE && matches!(result, DialogResult::Ok) {
                if let Some(name) = self.selected().map(|e| e.name.clone()) {
                    let result = rt::remove_path(&self.path(&name));
                    self.refresh();
                    return Action::Toast(match result {
                        Ok(()) => alloc::format!("Deleted {}", name),
                        Err(rt::FsError::Busy) => alloc::format!("{} is open in another program", name),
                        Err(e) => alloc::format!("Could not delete {}: {}", name, e.message()),
                    });
                }
            }
            return Action::Redraw;
        };
        let name = String::from(text.trim());
        if name.contains('/') { return Action::Toast(String::from("Names cannot contain /")); }
        let path = self.path(&name);
        match tag {
            NEW_FILE | NEW_FOLDER => {
                if rt::stat(&path).is_ok() { return Action::Toast(alloc::format!("{} already exists", name)); }
                let result = if tag == NEW_FOLDER { rt::mkdir(&path) } else {
                    let fd = rt::open(&path);
                    if fd == ERROR { Err(rt::FsError::Invalid) } else { rt::close(fd); Ok(()) }
                };
                if let Err(e) = result { return Action::Toast(alloc::format!("Could not create {}: {}", name, e.message())); }
                self.refresh();
                self.list.select_where(0, &name);
                if tag == NEW_FOLDER { Action::Redraw } else { open_file(&path) }
            }
            RENAME => {
                let Some(old) = self.selected().map(|e| e.name.clone()) else { return Action::None };
                let result = rt::move_path(&self.path(&old), &path);
                self.refresh();
                self.list.select_where(0, if result.is_ok() { &name } else { &old });
                match result { Ok(()) => Action::Redraw, Err(e) => Action::Toast(alloc::format!("Could not rename to {}: {}", name, e.message())) }
            }
            _ => Action::Redraw,
        }
    }
}
