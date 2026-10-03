//! System monitor: memory use, uptime and processes, with End Process.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, Action, App, DialogResult};
use crate::font_data::{TITLE, UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;
use crate::ui::{button, ListView, Style};

const END: u32 = 1;

pub struct Monitor { list: ListView, pids: Vec<u64>, free: u64, total: u64, last: u64, self_pid: u64 }

impl Monitor {
    pub fn new() -> Self {
        let mut m = Self { list: ListView::new(&[60, 70, 100, 220]), pids: Vec::new(), free: 0, total: 0, last: 0,
            self_pid: rt::call(SYS_GETPID, 0, 0) };
        m.refresh();
        m
    }
    fn refresh(&mut self) {
        let selected = self.list.selected.and_then(|i| self.pids.get(i)).copied();
        let processes = rt::processes();
        self.pids = processes.iter().map(|p| p.pid as u64).collect();
        self.list.set_rows(processes.iter().map(|p| alloc::vec![alloc::format!("{}", p.pid), alloc::format!("{}", p.parent),
            String::from(p.state_name()), String::from(p.name())]).collect());
        self.list.selected = selected.and_then(|pid| self.pids.iter().position(|&p| p == pid));
        self.free = rt::call(SYS_FREE_FRAMES, 0, 0);
        self.total = rt::call(SYS_MEMORY_TOTAL, 0, 0);
    }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x + 16, area.y + 116, area.w - 32, area.h - 116 - 56) }
    fn end_button(area: Rect) -> Rect { Rect::new(area.right() - 136, area.bottom() - 46, 120, 32) }
    fn selected_pid(&self) -> Option<u64> { self.list.selected.and_then(|i| self.pids.get(i)).copied() }
}

impl App for Monitor {
    fn title(&self) -> String { "System Monitor".into() }
    fn icon(&self) -> Icon { Icon::Monitor }
    fn size(&self) -> (i32, i32) { (560, 470) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, SURFACE);
        let used = self.total.saturating_sub(self.free);
        let mib = |frames: u64| frames * 4 / 1024;
        c.text(&UI_BOLD, area.x + 16, area.y + 14, "Memory", TEXT_MUTED);
        c.text(&TITLE, area.x + 16, area.y + 32, &alloc::format!("{} MiB of {} MiB used", mib(used), mib(self.total)), TEXT);
        let bar = Rect::new(area.x + 16, area.y + 66, area.w - 32, 12);
        c.round_rect(bar, 6, DIVIDER, 255);
        let fill = (bar.w as u64 * used / self.total.max(1)) as i32;
        c.round_rect(Rect::new(bar.x, bar.y, fill.max(12), bar.h), 6, ACCENT, 255);
        let seconds = rt::ticks() / 100;
        c.text(&UI, area.x + 16, area.y + 88, &alloc::format!("Uptime {}:{:02}:{:02}   ·   {} processes",
            seconds / 3600, seconds / 60 % 60, seconds % 60, self.pids.len()), TEXT_MUTED);
        let list = Self::list_rect(area);
        self.list.draw(c, list, &["PID", "Parent", "State", "Program"], focused);
        c.round_outline(list, 4, BORDER, 255);
        let can_end = self.selected_pid().is_some_and(|p| p != self.self_pid);
        button(c, Self::end_button(area), "End process", Style::Danger, can_end);
    }
    fn key(&mut self, e: &InputEvent, area: Rect) -> Action {
        if e.pressed != 0 && e.key == KEY_DELETE { return self.mouse_down(Self::end_button(area).x + 1, Self::end_button(area).y + 1, area, false); }
        self.list.key(e, Self::list_rect(area));
        Action::Redraw
    }
    fn mouse_down(&mut self, x: i32, y: i32, area: Rect, _double: bool) -> Action {
        if Self::end_button(area).contains(x, y) {
            if let Some(pid) = self.selected_pid().filter(|&p| p != self.self_pid) {
                let name = self.list.rows[self.list.selected.unwrap()][3].clone();
                return Action::Dialog(Box::new(Dialog::confirm("End Process",
                    &alloc::format!("End {} (PID {})? Unsaved work in it will be lost.", name, pid), "End process", true)), END);
            }
            return Action::None;
        }
        if let Some(index) = self.list.hit(Self::list_rect(area), x, y) { self.list.selected = Some(index); }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action { self.list.wheel(delta, Self::list_rect(area)); Action::Redraw }
    fn tick(&mut self, ticks: u64) -> Action {
        if ticks.saturating_sub(self.last) < 100 { return Action::None; }
        self.last = ticks;
        self.refresh();
        Action::Redraw
    }
    fn dialog_result(&mut self, tag: u32, result: DialogResult) -> Action {
        if tag == END && matches!(result, DialogResult::Ok) {
            if let Some(pid) = self.selected_pid() {
                let ok = rt::kill(pid);
                self.refresh();
                return Action::Toast(if ok { alloc::format!("Ended process {}", pid) } else { alloc::format!("Process {} had already ended", pid) });
            }
        }
        Action::Redraw
    }
}
