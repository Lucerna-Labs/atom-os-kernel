//! Text editor: the shared text area with a toolbar, a status bar, and Open / Save As
//! via the picker.
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, picker::{Mode, Picker}, Action, App, DialogResult};
use crate::font::UI;
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;
use crate::textarea::{Edit, TextArea};
use crate::ui::{button, toolbar_layout, Style};

const TOOLBAR: [&str; 4] = ["New", "Open...", "Save", "Save As..."];
const OPEN: u32 = 1;
const SAVE_AS: u32 = 2;
const DISCARD_THEN_OPEN: u32 = 3;
/// Largest document the editor holds (it keeps the text in memory as lines).
const MAX_BYTES: usize = 16 << 20;

pub struct Editor { name: Option<String>, area: TextArea, modified: bool }

impl Editor {
    pub fn new() -> Self { Self { name: None, area: TextArea::new(true), modified: false } }
    pub fn open(name: &str) -> Self {
        let mut editor = Self::new();
        editor.load(name);
        editor
    }
    fn load(&mut self, name: &str) {
        let bytes = rt::read_file(name).unwrap_or_default();
        self.area.set_text(&String::from_utf8_lossy(&bytes));
        rt::console_print(&alloc::format!("EDITOR_OPEN {} {} bytes\n", name, bytes.len()));
        self.name = Some(name.to_string());
        self.modified = false;
    }
    /// Writes the file and saves it to the data disk, so Save means saved.
    fn save_to(&mut self, path: &str) -> Action {
        let text = self.area.text();
        let short = rt::path::name(path).to_string();
        if text.len() > MAX_BYTES { return Action::Toast("The editor handles files up to 16 MB".to_string()); }
        if let Err(e) = rt::write_file_status(path, text.as_bytes()) {
            return Action::Toast(alloc::format!("Could not save {}: {}", short, e.message()));
        }
        self.name = Some(path.to_string());
        self.modified = false;
        Action::Toast(match rt::sync_status() {
            Ok(()) => alloc::format!("Saved {}", short),
            Err(rt::FsError::NotFound) => alloc::format!("Saved {} (no data disk: kept until restart)", short),
            Err(e) => alloc::format!("Saved {}, but writing to disk failed: {}", short, e.message()),
        })
    }
    fn save(&mut self) -> Action {
        match self.name.clone() {
            Some(name) if !crate::is_builtin(&name) => self.save_to(&name),
            _ => Action::Dialog(Box::new(Picker::new(Mode::Save, self.name.as_deref().unwrap_or("untitled.txt"))), SAVE_AS),
        }
    }
    /// The text region (with its line-number gutter) between toolbar and status bar.
    fn text_rect(area: Rect) -> Rect { Rect::new(area.x, area.y + 42, area.w, area.h - 42 - 26) }
    fn open_flow(&mut self) -> Action {
        if self.modified {
            return Action::Dialog(Box::new(Dialog::confirm("Unsaved Changes", "Discard your unsaved changes and open another file?", "Discard", true)), DISCARD_THEN_OPEN);
        }
        Action::Dialog(Box::new(Picker::new(Mode::Open, self.name.as_deref().map_or("", rt::path::parent_str))), OPEN)
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
        alloc::format!("{}{} - Text Editor", if self.modified { "* " } else { "" }, self.name.as_deref().map_or("Untitled", rt::path::name))
    }
    fn icon(&self) -> Icon { Icon::Document }
    fn size(&self) -> (i32, i32) { (680, 480) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(Rect::new(area.x, area.y, area.w, 42), SURFACE);
        for (i, rect) in toolbar_layout(area, &TOOLBAR).iter().enumerate() {
            button(c, *rect, TOOLBAR[i], if i == 2 && self.modified { Style::Primary } else { Style::Normal }, true);
        }
        c.fill(Rect::new(area.x, area.y + 41, area.w, 1), DIVIDER);
        self.area.draw(c, Self::text_rect(area), focused);
        let status = Rect::new(area.x, area.bottom() - 26, area.w, 26);
        c.fill(status, SURFACE);
        c.fill(Rect::new(area.x, status.y, area.w, 1), DIVIDER);
        let (lines, cursor) = (self.area.lines.len(), self.area.cursor);
        c.text(&UI, area.x + 12, status.y + 6, &alloc::format!("Ln {}, Col {}   ·   {} line{}   ·   {} bytes{}",
            cursor.line + 1, cursor.col + 1, lines, if lines == 1 { "" } else { "s" }, self.area.bytes(),
            if self.modified { "   ·   unsaved" } else { "" }), TEXT_MUTED);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        if e.modifiers & MOD_CTRL != 0 {
            let shift = e.modifiers & MOD_SHIFT != 0;
            match e.key {
                k if (k == b's' as u16 || k == b'S' as u16) && shift => return self.command(3),
                k if k == b's' as u16 || k == b'S' as u16 => return self.save(),
                k if k == b'o' as u16 || k == b'O' as u16 => return self.open_flow(),
                k if k == b'n' as u16 || k == b'N' as u16 => return self.command(0),
                _ => {}
            }
        }
        match self.area.key(e, Self::text_rect(area)) {
            Edit::Ignored => Action::None,
            Edit::Moved => Action::Redraw,
            Edit::Changed => { self.modified = true; Action::Redraw }
        }
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, double: bool) -> Action {
        for (i, rect) in toolbar_layout(area, &TOOLBAR).iter().enumerate() {
            if rect.contains(x, y) { return self.command(i); }
        }
        if self.area.mouse_down(x, y, Self::text_rect(area), double) { Action::Redraw } else { Action::None }
    }
    fn mouse_drag(&mut self, x: i32, y: i32, area: Rect) -> Action {
        if self.area.mouse_drag(x, y, Self::text_rect(area)) { Action::Redraw } else { Action::None }
    }
    fn mouse_up(&mut self, _x: i32, _y: i32, _area: Rect) -> Action { self.area.mouse_up(); Action::None }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action {
        self.area.wheel(delta, Self::text_rect(area));
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
