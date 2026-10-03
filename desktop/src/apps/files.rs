//! File manager: browse, open, create, rename, delete and save to disk.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, editor::Editor, terminal::Terminal, Action, App, DialogResult};
use crate::font_data::UI;
use crate::gfx::{Canvas, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::ui::{button, format_size, toolbar_layout, ListView, Style};

const TOOLBAR: [&str; 6] = ["New", "Open", "Rename", "Delete", "Refresh", "Save to disk"];
const NEW: u32 = 1;
const RENAME: u32 = 2;
const DELETE: u32 = 3;

pub struct Files { list: ListView, files: Vec<rt::FileEntry>, status: String, last_refresh: u64 }

pub fn open_file(name: &str) -> Action {
    if name.ends_with(".elf") { Action::Open(Box::new(Terminal::run(name, ""))) }
    else { Action::Open(Box::new(Editor::open(name))) }
}

impl Files {
    pub fn new() -> Self {
        let mut f = Self { list: ListView::new(&[260, 90, 110]), files: Vec::new(), status: String::new(), last_refresh: 0 };
        f.refresh();
        f
    }
    fn refresh(&mut self) {
        let selected = self.selected_name();
        self.files = rt::list_files();
        self.files.sort_by(|a, b| (a.builtin, &a.name).cmp(&(b.builtin, &b.name)));
        self.list.set_rows(self.files.iter().map(|f| alloc::vec![f.name.clone(), format_size(f.size),
            String::from(if f.name.ends_with(".elf") { "Program" } else { "Document" }),
            String::from(if f.builtin { "Built in" } else { "" })]).collect());
        if let Some(name) = selected { self.list.select_where(0, &name); }
        let total: u32 = self.files.iter().filter(|f| !f.builtin).map(|f| f.size).sum();
        self.status = alloc::format!("{} items  ·  {} in user files", self.files.len(), format_size(total));
    }
    fn selected_name(&self) -> Option<String> { self.list.selected.and_then(|i| self.files.get(i)).map(|f| f.name.clone()) }
    fn selected_builtin(&self) -> bool { self.list.selected.and_then(|i| self.files.get(i)).is_some_and(|f| f.builtin) }
    fn toolbar(area: Rect) -> Vec<Rect> { toolbar_layout(Rect::new(area.x, area.y, area.w, 42), &TOOLBAR) }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x, area.y + 42, area.w, area.h - 42 - 28) }
    fn command(&mut self, index: usize) -> Action {
        let name = self.selected_name();
        match index {
            0 => Action::Dialog(Box::new(Dialog::prompt("New File", "Name for the new file:", "untitled.txt", "Create")), NEW),
            1 => name.map_or(Action::None, |n| open_file(&n)),
            2 if !self.selected_builtin() => name.map_or(Action::None, |n|
                Action::Dialog(Box::new(Dialog::prompt("Rename", &alloc::format!("New name for \"{}\":", n), &n, "Rename")), RENAME)),
            3 if !self.selected_builtin() => name.map_or(Action::None, |n|
                Action::Dialog(Box::new(Dialog::confirm("Delete File", &alloc::format!("Delete \"{}\"? It is removed from disk at the next save.", n), "Delete", true)), DELETE)),
            4 => { self.refresh(); Action::Redraw }
            5 => Action::Toast(String::from(if rt::sync() { "All files saved to disk" } else { "Save failed: no data disk" })),
            _ => Action::None,
        }
    }
}

impl App for Files {
    fn title(&self) -> String { "Files".into() }
    fn icon(&self) -> Icon { Icon::Folder }
    fn size(&self) -> (i32, i32) { (600, 440) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(Rect::new(area.x, area.y, area.w, 42), SURFACE);
        let enabled = [true, self.list.selected.is_some(), self.list.selected.is_some() && !self.selected_builtin(),
            self.list.selected.is_some() && !self.selected_builtin(), true, true];
        for (i, rect) in Self::toolbar(area).iter().enumerate() {
            button(c, *rect, TOOLBAR[i], if i == 5 { Style::Primary } else { Style::Normal }, enabled[i]);
        }
        let list = Self::list_rect(area);
        let mut shifted = list;
        shifted.x += 26; shifted.w -= 26;
        c.fill(list, WINDOW);
        self.list.draw(c, shifted, &["Name", "Size", "Type", ""], focused);
        // Type icons in the gutter, aligned with the visible rows.
        for (slot, index) in (self.list.scroll..self.files.len()).enumerate() {
            let y = list.y + ROW_HEIGHT + slot as i32 * ROW_HEIGHT;
            if y + ROW_HEIGHT > list.bottom() { break; }
            let icon = if self.files[index].name.ends_with(".elf") { Icon::Program } else { Icon::Document };
            if self.list.selected == Some(index) { c.fill(Rect::new(list.x, y, 26, ROW_HEIGHT), if focused { SELECTION } else { DIVIDER }); }
            icons::draw(c, icon, list.x + 7, y + 4, 18);
        }
        let status = Rect::new(area.x, area.bottom() - 28, area.w, 28);
        c.fill(status, SURFACE);
        c.fill(Rect::new(area.x, status.y, area.w, 1), DIVIDER);
        c.text(&UI, area.x + 12, status.y + 7, &self.status, TEXT_MUTED);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        let mut shifted = Self::list_rect(area); shifted.x += 26; shifted.w -= 26;
        match e.key {
            k if k == b'\n' as u16 => self.command(1),
            KEY_DELETE => self.command(3),
            k if k == KEY_F1 + 1 => self.command(2),
            k if k == KEY_F1 + 4 => self.command(4),
            _ => { self.list.key(e, shifted); Action::Redraw }
        }
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, double: bool) -> Action {
        for (i, rect) in Self::toolbar(area).iter().enumerate() {
            if rect.contains(x, y) { return self.command(i); }
        }
        let list = Self::list_rect(area);
        let mut shifted = list; shifted.x += 26; shifted.w -= 26;
        let hit = self.list.hit(shifted, x.max(shifted.x), y);
        if list.contains(x, y) { self.list.selected = hit; }
        if double && hit.is_some() { return self.command(1); }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action { self.list.wheel(delta, Self::list_rect(area)); Action::Redraw }
    fn tick(&mut self, ticks: u64) -> Action {
        // Pick up files created by other windows or programs.
        if ticks.saturating_sub(self.last_refresh) >= 200 {
            self.last_refresh = ticks;
            let mut now: Vec<_> = rt::list_files().into_iter().map(|f| (f.name, f.size)).collect();
            let mut shown: Vec<_> = self.files.iter().map(|f| (f.name.clone(), f.size)).collect();
            now.sort(); shown.sort();
            if now != shown {
                self.refresh();
                return Action::Redraw;
            }
        }
        Action::None
    }
    fn dialog_result(&mut self, tag: u32, result: DialogResult) -> Action {
        let DialogResult::Text(text) = result else {
            if tag == DELETE && matches!(result, DialogResult::Ok) {
                if let Some(name) = self.selected_name() {
                    let ok = rt::remove(&name);
                    self.refresh();
                    return Action::Toast(if ok { alloc::format!("Deleted {}", name) } else { alloc::format!("{} is open in another program", name) });
                }
            }
            return Action::Redraw;
        };
        let name = String::from(text.trim());
        match tag {
            NEW => {
                if self.files.iter().any(|f| f.name == name) { return Action::Toast(alloc::format!("{} already exists", name)); }
                let fd = rt::open(&name);
                if fd == ERROR { return Action::Toast(String::from("That name is not allowed")); }
                rt::close(fd);
                self.refresh();
                self.list.select_where(0, &name);
                open_file(&name)
            }
            RENAME => {
                let Some(old) = self.selected_name() else { return Action::None };
                let ok = rt::rename(&old, &name);
                self.refresh();
                self.list.select_where(0, if ok { &name } else { &old });
                if ok { Action::Redraw } else { Action::Toast(alloc::format!("Could not rename to {}", name)) }
            }
            _ => Action::Redraw,
        }
    }
}
