//! Terminal: runs a program (the shell by default) with its standard input
//! and output connected to this window through kernel pipes.
use alloc::string::String;
use alloc::vec::Vec;
use user_rt::{self as rt, abi::*};
use super::{Action, App};
use crate::font_data::MONO;
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;
use crate::theme::*;

const SCROLLBACK: usize = 2000;

pub struct Terminal { program: String, pid: Option<u64>, input: u64, output: u64, lines: Vec<String>, scroll: usize, status: Option<u64> }

impl Terminal {
    pub fn shell() -> Self { Self::run("shell.elf", "") }
    pub fn run(program: &str, args: &str) -> Self {
        let mut t = Self { program: program.into(), pid: None, input: ERROR, output: ERROR, lines: alloc::vec![String::new()], scroll: 0, status: None };
        match (rt::pipe(), rt::pipe()) {
            (Some(input), Some(output)) => {
                let pid = rt::spawn_with(program, args, input, output);
                // Keep only our ends: we write the child's input, read its output.
                rt::pipe_close(input, PIPE_READ_END);
                rt::pipe_close(output, PIPE_WRITE_END);
                t.input = input; t.output = output;
                if pid == ERROR { t.feed(&alloc::format!("Could not start {}\n", program)); } else { t.pid = Some(pid); }
            }
            _ => t.feed("No pipe handles available\n"),
        }
        t
    }
    fn feed(&mut self, text: &str) {
        for byte in text.bytes() {
            match byte {
                b'\n' => self.lines.push(String::new()),
                b'\r' => {}
                8 => { self.lines.last_mut().unwrap().pop(); }
                0x0c => { self.lines.clear(); self.lines.push(String::new()); }
                b'\t' => { let line = self.lines.last_mut().unwrap(); let pad = 4 - line.len() % 4; for _ in 0..pad { line.push(' '); } }
                32..=126 => self.lines.last_mut().unwrap().push(byte as char),
                _ => {}
            }
        }
        if self.lines.len() > SCROLLBACK { self.lines.drain(..self.lines.len() - SCROLLBACK); }
    }
    fn cols(area: Rect) -> usize { ((area.w - 20) / MONO.glyph('M').advance as i32).max(10) as usize }
    fn rows(area: Rect) -> usize { ((area.h - 16) / Self::lh()).max(1) as usize }
    fn lh() -> i32 { MONO.line_height() + 2 }
    /// Lines wrapped to the window width.
    fn wrapped(&self, cols: usize) -> Vec<&str> {
        let mut out = Vec::new();
        for line in &self.lines {
            if line.is_empty() { out.push(""); continue; }
            let mut rest = line.as_str();
            while rest.len() > cols { out.push(&rest[..cols]); rest = &rest[cols..]; }
            out.push(rest);
        }
        out
    }
}

impl App for Terminal {
    fn title(&self) -> String {
        let name = if self.program == "shell.elf" { "Terminal" } else { &self.program };
        match self.status { Some(code) => alloc::format!("{} (exited {})", name, code), None => String::from(name) }
    }
    fn icon(&self) -> Icon { Icon::Terminal }
    fn size(&self) -> (i32, i32) { (700, 460) }
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, TERMINAL_BG);
        let cols = Self::cols(area);
        let rows = Self::rows(area);
        let lines = self.wrapped(cols);
        let end = lines.len().saturating_sub(self.scroll);
        let start = end.saturating_sub(rows);
        let cw = MONO.glyph('M').advance as i32;
        for (slot, line) in lines[start..end].iter().enumerate() {
            c.text(&MONO, area.x + 10, area.y + 8 + slot as i32 * Self::lh(), line, TERMINAL_TEXT);
        }
        if self.scroll == 0 && self.status.is_none() && end > start {
            let last = lines[end - 1];
            let x = area.x + 10 + last.len() as i32 * cw;
            let y = area.y + 8 + (end - 1 - start) as i32 * Self::lh();
            if focused { c.fill(Rect::new(x, y, cw, Self::lh() - 2), rgb_cursor()); }
            else { c.outline(Rect::new(x, y, cw, Self::lh() - 2), rgb_cursor()); }
        }
    }
    fn key(&mut self, e: &InputEvent, _area: Rect) -> Action {
        if e.pressed == 0 || self.status.is_some() || e.modifiers & (MOD_CTRL | MOD_ALT) != 0 || e.key >= 128 { return Action::None; }
        let byte = e.key as u8;
        if !matches!(byte, 8 | 9 | b'\n' | 27 | 32..=126) { return Action::None; }
        self.scroll = 0;
        if rt::pipe_write(self.input, &[byte]).is_none() { self.feed("\n[input closed]\n"); }
        Action::Redraw
    }
    fn wheel(&mut self, delta: i32, area: Rect) -> Action {
        let total = self.wrapped(Self::cols(area)).len();
        let max = total.saturating_sub(Self::rows(area));
        self.scroll = (self.scroll as i32 + delta * 3).clamp(0, max as i32) as usize;
        Action::Redraw
    }
    fn tick(&mut self, _ticks: u64) -> Action {
        if self.output == ERROR || self.status.is_some() { return Action::None; }
        let mut changed = false;
        let mut buffer = [0u8; 4096];
        for _ in 0..8 {
            match rt::pipe_read(self.output, &mut buffer) {
                Some(0) => {
                    // Every writer is gone: the program (and its children) ended.
                    let code = self.pid.take().map(rt::wait).unwrap_or(ERROR);
                    self.status = Some(code);
                    self.feed(&alloc::format!("\n[{} exited with status {}]\n", self.program, code));
                    return Action::Redraw;
                }
                Some(count) => { let text = String::from_utf8_lossy(&buffer[..count]).into_owned(); self.feed(&text); changed = true; }
                None => break,
            }
        }
        if changed { Action::Redraw } else { Action::None }
    }
    fn closing(&mut self) {
        if let Some(pid) = self.pid.take() { rt::kill(pid); rt::wait(pid); }
        if self.input != ERROR { rt::pipe_close(self.input, PIPE_BOTH); }
        if self.output != ERROR { rt::pipe_close(self.output, PIPE_BOTH); }
    }
}

fn rgb_cursor() -> crate::gfx::Color { crate::gfx::rgb(125, 211, 252) }
