//! atom-window — a program's side of a desktop window.
//!
//! The desktop starts a windowed program with its standard output connected to the
//! window (requests: the window's size, title and intent tree) and its standard input
//! carrying events back. The program owns all state; it re-sends its tree whenever
//! that state changes and the desktop redraws. Diagnostics should go to
//! `user_rt::console_print`, since standard output is the window protocol.
//!
//! ```ignore
//! let mut win = Window::open("Counter", 240, 160);
//! let mut count = 0;
//! loop {
//!     win.show(&column(vec![text(&format!("{count}")).size(28), button(1, "Add")]).pad(12));
//!     match win.wait() { Event::Click(1) => count += 1, Event::Close => break, _ => {} }
//! }
//! ```
#![no_std]
extern crate alloc;

use alloc::collections::VecDeque;
pub use ui_intent::*;
use user_rt as rt;

pub struct Window { reader: EventReader, queue: VecDeque<Event>, closed: bool }

impl Window {
    /// Opens the window: a size hint in logical pixels and the title.
    pub fn open(title: &str, width: u16, height: u16) -> Window {
        rt::print(&Request::Window { width, height, title: title.into() }.encode());
        Window { reader: EventReader::new(), queue: VecDeque::new(), closed: false }
    }
    pub fn set_title(&mut self, title: &str) { rt::print(&Request::Title(title.into()).encode()); }
    /// Shows `tree` (replacing the previous one). False when the output was refused.
    pub fn show(&mut self, tree: &Node) -> bool { rt::try_print(&encode_show(tree)) }
    /// Fills text area `id` with `text`. False when the output was refused (the E24
    /// egress cone stops text that reads as key material).
    pub fn set_text(&mut self, id: u32, text: &str) -> bool { rt::try_print(&Request::SetText(id, text.into()).encode()) }
    /// Asks for text area `id`'s contents; they arrive as `Event::Text`.
    pub fn request_text(&mut self, id: u32) { rt::print(&Request::GetText(id).encode()); }

    /// The next event, if one has arrived. The desktop closing the window's input
    /// (it closed the window or exited) reads as `Close`.
    pub fn poll(&mut self) -> Option<Event> {
        if let Some(event) = self.queue.pop_front() { return Some(event); }
        if self.closed { return Some(Event::Close); }
        let mut buffer = [0u8; 512];
        match rt::stdin_read(&mut buffer) {
            Some(0) => { self.closed = true; Some(Event::Close) }
            Some(n) => {
                // Malformed lines are dropped: the desktop is trusted, but a
                // program never acts on an event it could not read.
                for event in self.reader.feed(&buffer[..n]).into_iter().flatten() { self.queue.push_back(event); }
                self.queue.pop_front()
            }
            None => None,
        }
    }
    /// Waits for the next event.
    pub fn wait(&mut self) -> Event {
        loop {
            if let Some(event) = self.poll() { return event; }
            rt::sleep(1);
        }
    }
    /// Waits up to `ticks` timer ticks (10 ms each); None when nothing arrived.
    pub fn wait_for(&mut self, ticks: u64) -> Option<Event> {
        let deadline = rt::ticks().saturating_add(ticks);
        loop {
            if let Some(event) = self.poll() { return Some(event); }
            if rt::ticks() >= deadline { return None; }
            rt::sleep(1);
        }
    }
}
