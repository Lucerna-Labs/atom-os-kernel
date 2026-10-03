//! About Atom OS.
use alloc::string::String;
use user_rt::{self as rt, abi::*};
use super::{Action, App};
use crate::font::{LARGE, UI};
use crate::gfx::{Canvas, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;

pub struct About { last: u64 }
impl About { pub fn new() -> Self { Self { last: 0 } } }

impl App for About {
    fn title(&self) -> String { "About Atom OS".into() }
    fn icon(&self) -> Icon { Icon::Info }
    fn size(&self) -> (i32, i32) { (440, 330) }
    fn resizable(&self) -> bool { false }
    fn draw(&mut self, c: &mut Canvas, area: Rect, _focused: bool) {
        c.fill(area, WINDOW);
        c.gradient_v(Rect::new(area.x, area.y, area.w, 120), crate::gfx::rgb(30, 27, 75), crate::gfx::rgb(15, 118, 110));
        icons::draw(c, Icon::Atom, area.x + 24, area.y + 24, 72);
        c.text(&LARGE, area.x + 112, area.y + 36, "Atom OS", crate::gfx::rgb(255, 255, 255));
        let total = rt::call(SYS_MEMORY_TOTAL, 0, 0) * 4 / 1024;
        let seconds = rt::ticks() / 100;
        let lines = [
            String::from("An experimental x86-64 operating system written in Rust."),
            String::new(),
            alloc::format!("Memory:      {} MiB", total),
            String::from("Display:     1024 x 768, 32-bit colour"),
            alloc::format!("Uptime:      {}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60),
            alloc::format!("Processes:   {}", rt::call(SYS_TASK_COUNT, 0, 0)),
            String::new(),
            String::from("Fonts: DejaVu (Bitstream Vera derived)."),
        ];
        for (i, line) in lines.iter().enumerate() {
            c.text(&UI, area.x + 24, area.y + 136 + i as i32 * 20, line, if i == 7 { TEXT_MUTED } else { TEXT });
        }
    }
    fn tick(&mut self, ticks: u64) -> Action {
        if ticks.saturating_sub(self.last) < 100 { return Action::None; }
        self.last = ticks;
        Action::Redraw
    }
}
