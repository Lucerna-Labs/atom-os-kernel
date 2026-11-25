//! System monitor: live CPU, RAM, heap, disk I/O, frame speed, and processes.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use user_rt::{self as rt, abi::*};
use super::{dialog::Dialog, Action, App, DialogResult};
use crate::font::{TITLE, UI, UI_BOLD};
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;
use crate::ui::{button, ListView, Style};
use crate::{AVG_FRAME_US, FRAME_COUNT, LAST_FRAME_US};

const END: u32 = 1;

pub struct Monitor {
    list: ListView,
    pids: Vec<u64>,
    prev_pid_ticks: Vec<(u64, u64)>,
    perf: PerfStats,
    prev_perf: Option<PerfStats>,
    last: u64,
    self_pid: u64,
}

impl Monitor {
    pub fn new() -> Self {
        let mut m = Self {
            list: ListView::new(&[48, 56, 82, 68, 86, 260]),
            pids: Vec::new(),
            prev_pid_ticks: Vec::new(),
            perf: PerfStats::default(),
            prev_perf: None,
            last: 0,
            self_pid: rt::call(SYS_GETPID, 0, 0),
        };
        m.refresh();
        m
    }
    fn refresh(&mut self) {
        let selected = self.list.selected.and_then(|i| self.pids.get(i)).copied();
        let next_perf = rt::perf().unwrap_or(self.perf);
        let window_ticks = if self.perf.ticks != 0 {
            next_perf.ticks.saturating_sub(self.perf.ticks)
        } else {
            0
        };
        let mut processes = rt::processes().unwrap_or_default();
        processes.sort_by_key(|p| p.pid);
        self.pids = processes.iter().map(|p| p.pid).collect();
        let mut next_pid_ticks = Vec::with_capacity(processes.len());
        let rows = processes.iter().map(|p| {
            next_pid_ticks.push((p.pid, p.cpu_ticks));
            let cpu_text = if window_ticks > 0 {
                let prev_cpu = self.prev_pid_ticks.iter().find(|&&(id, _)| id == p.pid).map_or(0, |&(_, t)| t);
                let pct = p.cpu_ticks.saturating_sub(prev_cpu).saturating_mul(100) / window_ticks;
                alloc::format!("{}%", pct.min(100))
            } else if next_perf.ticks > 0 {
                let pct = p.cpu_ticks.saturating_mul(100) / next_perf.ticks;
                alloc::format!("{}%", pct.min(100))
            } else {
                alloc::format!("{}t", p.cpu_ticks)
            };
            let ram_kib = p.mem_frames * 4;
            let ram_text = if ram_kib >= 1024 {
                alloc::format!("{}.{} MiB", ram_kib / 1024, (ram_kib % 1024) * 10 / 1024)
            } else {
                alloc::format!("{} KiB", ram_kib)
            };
            alloc::vec![
                alloc::format!("{}", p.pid),
                alloc::format!("{}", p.parent),
                String::from(p.state_name()),
                cpu_text,
                ram_text,
                String::from(p.name()),
            ]
        }).collect();
        self.list.set_rows(rows);
        self.list.selected = selected.and_then(|pid| self.pids.iter().position(|&p| p == pid));
        if self.perf.ticks != 0 {
            self.prev_perf = Some(self.perf);
        }
        self.perf = next_perf;
        self.prev_pid_ticks = next_pid_ticks;
    }
    fn list_rect(area: Rect) -> Rect { Rect::new(area.x + 16, area.y + 128, area.w - 32, area.h - 128 - 56) }
    fn end_button(area: Rect) -> Rect { Rect::new(area.right() - 136, area.bottom() - 46, 120, 32) }
    fn selected_pid(&self) -> Option<u64> { self.list.selected.and_then(|i| self.pids.get(i)).copied() }
}

impl App for Monitor {
    fn title(&self) -> String { "System Monitor".into() }
    fn icon(&self) -> Icon { Icon::Monitor }
    fn size(&self) -> (i32, i32) { (660, 520) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, SURFACE);
        let half = (area.w - 48) / 2;
        let rx = area.x + 32 + half;
        let rw = area.w - 48 - half;

        // Windowed deltas for CPU %, syscalls/s, ctx_sw/s over last refresh (~1 s).
        let (dt, d_busy, d_sc, d_cs) = if let Some(prev) = self.prev_perf {
            let dt = self.perf.ticks.saturating_sub(prev.ticks);
            (
                dt,
                self.perf.busy_ticks.saturating_sub(prev.busy_ticks),
                self.perf.syscalls.saturating_sub(prev.syscalls),
                self.perf.context_switches.saturating_sub(prev.context_switches),
            )
        } else {
            (self.perf.ticks, self.perf.busy_ticks, self.perf.syscalls, self.perf.context_switches)
        };
        let cpu_pct = if dt > 0 { (d_busy.saturating_mul(100) / dt).min(100) } else { 0 };
        let sc_per_s = if dt > 0 { d_sc.saturating_mul(100) / dt } else { 0 };
        let cs_per_s = if dt > 0 { d_cs.saturating_mul(100) / dt } else { 0 };
        let mhz = self.perf.tsc_khz / 1000;

        c.text(&UI_BOLD, area.x + 16, area.y + 12, "CPU", TEXT_MUTED);
        c.text_fit(&TITLE, area.x + 16, area.y + 30, &alloc::format!("{}% · {} MHz", cpu_pct, mhz), half, TEXT);
        let cpu_bar = Rect::new(area.x + 16, area.y + 62, half, 10);
        c.round_rect(cpu_bar, 5, DIVIDER, 255);
        let cpu_fill = (cpu_bar.w as u64 * cpu_pct / 100) as i32;
        if cpu_fill > 0 {
            c.round_rect(Rect::new(cpu_bar.x, cpu_bar.y, cpu_fill.max(10), cpu_bar.h), 5, ACCENT, 255);
        }
        c.text_fit(&UI, area.x + 16, area.y + 78, &alloc::format!("{}/s syscalls · {}/s ctx sw", sc_per_s, cs_per_s), half, TEXT_MUTED);

        let used = self.perf.phys_total_frames.saturating_sub(self.perf.phys_free_frames);
        let mib = |frames: u64| frames * 4 / 1024;
        let heap_used_kib = self.perf.heap_used_bytes / 1024;
        let heap_cap_kib = self.perf.heap_capacity_bytes / 1024;
        let slab_total = self.perf.slab_hits + self.perf.slab_misses;
        let slab_pct = if slab_total > 0 { self.perf.slab_hits * 100 / slab_total } else { 100 };

        c.text(&UI_BOLD, rx, area.y + 12, "Memory", TEXT_MUTED);
        c.text_fit(&TITLE, rx, area.y + 30, &alloc::format!("{} / {} MiB", mib(used), mib(self.perf.phys_total_frames)), rw, TEXT);
        let mem_bar = Rect::new(rx, area.y + 62, rw, 10);
        c.round_rect(mem_bar, 5, DIVIDER, 255);
        let mem_fill = (mem_bar.w as u64 * used / self.perf.phys_total_frames.max(1)) as i32;
        c.round_rect(Rect::new(mem_bar.x, mem_bar.y, mem_fill.max(10), mem_bar.h), 5, SUCCESS, 255);
        c.text_fit(&UI, rx, area.y + 78, &alloc::format!("Heap {}/{} KiB ({}% slab hit)", heap_used_kib, heap_cap_kib, slab_pct), rw, TEXT_MUTED);

        let seconds = self.perf.ticks / 100;
        let last_us = LAST_FRAME_US.load(Ordering::Relaxed);
        let avg_us = AVG_FRAME_US.load(Ordering::Relaxed);
        let frame_n = FRAME_COUNT.load(Ordering::Relaxed);
        let summary = alloc::format!(
            "Disk R/W {}/{} KiB ({}/{} ops) · Frame {} us (avg {}, #{}) · Up {}:{:02}:{:02}",
            self.perf.disk_read_bytes / 1024,
            self.perf.disk_write_bytes / 1024,
            self.perf.disk_reads,
            self.perf.disk_writes,
            last_us,
            avg_us,
            frame_n,
            seconds / 3600, seconds / 60 % 60, seconds % 60
        );
        c.text_fit(&UI, area.x + 16, area.y + 102, &summary, area.w - 32, TEXT_MUTED);

        let list = Self::list_rect(area);
        self.list.draw(c, list, &["PID", "Parent", "State", "CPU", "RAM", "Program"], focused);
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
                let name = self.list.rows[self.list.selected.unwrap()][5].clone();
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
