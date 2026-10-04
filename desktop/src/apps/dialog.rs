//! Message, confirmation and text-prompt dialogs.
use alloc::string::String;
use user_rt::abi::*;
use super::{Action, App, DialogResult};
use crate::font::{UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;
use crate::ui::{button, FieldEvent, Style, TextField};

pub struct Dialog { title: String, message: String, ok: String, danger: bool, field: Option<TextField>, cancel: bool }

impl Dialog {
    pub fn message(title: &str, message: &str) -> Self {
        Self { title: title.into(), message: message.into(), ok: "OK".into(), danger: false, field: None, cancel: false }
    }
    pub fn confirm(title: &str, message: &str, ok: &str, danger: bool) -> Self {
        Self { title: title.into(), message: message.into(), ok: ok.into(), danger, field: None, cancel: true }
    }
    pub fn prompt(title: &str, message: &str, initial: &str, ok: &str) -> Self {
        Self { title: title.into(), message: message.into(), ok: ok.into(), danger: false, field: Some(TextField::selected(initial)), cancel: true }
    }
    fn buttons(&self, area: Rect) -> (Rect, Rect) {
        let ok = Rect::new(area.right() - 108, area.bottom() - 46, 96, 32);
        (ok, ok.offset(-104, 0))
    }
    fn field_rect(area: Rect) -> Rect { Rect::new(area.x + 20, area.y + 54, area.w - 40, 32) }
    fn submit(&self) -> Action {
        Action::Finish(match &self.field { Some(f) => DialogResult::Text(f.text.clone()), None => DialogResult::Ok })
    }
}

impl App for Dialog {
    fn title(&self) -> String { self.title.clone() }
    fn icon(&self) -> Icon { Icon::Info }
    fn size(&self) -> (i32, i32) { (420, if self.field.is_some() { 190 } else { 170 }) }
    fn resizable(&self) -> bool { false }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, WINDOW);
        let mut y = area.y + 18;
        // Simple word wrap of the message.
        let mut line = String::new();
        for word in self.message.split(' ') {
            if !line.is_empty() && UI.width(&line) + UI.width(word) + 4 > area.w - 40 {
                c.text(&UI, area.x + 20, y, &line, TEXT); y += 20; line.clear();
            }
            if !line.is_empty() { line.push(' '); }
            line.push_str(word);
        }
        c.text(&UI, area.x + 20, y, &line, TEXT);
        if let Some(field) = &self.field { field.draw(c, Self::field_rect(area), focused); }
        let (ok, cancel) = self.buttons(area);
        c.fill(Rect::new(area.x, ok.y - 12, area.w, 1), DIVIDER);
        button(c, ok, &self.ok, if self.danger { Style::Danger } else { Style::Primary }, true);
        if self.cancel { button(c, cancel, "Cancel", Style::Normal, true); }
        let _ = UI_BOLD.line_height();
    }
    fn key(&mut self, e: &InputEvent, _area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        if e.key == 27 { return Action::Finish(DialogResult::Cancel); }
        if e.key == b'\n' as u16 { return self.submit(); }
        if let Some(field) = &mut self.field {
            return match field.key(e) { FieldEvent::None => Action::Redraw, _ => Action::Redraw };
        }
        Action::None
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, _double: bool) -> Action {
        let (ok, cancel) = self.buttons(area);
        if ok.contains(x, y) { return self.submit(); }
        if self.cancel && cancel.contains(x, y) { return Action::Finish(DialogResult::Cancel); }
        if let Some(field) = &mut self.field {
            let rect = Self::field_rect(area);
            if rect.contains(x, y) { field.click(x, rect); return Action::Redraw; }
        }
        Action::None
    }
}
