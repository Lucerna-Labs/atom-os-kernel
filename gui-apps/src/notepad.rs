//! Notepad — a windowed program for plain text files. The desktop holds the text
//! while it is edited (a `textarea`); Notepad does the file work: Open reads the
//! file named in the path field into the area, Save asks the desktop for the text
//! and writes it, then saves to the data disk (Save means saved).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{format, string::String, vec};
use atom_window::*;
use user_rt::{self as rt};
user_rt::entry!(main);

const NEW: u32 = 1;
const OPEN: u32 = 2;
const SAVE: u32 = 3;
const PATH: u32 = 4;
const TEXT: u32 = 5;
/// The most a document may hold (what one text block carries).
const MAX_BYTES: usize = MAX_TEXT_BYTES;

struct Notepad { path: String, dirty: bool, status: String, saving: bool }

impl Notepad {
    fn name(&self) -> &str { rt::path::name(&self.path) }
    fn title(&self) -> String { format!("{}{} - Notepad", if self.dirty { "* " } else { "" }, if self.path.is_empty() { "Untitled" } else { self.name() }) }

    fn view(&self) -> Node {
        let save = button(SAVE, "Save").key(AccessKey::Ctrl('s'));
        column(vec![
            row(vec![
                button(NEW, "New").key(AccessKey::Ctrl('n')),
                button(OPEN, "Open").key(AccessKey::Ctrl('o')),
                if self.dirty { save.primary() } else { save },
                field(PATH, &self.path).grow(),
            ]).gap(8).align(Align::Center),
            text_area(TEXT).grow(),
            text(&self.status).size(12).tone(if self.status.starts_with("Could not") { Tone::Danger } else { Tone::Muted }),
        ]).gap(8).pad(10)
    }

    fn open(&mut self, window: &mut Window) {
        let path = rt::path::join("/", &self.path);
        match rt::read_file(&path) {
            Some(bytes) if bytes.len() <= MAX_BYTES => {
                let text = String::from_utf8_lossy(&bytes);
                if window.set_text(TEXT, &text) {
                    self.status = format!("Opened {} ({} bytes)", path, bytes.len());
                    rt::console_print(&format!("NOTEPAD_OPENED {} {}\n", path, bytes.len()));
                } else {
                    self.status = format!("Could not show {}: it reads as binary data", path);
                }
                self.dirty = false;
            }
            Some(_) => self.status = format!("Could not open {}: larger than 4 MB", path),
            None => self.status = format!("Could not open {}: no such file", path),
        }
    }

    fn save(&mut self, text: &str) {
        let path = rt::path::join("/", &self.path);
        self.saving = false;
        if self.path.is_empty() { self.status = String::from("Could not save: type a file name first"); return; }
        if let Err(e) = rt::write_file_status(&path, text.as_bytes()) {
            self.status = format!("Could not save {}: {}", path, e.message());
            return;
        }
        self.dirty = false;
        self.status = match rt::sync_status() {
            Ok(()) => format!("Saved {} ({} bytes)", path, text.len()),
            Err(rt::FsError::NotFound) => format!("Saved {} (no data disk: kept until restart)", path),
            Err(e) => format!("Saved {}, but writing to disk failed: {}", path, e.message()),
        };
        rt::console_print(&format!("NOTEPAD_SAVED {} {}\n", path, text.len()));
    }
}

fn main() {
    let mut window = Window::open("Notepad", 640, 460);
    let mut pad = Notepad { path: String::from("/notepad.txt"), dirty: false, saving: false,
                            status: String::from("Ctrl+S saves, Ctrl+O opens the file named above, Ctrl+N starts over.") };
    let mut title = String::new();
    loop {
        window.show(&pad.view());
        if pad.title() != title { title = pad.title(); window.set_title(&title); }
        match window.wait() {
            Event::Change(PATH, path) => pad.path = path,
            Event::Submit(PATH, path) => { pad.path = path; pad.open(&mut window); }
            Event::Click(NEW) => { window.set_text(TEXT, ""); pad.dirty = false; pad.status = String::from("New document"); }
            Event::Click(OPEN) => pad.open(&mut window),
            Event::Click(SAVE) => { pad.saving = true; window.request_text(TEXT); }
            Event::Text(TEXT, text) if pad.saving => pad.save(&text),
            Event::Edited(TEXT) => { pad.dirty = true; pad.status = String::from("Unsaved changes"); }
            Event::Close => break,
            _ => {}
        }
    }
}
