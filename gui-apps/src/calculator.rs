//! Calculator — a windowed program: its own process, drawn by the desktop from the
//! intent tree it sends. Integer arithmetic with overflow and division checks; the
//! display shows "Error" instead of wrapping.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{format, string::String, vec, vec::Vec};
use atom_window::*;
use user_rt as rt;
user_rt::entry!(main);

const ADD: u32 = 10;
const SUB: u32 = 11;
const MUL: u32 = 12;
const DIV: u32 = 13;
const EQUALS: u32 = 14;
const CLEAR: u32 = 15;
const BACK: u32 = 16;

#[derive(Default)]
struct Calc { value: i64, pending: Option<(u32, i64)>, entering: bool, error: bool, history: String }

impl Calc {
    fn apply(op: u32, a: i64, b: i64) -> Option<i64> {
        match op { ADD => a.checked_add(b), SUB => a.checked_sub(b), MUL => a.checked_mul(b), DIV => a.checked_div(b), _ => None }
    }
    // The desktop's font subsets carry ASCII and ×, not ÷ or −.
    fn symbol(op: u32) -> &'static str { match op { ADD => "+", SUB => "-", MUL => "×", DIV => "/", _ => "" } }
    fn press(&mut self, key: u32) {
        if key == CLEAR { *self = Calc::default(); return; }
        if self.error { return; }
        match key {
            0..=9 => {
                let digit = key as i64;
                self.value = if self.entering {
                    match self.value.checked_mul(10).and_then(|v| v.checked_add(if self.value < 0 { -digit } else { digit })) {
                        Some(v) => v,
                        None => return, // too many digits: ignore the key
                    }
                } else { digit };
                self.entering = true;
            }
            BACK => { if self.entering { self.value /= 10; } }
            ADD | SUB | MUL | DIV | EQUALS => {
                let mut value = self.value;
                if let Some((op, left)) = self.pending.take() {
                    if self.entering || key == EQUALS {
                        match Calc::apply(op, left, value) {
                            Some(v) => value = v,
                            None => { self.error = true; self.history.clear(); return; }
                        }
                    } else {
                        value = left; // an operator pressed twice: keep the left side
                    }
                }
                self.value = value;
                self.entering = false;
                if key == EQUALS { self.history.clear(); }
                else { self.pending = Some((key, value)); self.history = format!("{} {}", value, Calc::symbol(key)); }
            }
            _ => {}
        }
    }
    fn display(&self) -> String { if self.error { String::from("Error") } else { format!("{}", self.value) } }
}

/// The keyboard shortcut each key declares (the desktop presses the button).
fn shortcut(id: u32) -> AccessKey {
    match id {
        0..=9 => AccessKey::Char((b'0' + id as u8) as char),
        ADD => AccessKey::Char('+'), SUB => AccessKey::Char('-'), MUL => AccessKey::Char('*'), DIV => AccessKey::Char('/'),
        EQUALS => AccessKey::Enter, CLEAR => AccessKey::Escape, _ => AccessKey::Backspace,
    }
}

fn key(id: u32, label: &str) -> Node {
    let b = button(id, label).grow().key(shortcut(id));
    if matches!(id, ADD | SUB | MUL | DIV | EQUALS) { b.primary() } else if id == CLEAR { b.danger() } else { b }
}

fn view(calc: &Calc) -> Node {
    let rows: Vec<Node> = [
        [(7, "7"), (8, "8"), (9, "9"), (DIV, "/")],
        [(4, "4"), (5, "5"), (6, "6"), (MUL, "×")],
        [(1, "1"), (2, "2"), (3, "3"), (SUB, "-")],
        [(CLEAR, "C"), (0, "0"), (EQUALS, "="), (ADD, "+")],
    ].iter().map(|keys| row(keys.iter().map(|&(id, label)| key(id, label)).collect()).gap(8).grow()).collect();
    let mut children = vec![
        column(vec![
            text(&calc.history).size(14).tone(Tone::Muted).align(Align::End),
            text(&calc.display()).size(36).bold().align(Align::End),
        ]).gap(4).pad(12).panel(),
        row(vec![Node::Spacer, button(BACK, "Back").key(shortcut(BACK))]),
    ];
    children.extend(rows);
    column(children).gap(8).pad(12)
}

fn main() {
    let mut window = Window::open("Calculator", 300, 440);
    let mut calc = Calc::default();
    loop {
        window.show(&view(&calc));
        // The display on the console as well, for acceptance tests.
        rt::console_print(&format!("CALC_DISPLAY {}\n", calc.display()));
        match window.wait() {
            Event::Click(id) => calc.press(id),
            Event::Close => break,
            _ => {}
        }
    }
}
