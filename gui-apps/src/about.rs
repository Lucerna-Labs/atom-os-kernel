//! About Atom OS — a windowed program (its own process; the desktop draws the tree).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{format, vec};
use atom_window::*;
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn view() -> Node {
    let total = rt::call(SYS_MEMORY_TOTAL, 0, 0) * 4 / 1024;
    let seconds = rt::ticks() / 100;
    let line = |label: &str, value: alloc::string::String| row(vec![text(label).tone(Tone::Muted), Node::Spacer, text(&value)]);
    column(vec![
        row(vec![icon("atom", 64), column(vec![
            text("Atom OS").size(28).bold(),
            text("An experimental x86-64 operating system written in Rust.").tone(Tone::Muted),
        ]).gap(4).grow()]).gap(16).align(Align::Center).pad(16).panel(),
        column(vec![
            line("Memory", format!("{} MiB", total)),
            line("Uptime", format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)),
            line("Processes", format!("{}", rt::call(SYS_TASK_COUNT, 0, 0))),
            line("Window", format!("process {}", rt::call(SYS_GETPID, 0, 0))),
        ]).gap(8).pad(8),
        Node::Spacer,
        text("Fonts: DejaVu (Bitstream Vera derived).").size(12).tone(Tone::Muted),
    ]).gap(12).pad(16)
}

fn main() {
    let mut window = Window::open("About Atom OS", 460, 340);
    loop {
        window.show(&view());
        // Refresh once a second (uptime), or close.
        if let Some(Event::Close) = window.wait_for(100) { break; }
    }
}
