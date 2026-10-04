//! The Atom OS desktop: window manager, taskbar, start menu and launcher.
#![no_std]
#![no_main]
extern crate alloc;

mod apps;
mod font;
mod gfx;
mod icons;
mod theme;
mod textarea;
mod ui;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use apps::{editor::Editor, files::Files, monitor::Monitor, remote::Remote, terminal::Terminal, Action, App, DialogResult};
use font::{UI, UI_BOLD};
use gfx::{rgb, Canvas, Color, Rect};
use icons::Icon;
use theme::*;
use user_rt::{self as rt, abi::*};

user_rt::entry!(main);

/// How far a window's shadow reaches beyond its frame.
const SHADOW_MARGIN: i32 = 24;

pub fn is_builtin(path: &str) -> bool { rt::stat(path).is_ok_and(|e| e.builtin) }

/// A windowed program: its own process, drawn from the intent tree it sends.
#[derive(Clone, PartialEq, Eq)]
struct Program { title: String, path: String, icon: Icon, size: (i32, i32) }

/// Windowed programs in the boot image (in /bin).
fn bundled_programs() -> Vec<Program> {
    alloc::vec![
        Program { title: "Calculator".into(), path: "calculator.elf".into(), icon: Icon::Calculator, size: (300, 440) },
        Program { title: "Notepad".into(), path: "notepad.elf".into(), icon: Icon::Document, size: (640, 460) },
        Program { title: "About Atom OS".into(), path: "about.elf".into(), icon: Icon::Info, size: (460, 340) },
    ]
}

/// Installed programs: one `/apps/<name>.app` manifest each, `key=value` lines:
/// `name=` (title), `program=` (path), optional `icon=` (a desktop icon name) and
/// `size=WxH`. A manifest without a name or program is skipped.
fn installed_programs() -> Vec<Program> {
    let mut out = Vec::new();
    let Ok(entries) = rt::read_dir("/apps") else { return out };
    for entry in entries.iter().filter(|e| !e.dir && e.name.ends_with(".app")) {
        let Some(bytes) = rt::read_file(&alloc::format!("/apps/{}", entry.name)) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        let (mut title, mut path, mut icon, mut size) = (None, None, Icon::Program, (520, 380));
        for line in text.lines() {
            match line.split_once('=') {
                Some(("name", v)) => title = Some(String::from(v.trim())),
                Some(("program", v)) => path = Some(String::from(v.trim())),
                Some(("icon", v)) => icon = apps::remote::icon_named(v.trim()),
                Some(("size", v)) => if let Some((w, h)) = v.trim().split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>()) { size = (w.clamp(160, 4000), h.clamp(120, 3000)); }
                },
                _ => {}
            }
        }
        if let (Some(title), Some(path)) = (title, path) {
            if !title.is_empty() && !path.is_empty() { out.push(Program { title, path, icon, size }); }
        }
    }
    out
}

#[derive(Clone, PartialEq, Eq)]
enum Launch { Files, Editor, Terminal, Monitor, Program(Program) }
impl Launch {
    fn create(&self) -> Box<dyn App> {
        match self {
            Launch::Files => Box::new(Files::new()),
            Launch::Editor => Box::new(Editor::new()),
            Launch::Terminal => Box::new(Terminal::shell()),
            Launch::Monitor => Box::new(Monitor::new()),
            Launch::Program(p) => Box::new(Remote::run(&p.path, &p.title, p.icon, p.size)),
        }
    }
}
fn desktop_icons() -> Vec<(Launch, Icon, String)> {
    let mut icons = alloc::vec![
        (Launch::Files, Icon::Folder, String::from("Files")), (Launch::Editor, Icon::Document, String::from("Text Editor")),
        (Launch::Terminal, Icon::Terminal, String::from("Terminal")), (Launch::Monitor, Icon::Monitor, String::from("System Monitor")),
    ];
    if let Some(calc) = bundled_programs().into_iter().find(|p| p.path == "calculator.elf") {
        icons.push((Launch::Program(calc.clone()), calc.icon, calc.title));
    }
    icons
}
#[derive(Clone)]
enum MenuItem { App(Launch), Sync, Exit, Restart }
/// The start menu: the built-in apps, the windowed programs (bundled, then installed
/// in /apps), then the system items. Returns the items and how many are apps.
fn menu_items() -> (Vec<(MenuItem, Icon, String)>, usize) {
    let mut items = alloc::vec![
        (MenuItem::App(Launch::Files), Icon::Folder, String::from("Files")),
        (MenuItem::App(Launch::Editor), Icon::Document, String::from("Text Editor")),
        (MenuItem::App(Launch::Terminal), Icon::Terminal, String::from("Terminal")),
        (MenuItem::App(Launch::Monitor), Icon::Monitor, String::from("System Monitor")),
    ];
    for p in bundled_programs().into_iter().chain(installed_programs()) {
        items.push((MenuItem::App(Launch::Program(p.clone())), p.icon, p.title));
    }
    let apps = items.len();
    items.push((MenuItem::Sync, Icon::Save, String::from("Save all to disk")));
    items.push((MenuItem::Exit, Icon::Exit, String::from("Exit to console")));
    items.push((MenuItem::Restart, Icon::Power, String::from("Restart")));
    (items, apps)
}

struct Window { id: u32, app: Box<dyn App>, rect: Rect, minimized: bool, maximized: Option<Rect>, parent: Option<u32>, modal: Option<u32>, tag: u32, title: String }
impl Window {
    /// Areas this window paints opaquely: everything except its rounded corners.
    fn opaque(&self) -> [Rect; 2] {
        let r = self.rect;
        [Rect::new(r.x, r.y + RADIUS, r.w, r.h - 2 * RADIUS), Rect::new(r.x + RADIUS, r.y, r.w - 2 * RADIUS, r.h)]
    }
    fn client(&self) -> Rect { Rect::new(self.rect.x + 1, self.rect.y + TITLE_HEIGHT, self.rect.w - 2, self.rect.h - TITLE_HEIGHT - 1) }
    fn title_bar(&self) -> Rect { Rect::new(self.rect.x, self.rect.y, self.rect.w, TITLE_HEIGHT) }
    /// Close, maximise and minimise buttons, right to left.
    fn buttons(&self) -> [Rect; 3] {
        let w = 44;
        let close = Rect::new(self.rect.right() - w - 1, self.rect.y + 1, w, TITLE_HEIGHT - 1);
        [close, close.offset(-w, 0), close.offset(-2 * w, 0)]
    }
    fn grip(&self) -> Rect { Rect::new(self.rect.right() - 16, self.rect.bottom() - 16, 16, 16) }
}

enum Drag { None, Move(u32, i32, i32), Resize(u32, Rect, i32, i32), App(u32) }

struct Desktop {
    canvas: Canvas, wallpaper: Vec<Color>, fb: *mut u32, pitch: usize,
    windows: Vec<Window>, next_id: u32,
    mx: i32, my: i32, buttons: u8, drag: Drag,
    last_click: (u64, i32, i32), menu_open: bool, menu_hover: Option<usize>, hover_button: Option<(u32, usize)>,
    selected_icon: Option<usize>, toast: Option<(String, u64)>, damage: Rect, cursor_drawn: Option<(i32, i32)>,
    clock: String, quit: bool, pointer: Pointer, event_time: u64,
    /// The start menu's items (rebuilt each time it opens) and how many are apps.
    menu: Vec<(MenuItem, Icon, String)>, menu_apps: usize,
    icons: Vec<(Launch, Icon, String)>,
}


/// The wallpaper, rendered once by the kit: a diagonal linear gradient with soft glows.
fn wallpaper(width: i32, height: i32, scale: i32) -> Vec<Color> {
    let mut c = Canvas::new(width, height, scale);
    let (width, height) = (c.bounds().w, c.bounds().h);
    c.diagonal(c.bounds(), rgb(24, 22, 64), rgb(13, 92, 99));
    // Glow positions are laid out on a 1024x768 design and scaled to the screen.
    let scale = |v: i32, design: i32, actual: i32| v * actual / design;
    let r_scale = |r: i32| r * width.min(height * 4 / 3) / 1024;
    for (x, y, r, a) in [(820, 170, 260, 26u32), (180, 620, 320, 22), (560, 420, 180, 18), (980, 640, 140, 24)] {
        c.glow(scale(x, 1024, width), scale(y, 768, height), r_scale(r), rgb(147, 197, 253), a, 40.0);
    }
    c.pixels
}

/// The mouse pointer, rasterized by the kit into a straight-alpha sprite: a filled arrow
/// path with a dark outline stroke.
struct Pointer { width: i32, height: i32, pixels: Vec<pmre_kit::Rgba> }
impl pmre_kit::Surface for Pointer {
    fn width(&self) -> u32 { self.width as u32 }
    fn height(&self) -> u32 { self.height as u32 }
    fn blend_over(&mut self, x: u32, y: u32, src: pmre_kit::Rgba) {
        if (x as i32) < self.width && (y as i32) < self.height {
            let i = (y as i32 * self.width + x as i32) as usize;
            self.pixels[i] = pmre_kit::paint::over(self.pixels[i], src);
        }
    }
}
impl Pointer {
    /// The 16x22 logical arrow, rasterized at `scale` physical pixels per logical pixel.
    fn new(scale: i32) -> Self {
        let (width, height) = (16 * scale, 22 * scale);
        let mut p = Pointer { width, height, pixels: alloc::vec![pmre_kit::Rgba::new(0.0, 0.0, 0.0, 0.0); (width * height) as usize] };
        let k = scale as f32;
        let arrow = [(1.0, 1.0), (1.0, 17.0), (5.0, 13.5), (8.2, 20.0), (10.6, 18.9), (7.6, 12.6), (12.6, 12.6)];
        let points: Vec<pmre_kit::Vec2> = arrow.iter().map(|&(x, y)| pmre_kit::Vec2::new(x * k, y * k)).collect();
        pmre_kit::path::fill(&mut p, &[points.clone()], pmre_kit::Paint::Solid(pmre_kit::Rgba::new(1.0, 1.0, 1.0, 1.0)), None);
        pmre_kit::path::stroke(&mut p, &[points], 1.1 * k, pmre_kit::Paint::Solid(pmre_kit::Rgba::new(0.05, 0.06, 0.09, 1.0)), None, true);
        p
    }
}

impl Desktop {
    fn screen(&self) -> Rect { self.canvas.bounds() }
    fn work_area(&self) -> Rect { let s = self.screen(); Rect::new(0, 0, s.w, s.h - TASKBAR_HEIGHT) }
    fn index(&self, id: u32) -> Option<usize> { self.windows.iter().position(|w| w.id == id) }
    fn focused(&self) -> Option<u32> { self.windows.iter().rev().find(|w| !w.minimized).map(|w| w.id) }

    fn open(&mut self, app: Box<dyn App>, parent: Option<u32>, tag: u32) -> u32 {
        let (w, h) = app.size();
        let work = self.work_area();
        let (w, h) = (w.min(work.w - 20), (h + TITLE_HEIGHT).min(work.h - 20));
        let rect = match parent.and_then(|p| self.index(p)) {
            // Dialogs are centred over their parent window.
            Some(i) => { let p = self.windows[i].rect; Rect::new(p.x + (p.w - w) / 2, p.y + (p.h - h) / 3, w, h) }
            None => {
                let n = self.windows.iter().filter(|w| w.parent.is_none()).count() as i32 % 8;
                Rect::new(140 + n * 32, 40 + n * 28, w, h)
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        let title = app.title();
        let mut window = Window { id, app, rect, minimized: false, maximized: None, parent, modal: None, tag, title };
        window.rect.x = window.rect.x.clamp(0, (work.w - w).max(0));
        window.rect.y = window.rect.y.clamp(0, (work.h - h).max(0));
        if let Some(i) = parent.and_then(|p| self.index(p)) { self.windows[i].modal = Some(id); }
        rt::console_print(&alloc::format!("WINDOW_OPEN {}\n", window.app.title()));
        self.windows.push(window);
        self.invalidate_window(id);
        self.invalidate_taskbar();
        id
    }
    fn close(&mut self, id: u32) {
        let Some(i) = self.index(id) else { return };
        // Close modal children first.
        if let Some(child) = self.windows[i].modal { self.close(child); }
        let Some(i) = self.index(id) else { return };
        self.invalidate_window(id);
        self.invalidate_taskbar();
        let mut window = self.windows.remove(i);
        window.app.closing();
        if let Some(p) = window.parent.and_then(|p| self.index(p)) { self.windows[p].modal = None; }
    }
    fn raise(&mut self, id: u32) {
        let Some(i) = self.index(id) else { return };
        if let Some(previous) = self.focused() { self.invalidate_window(previous); }
        let mut window = self.windows.remove(i);
        window.minimized = false;
        let modal = window.modal;
        self.windows.push(window);
        if let Some(child) = modal { self.raise(child); }
        self.invalidate_window(id);
        self.invalidate_taskbar();
    }
    fn minimize(&mut self, id: u32) {
        if let Some(i) = self.index(id) {
            if let Some(child) = self.windows[i].modal { self.minimize(child); }
            self.invalidate_window(id);
            if let Some(i) = self.index(id) { self.windows[i].minimized = true; }
        }
        if let Some(next) = self.focused() { self.invalidate_window(next); }
        self.invalidate_taskbar();
    }
    fn toggle_maximize(&mut self, id: u32) {
        let work = self.work_area();
        let Some(i) = self.index(id) else { return };
        if !self.windows[i].app.resizable() { return; }
        self.invalidate_window(id);
        let w = &mut self.windows[i];
        match w.maximized.take() { Some(old) => w.rect = old, None => { w.maximized = Some(w.rect); w.rect = work; } }
        rt::console_print(&alloc::format!("WINDOW_{} {}\n", if w.maximized.is_some() { "MAXIMIZE" } else { "RESTORE" }, w.app.title()));
        self.invalidate_window(id);
    }
    // ---- damage: only the changed region is re-rendered and presented ----
    fn invalidate(&mut self, rect: Rect) {
        let rect = rect.intersect(&self.screen());
        if !rect.is_empty() { self.damage = self.damage.union(&rect); }
    }
    fn invalidate_window(&mut self, id: u32) {
        if let Some(i) = self.index(id) {
            let r = self.windows[i].rect.inset(-SHADOW_MARGIN);
            self.invalidate(r);
        }
    }
    fn invalidate_taskbar(&mut self) {
        let s = self.screen();
        self.invalidate(Rect::new(0, s.h - TASKBAR_HEIGHT, s.w, TASKBAR_HEIGHT));
    }
    fn invalidate_menu(&mut self) {
        let m = self.menu_rect().inset(-24);
        self.invalidate(m);
        let start = self.start_button();
        self.invalidate(start);
    }
    fn toast_area(&self) -> Rect { let s = self.screen(); Rect::new(s.w / 3, s.h - TASKBAR_HEIGHT - 90, s.w - s.w / 3, 90) }

    fn toast(&mut self, text: String) {
        rt::console_print(&alloc::format!("TOAST {}\n", text));
        self.toast = Some((text, rt::ticks() + 300));
        let area = self.toast_area();
        self.invalidate(area);
    }

    fn apply(&mut self, id: u32, action: Action) {
        match action {
            Action::None => {}
            Action::Redraw => {
                // The window's own frame; the taskbar only when its title changed.
                let Some(i) = self.index(id) else { return };
                let title = self.windows[i].app.title();
                let rect = self.windows[i].rect;
                if title != self.windows[i].title { self.windows[i].title = title; self.invalidate_taskbar(); }
                self.invalidate(rect);
            }
            Action::Close => self.close(id),
            Action::Open(app) => { self.open(app, None, 0); }
            Action::Dialog(app, tag) => { self.open(app, Some(id), tag); }
            Action::Finish(result) => {
                let Some(i) = self.index(id) else { return };
                let (parent, tag) = (self.windows[i].parent, self.windows[i].tag);
                self.close(id);
                if let Some(p) = parent.and_then(|p| self.index(p)) {
                    let pid = self.windows[p].id;
                    let next = self.windows[p].app.dialog_result(tag, result);
                    self.raise(pid);
                    self.apply(pid, next);
                }
            }
            Action::Toast(text) => self.toast(text),
            Action::Exit => self.quit = true,
            Action::Restart => { rt::sync(); rt::call(SYS_REBOOT, 0, 0); self.toast("Restart failed: save to disk first".into()); }
        }
    }

    fn launch(&mut self, launch: Launch) {
        // Single-instance apps are raised instead of duplicated.
        let single = match &launch {
            Launch::Monitor => Some(String::from("System Monitor")),
            Launch::Program(p) => Some(p.title.clone()),
            _ => None,
        };
        if let Some(title) = single {
            if let Some(id) = self.windows.iter().find(|w| w.app.title() == title).map(|w| w.id) { self.raise(id); return; }
        }
        self.open(launch.create(), None, 0);
    }
    fn open_menu(&mut self) {
        let (menu, apps) = menu_items();
        self.menu = menu; self.menu_apps = apps;
        self.menu_open = true; self.menu_hover = None;
        self.invalidate_menu();
    }

    // ---- input ---------------------------------------------------------
    fn taskbar_buttons(&self) -> Vec<(u32, Rect)> {
        let s = self.screen();
        let mut x = 64;
        // Stable order (by creation), independent of stacking.
        let mut top: Vec<&Window> = self.windows.iter().filter(|w| w.parent.is_none()).collect();
        top.sort_by_key(|w| w.id);
        // Buttons shrink to fit between the start button and the clock.
        let available = s.w - 64 - 140;
        let width = (available / top.len().max(1) as i32 - 6).clamp(44, 180);
        top.into_iter().map(|w| {
            let r = Rect::new(x, s.h - TASKBAR_HEIGHT + 6, width, TASKBAR_HEIGHT - 12);
            x += width + 6;
            (w.id, r)
        }).collect()
    }
    fn start_button(&self) -> Rect { let s = self.screen(); Rect::new(8, s.h - TASKBAR_HEIGHT + 6, 48, TASKBAR_HEIGHT - 12) }
    fn menu_rect(&self) -> Rect { let s = self.screen(); let n = self.menu.len() as i32; Rect::new(8, s.h - TASKBAR_HEIGHT - 8 - (64 + n * 40 + 16), 300, 64 + n * 40 + 16) }
    fn menu_item(&self, index: usize) -> Rect { let m = self.menu_rect(); Rect::new(m.x + 8, m.y + 64 + index as i32 * 40 + if index >= self.menu_apps { 8 } else { 0 }, m.w - 16, 36) }
    fn icon_rect(index: usize) -> Rect { Rect::new(20, 20 + index as i32 * 96, 88, 88) }

    fn window_at(&self, x: i32, y: i32) -> Option<usize> {
        self.windows.iter().rposition(|w| !w.minimized && w.rect.contains(x, y))
    }

    fn mouse(&mut self, e: &InputEvent) {
        let s = self.screen();
        self.mx = (self.mx + e.dx as i32).clamp(0, s.w - 1);
        self.my = (self.my + e.dy as i32).clamp(0, s.h - 1);
        let (x, y) = (self.mx, self.my);
        self.event_time = e.time as u64;
        let pressed = e.buttons & !self.buttons;
        let released = self.buttons & !e.buttons;
        self.buttons = e.buttons;
        // Button-only packets are not motion (a click must not start a drag).
        if e.dx != 0 || e.dy != 0 { self.motion(x, y); }
        if pressed & MOUSE_LEFT != 0 { self.left_down(x, y); }
        if released & MOUSE_LEFT != 0 { self.left_up(x, y); }
        if e.wheel != 0 {
            if let Some(i) = self.window_at(x, y) {
                let (id, area) = (self.windows[i].id, self.windows[i].client());
                if self.windows[i].modal.is_none() { let a = self.windows[i].app.wheel(e.wheel as i32, area); self.apply(id, a); }
            }
        }
    }

    fn motion(&mut self, x: i32, y: i32) {
        match self.drag {
            Drag::Move(id, ox, oy) => if let Some(i) = self.index(id) {
                self.invalidate_window(id);
                let work = self.work_area();
                let w = &mut self.windows[i];
                if let Some(old) = w.maximized.take() { w.rect.w = old.w; w.rect.h = old.h; }
                w.rect.x = (x - ox).clamp(-w.rect.w + 80, work.w - 80);
                w.rect.y = (y - oy).clamp(0, work.h - TITLE_HEIGHT);
                self.invalidate_window(id);
            },
            Drag::Resize(id, start, sx, sy) => if let Some(i) = self.index(id) {
                self.invalidate_window(id);
                let w = &mut self.windows[i];
                w.rect.w = (start.w + x - sx).max(280);
                w.rect.h = (start.h + y - sy).max(180);
                w.maximized = None;
                self.invalidate_window(id);
            },
            Drag::App(id) => if let Some(i) = self.index(id) {
                let area = self.windows[i].client();
                let a = self.windows[i].app.mouse_drag(x, y, area);
                self.apply(id, a);
            },
            Drag::None => {
                // Hover feedback for title-bar buttons and the start menu.
                let hover = self.window_at(x, y).and_then(|i| {
                    let w = &self.windows[i];
                    w.buttons().iter().position(|b| b.contains(x, y)).map(|b| (w.id, b))
                });
                if hover != self.hover_button {
                    for (id, _) in [self.hover_button, hover].into_iter().flatten() { self.invalidate_window(id); }
                    self.hover_button = hover;
                }
                if self.menu_open {
                    let item = (0..self.menu.len()).find(|&i| self.menu_item(i).contains(x, y));
                    if item != self.menu_hover { self.menu_hover = item; self.invalidate_menu(); }
                }
            }
        }
    }

    fn left_down(&mut self, x: i32, y: i32) {
        // Hardware delivery time, so slow frames between two clicks do not split them.
        let now = self.event_time;
        let (lt, lx, ly) = self.last_click;
        let double = now.saturating_sub(lt) < 45 && (x - lx).abs() < 5 && (y - ly).abs() < 5;
        self.last_click = if double { (0, x, y) } else { (now, x, y) };
        let s = self.screen();

        if self.menu_open {
            self.menu_open = false;
            self.invalidate_menu();
            if let Some(i) = (0..self.menu.len()).find(|&i| self.menu_item(i).contains(x, y)) {
                match self.menu[i].0.clone() {
                    MenuItem::App(launch) => self.launch(launch),
                    MenuItem::Sync => { let ok = rt::sync(); self.toast(String::from(if ok { "All files saved to disk" } else { "Save failed: no data disk" })); }
                    MenuItem::Exit => self.quit = true,
                    MenuItem::Restart => self.apply(0, Action::Restart),
                }
                return;
            }
            if self.menu_rect().contains(x, y) || self.start_button().contains(x, y) { return; }
        }
        if y >= s.h - TASKBAR_HEIGHT {
            if self.start_button().contains(x, y) { self.open_menu(); return; }
            if let Some((id, _)) = self.taskbar_buttons().into_iter().find(|(_, r)| r.contains(x, y)) {
                let i = self.index(id).unwrap();
                if self.focused() == Some(id) && !self.windows[i].minimized { self.minimize(id); } else { self.raise(id); }
            }
            return;
        }
        if let Some(i) = self.window_at(x, y) {
            let id = self.windows[i].id;
            if let Some(child) = self.windows[i].modal { self.raise(child); return; }
            self.raise(id);
            let i = self.index(id).unwrap();
            let w = &self.windows[i];
            match w.buttons().iter().position(|b| b.contains(x, y)) {
                Some(0) => { let a = if w.parent.is_some() { Action::Finish(DialogResult::Cancel) } else { Action::Close }; self.apply(id, a); return; }
                Some(1) if w.parent.is_none() && w.app.resizable() => { self.toggle_maximize(id); return; }
                Some(2) if w.parent.is_none() => { self.minimize(id); return; }
                _ => {}
            }
            if w.title_bar().contains(x, y) {
                if double && w.parent.is_none() { self.toggle_maximize(id); } else { self.drag = Drag::Move(id, x - w.rect.x, y - w.rect.y); }
                return;
            }
            if w.app.resizable() && w.grip().contains(x, y) { self.drag = Drag::Resize(id, w.rect, x, y); return; }
            let area = w.client();
            if area.contains(x, y) {
                self.drag = Drag::App(id);
                let a = self.windows[i].app.mouse_down(x, y, area, double);
                self.apply(id, a);
            }
            return;
        }
        // The desktop itself: icons.
        let hit = (0..self.icons.len()).find(|&i| Self::icon_rect(i).contains(x, y));
        if hit != self.selected_icon { self.selected_icon = hit; self.invalidate(Rect::new(0, 0, 130, 20 + self.icons.len() as i32 * 96)); }
        if let (Some(i), true) = (hit, double) { self.launch(self.icons[i].0.clone()); }
    }

    fn left_up(&mut self, x: i32, y: i32) {
        if let Drag::App(id) = self.drag {
            if let Some(i) = self.index(id) {
                let area = self.windows[i].client();
                let a = self.windows[i].app.mouse_up(x, y, area);
                self.apply(id, a);
            }
        }
        self.drag = Drag::None;
    }

    fn key(&mut self, e: &InputEvent) {
        if e.pressed == 0 { return; }
        if e.key == KEY_SUPER {
            if self.menu_open { self.menu_open = false; self.invalidate_menu(); } else { self.open_menu(); }
            return;
        }
        if self.menu_open && e.key == 27 { self.menu_open = false; self.invalidate_menu(); return; }
        if e.modifiers & MOD_ALT != 0 && e.key == KEY_F1 + 3 {
            if let Some(id) = self.focused() {
                let a = if self.windows[self.index(id).unwrap()].parent.is_some() { Action::Finish(DialogResult::Cancel) } else { Action::Close };
                self.apply(id, a);
            }
            return;
        }
        if e.modifiers & MOD_ALT != 0 && e.key == 9 {
            // Cycle through top-level windows.
            if let Some(first) = self.windows.iter().position(|w| w.parent.is_none()) {
                let id = self.windows[first].id;
                self.raise(id);
            }
            return;
        }
        if let Some(id) = self.focused() {
            let i = self.index(id).unwrap();
            let area = self.windows[i].client();
            let a = self.windows[i].app.key(e, area);
            self.apply(id, a);
        }
    }

    // ---- drawing -------------------------------------------------------
    fn draw_window(&mut self, index: usize, focused: bool) {
        let c = &mut self.canvas;
        let w = &mut self.windows[index];
        let r = w.rect;
        let saved = c.clip();
        // The shadow is only visible around the window, so it is drawn in four bands
        // that leave the interior out (the kit skips everything outside the clip).
        let m = SHADOW_MARGIN;
        for band in [Rect::new(r.x - m, r.y - m, r.w + 2 * m, m + RADIUS), Rect::new(r.x - m, r.bottom() - RADIUS, r.w + 2 * m, m + RADIUS),
                     Rect::new(r.x - m, r.y + RADIUS, m + RADIUS, r.h - 2 * RADIUS), Rect::new(r.right() - RADIUS, r.y + RADIUS, m + RADIUS, r.h - 2 * RADIUS)] {
            c.set_clip(band.intersect(&saved));
            c.shadow(r, RADIUS, if focused { 14 } else { 8 }, if focused { 70 } else { 40 });
        }
        c.set_clip(saved);
        let corners = c.save_corners(r, RADIUS);
        // No body fill: the app paints its whole client area; only the title bar is ours.
        c.set_clip(Rect::new(r.x, r.y, r.w, TITLE_HEIGHT).intersect(&saved));
        c.round_rect(Rect::new(r.x, r.y, r.w, TITLE_HEIGHT + RADIUS), RADIUS, if focused { TITLE_ACTIVE } else { TITLE_INACTIVE }, 255);
        c.set_clip(saved);
        icons::draw(c, w.app.icon(), r.x + 12, r.y + 9, 16);
        let title = w.app.title();
        let buttons = w.buttons();
        c.text_fit(if focused { &UI_BOLD } else { &UI }, r.x + 36, r.y + (TITLE_HEIGHT - UI.line_height()) / 2, &title,
            buttons[2].x - r.x - 44, if focused { TEXT } else { TEXT_MUTED });
        let hover = self.hover_button.filter(|(id, _)| *id == w.id).map(|(_, b)| b);
        for (b, rect) in buttons.iter().enumerate() {
            if w.parent.is_some() && b > 0 { continue; }
            if b == 1 && !w.app.resizable() { continue; }
            if hover == Some(b) {
                let mut hr = *rect;
                if b == 0 { hr.w += 1; }
                if b == 0 { c.set_clip(Rect::new(r.x, r.y, r.w, TITLE_HEIGHT).intersect(&saved)); c.round_rect(Rect::new(hr.x, hr.y - 1, hr.w, hr.h + RADIUS), RADIUS, DANGER, 255); c.set_clip(saved); }
                else { c.fill(hr, DIVIDER); }
            }
            let (cx, cy) = ((rect.x + rect.w / 2) as f32, (rect.y + rect.h / 2) as f32);
            let ink = if hover == Some(b) && b == 0 { rgb(255, 255, 255) } else { TEXT };
            match b {
                0 => { c.line(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 1.4, ink); c.line(cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0, 1.4, ink); }
                1 => c.outline(Rect::new(cx as i32 - 5, cy as i32 - 5, 10, 10), ink),
                _ => c.fill(Rect::new(cx as i32 - 5, cy as i32, 10, 1), ink),
            }
        }
        c.fill(Rect::new(r.x + 1, r.y + TITLE_HEIGHT - 1, r.w - 2, 1), DIVIDER);
        // Client area, clipped and with rounded bottom corners restored by the outline.
        let area = w.client();
        c.set_clip(area.intersect(&saved));
        w.app.draw(c, area, focused);
        c.set_clip(saved);
        // Restore what lay behind the bottom corners, outside the rounded edge, so square
        // app content never pokes past the window outline.
        c.set_clip(saved);
        c.restore_corners(r, RADIUS, &corners);
        c.round_outline(r, RADIUS, if focused { rgb(148, 163, 184) } else { BORDER }, 255);
        if w.app.resizable() && w.maximized.is_none() {
            for i in 0..3 { let d = 4 + i * 4; c.line((r.right() - 4) as f32, (r.bottom() - d) as f32, (r.right() - d) as f32, (r.bottom() - 4) as f32, 1.0, BORDER); }
        }
    }

    /// Re-renders the `damage` region only: everything is clipped to it, and windows,
    /// menus and bars that do not touch it are skipped.
    fn render(&mut self, damage: Rect) {
        let s = self.screen();
        self.canvas.set_clip(damage);
        self.canvas.blit(&self.wallpaper, self.canvas.width, s);
        for (i, (_, icon, label)) in self.icons.iter().enumerate() {
            let r = Self::icon_rect(i);
            if self.selected_icon == Some(i) { self.canvas.round_rect(r, 8, rgb(255, 255, 255), 50); }
            icons::draw(&mut self.canvas, *icon, r.x + 20, r.y + 8, 48);
            let w = UI.width(label);
            self.canvas.text(&UI, r.x + (r.w - w) / 2 + 1, r.y + 63, label, rgb(0, 0, 0));
            self.canvas.text(&UI, r.x + (r.w - w) / 2, r.y + 62, label, rgb(255, 255, 255));
        }
        let focused = self.focused();
        // Occlusion: each window is drawn only where no window above it paints opaquely,
        // so overlapping windows are not rendered underneath one another.
        let mut visible: Vec<Vec<Rect>> = alloc::vec![Vec::new(); self.windows.len()];
        let mut uncovered = alloc::vec![damage];
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized { continue; }
            let reach = w.rect.inset(-SHADOW_MARGIN);
            visible[i] = uncovered.iter().map(|r| r.intersect(&reach)).filter(|r| !r.is_empty()).collect();
            for cut in w.opaque() {
                uncovered = uncovered.iter().flat_map(|r| r.subtract(&cut)).collect();
            }
        }
        for i in 0..self.windows.len() {
            let f = Some(self.windows[i].id) == focused;
            for part in core::mem::take(&mut visible[i]) {
                self.canvas.set_clip(part);
                self.draw_window(i, f);
            }
        }
        self.canvas.set_clip(damage);
        if !Rect::new(0, s.h - TASKBAR_HEIGHT, s.w, TASKBAR_HEIGHT).intersect(&damage).is_empty() { self.draw_taskbar(focused); }
        if self.menu_open && !self.menu_rect().inset(-24).intersect(&damage).is_empty() { self.draw_menu(); }
        if let Some((text, until)) = &self.toast {
            if rt::ticks() < *until {
                let w = UI.width(text) + 40;
                let r = Rect::new(s.w - w - 16, s.h - TASKBAR_HEIGHT - 60, w, 44);
                self.canvas.shadow(r, 10, 10, 60);
                self.canvas.round_rect(r, 10, MENU, 245);
                self.canvas.circle(r.x + 16, r.y + 22, 4, rgb(74, 222, 128), 255);
                let t = text.clone();
                self.canvas.text(&UI, r.x + 28, r.y + 14, &t, TASKBAR_TEXT);
            }
        }
    }

    fn draw_taskbar(&mut self, focused: Option<u32>) {
        let s = self.screen();
        let bar = Rect::new(0, s.h - TASKBAR_HEIGHT, s.w, TASKBAR_HEIGHT);
        self.canvas.fill_alpha(bar, TASKBAR, 225);
        self.canvas.fill(Rect::new(0, bar.y, s.w, 1), rgb(60, 66, 90));
        let start = self.start_button();
        if self.menu_open { self.canvas.round_rect(start, 8, TASKBAR_ITEM, 255); }
        icons::draw(&mut self.canvas, Icon::Atom, start.x + 10, start.y + 3, 28);
        for (id, r) in self.taskbar_buttons() {
            let i = self.index(id).unwrap();
            let active = focused == Some(id) && !self.windows[i].minimized;
            self.canvas.round_rect(r, 8, if active { TASKBAR_ITEM } else { rgb(30, 34, 48) }, 255);
            if active { self.canvas.round_rect(Rect::new(r.x + r.w / 2 - 12, r.bottom() - 3, 24, 3), 1, ACCENT, 255); }
            let icon = self.windows[i].app.icon();
            let title = self.windows[i].app.title();
            icons::draw(&mut self.canvas, icon, r.x + 10, r.y + 8, 18);
            if r.w > 60 { self.canvas.text_fit(&UI, r.x + 36, r.y + (r.h - UI.line_height()) / 2, &title, r.w - 46,
                if self.windows[i].minimized { rgb(148, 163, 184) } else { TASKBAR_TEXT }); }
        }
        let (time, date) = clock_strings(rt::time());
        self.clock = time.clone();
        let tw = UI_BOLD.width(&time).max(UI.width(&date));
        self.canvas.text(&UI_BOLD, s.w - tw - 18, bar.y + 6, &time, TASKBAR_TEXT);
        self.canvas.text(&UI, s.w - tw - 18, bar.y + 24, &date, rgb(148, 163, 184));
    }

    fn draw_menu(&mut self) {
        let m = self.menu_rect();
        let items: Vec<Rect> = (0..self.menu.len()).map(|i| self.menu_item(i)).collect();
        let (hover, apps) = (self.menu_hover, self.menu_apps);
        let c = &mut self.canvas;
        c.shadow(m, 12, 16, 90);
        c.round_rect(m, 12, MENU, 255);
        c.round_outline(m, 12, rgb(60, 68, 92), 255);
        icons::draw(c, Icon::Atom, m.x + 16, m.y + 14, 36);
        c.text(&font::TITLE, m.x + 62, m.y + 14, "Atom OS", rgb(255, 255, 255));
        c.text(&UI, m.x + 62, m.y + 38, "Applications", rgb(148, 163, 184));
        for (i, (_, icon, label)) in self.menu.iter().enumerate() {
            let r = items[i];
            if i == apps { c.fill(Rect::new(r.x + 8, r.y - 6, r.w - 16, 1), rgb(60, 68, 92)); }
            if hover == Some(i) { c.round_rect(r, 8, MENU_HOVER, 255); }
            icons::draw(c, *icon, r.x + 10, r.y + 6, 24);
            c.text(&UI, r.x + 46, r.y + (r.h - UI.line_height()) / 2, label, TASKBAR_TEXT);
        }
    }

    // ---- output --------------------------------------------------------
    fn present(&mut self, rect: Rect) {
        let r = self.canvas.physical(rect);
        for y in r.y..r.bottom() {
            let src = &self.canvas.pixels[(y * self.canvas.width + r.x) as usize..][..r.w as usize];
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), self.fb.add(y as usize * self.pitch + r.x as usize), r.w as usize); }
        }
    }
    fn cursor_rect(x: i32, y: i32) -> Rect { Rect::new(x, y, 16, 22) }
    fn draw_cursor(&mut self) {
        if let Some((ox, oy)) = self.cursor_drawn.take() { self.present(Self::cursor_rect(ox, oy)); }
        let k = self.canvas.scale;
        let (left, top) = (self.mx * k, self.my * k);
        // Composite the kit-rasterized pointer sprite over the presented frame.
        for y in 0..self.pointer.height {
            for x in 0..self.pointer.width {
                let (sx, sy) = (left + x, top + y);
                if sx >= self.canvas.width || sy >= self.canvas.height { continue; }
                let src = self.pointer.pixels[(y * self.pointer.width + x) as usize];
                if src.a <= 0.0 { continue; }
                let i = sy as usize * self.pitch + sx as usize;
                let dst = unsafe { *self.fb.add(i) };
                let mix = |shift: u32, c: f32| {
                    let d = ((dst >> shift) & 255) as f32;
                    ((d + (c * 255.0 - d) * src.a + 0.5) as u32).min(255) << shift
                };
                unsafe { *self.fb.add(i) = mix(16, src.r) | mix(8, src.g) | mix(0, src.b); }
            }
        }
        self.cursor_drawn = Some((self.mx, self.my));
    }
}

/// "HH:MM" and "Sat 3 Oct 2026" (UTC) from Unix seconds.
fn clock_strings(unix: u64) -> (String, String) {
    let days = (unix / 86400) as i64;
    let secs = unix % 86400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    (alloc::format!("{:02}:{:02} UTC", secs / 3600, secs / 60 % 60),
     alloc::format!("{} {} {} {}", WEEKDAYS[days.rem_euclid(7) as usize], day, MONTHS[(month - 1) as usize], year))
}

/// Screen size as `width << 16 | height`, for apps that report it.
pub static SCREEN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// The screen size the kernel gave the desktop.
pub fn screen_size() -> (u32, u32) {
    let v = SCREEN.load(core::sync::atomic::Ordering::Relaxed);
    (v >> 16, v & 0xffff)
}

/// Integer UI scale: keep the logical screen at least about 1080 pixels tall, so the
/// interface has the same physical size as at Full HD and the extra pixels add detail
/// (1080p and 1440p: 1x; 4K and 5K: 2x; 6K: 3x).
fn ui_scale(width: i32, height: i32) -> i32 {
    (height / 1080).min(width / 1920).clamp(1, 4)
}

fn main() {
    font::install();
    let mut info = DisplayInfo::default();
    let base = rt::call(SYS_DISPLAY_OPEN, &mut info as *mut DisplayInfo as u64, 0);
    if base == ERROR { rt::print("desktop: no display available\n"); rt::exit(1); }
    let (pw, ph) = (info.width as i32, info.height as i32);
    let scale = ui_scale(pw, ph);
    let (w, h) = (pw / scale, ph / scale);
    let (menu, menu_apps) = menu_items();
    let mut desktop = Desktop {
        canvas: Canvas::new(pw, ph, scale), wallpaper: wallpaper(pw, ph, scale), fb: base as *mut u32, pitch: (info.pitch / 4) as usize,
        windows: Vec::new(), next_id: 1, mx: w / 2, my: h / 2, buttons: 0, drag: Drag::None,
        menu, menu_apps, icons: desktop_icons(),
        last_click: (0, 0, 0), menu_open: false, menu_hover: None, hover_button: None, selected_icon: None,
        toast: None, damage: Rect::new(0, 0, w, h), cursor_drawn: None, clock: String::new(), quit: false, pointer: Pointer::new(scale), event_time: 0,
    };
    SCREEN.store((pw as u32) << 16 | ph as u32, core::sync::atomic::Ordering::Relaxed);
    rt::console_print(&alloc::format!("DESKTOP_READY {}x{} scale={}\n", pw, ph, scale));
    let mut events = [InputEvent::default(); 64];
    let mut last_clock = String::new();
    let mut toast_shown = false;
    let mut frames = 0u64;
    while !desktop.quit {
        let count = rt::call(SYS_INPUT_POLL, events.as_mut_ptr() as u64, events.len() as u64);
        let count = if count == ERROR { 0 } else { count as usize };
        let mut moved = false;
        for e in &events[..count] {
            match e.kind {
                INPUT_MOUSE => { desktop.mouse(e); moved = true; }
                INPUT_KEY => desktop.key(e),
                _ => {}
            }
        }
        let ticks = rt::ticks();
        for i in 0..desktop.windows.len() {
            if i >= desktop.windows.len() { break; }
            let id = desktop.windows[i].id;
            let a = desktop.windows[i].app.tick(ticks);
            desktop.apply(id, a);
        }
        let (time, _) = clock_strings(rt::time());
        if time != last_clock { last_clock = time; desktop.invalidate_taskbar(); }
        let toast_live = desktop.toast.as_ref().is_some_and(|(_, until)| ticks < *until);
        if toast_live != toast_shown { toast_shown = toast_live; let area = desktop.toast_area(); desktop.invalidate(area); }
        if !desktop.damage.is_empty() {
            let damage = core::mem::take(&mut desktop.damage);
            let started = rt::ticks();
            desktop.render(damage);
            frames += 1;
            if frames <= 3 || frames % 50 == 0 {
                rt::console_print(&alloc::format!("FRAME {} render_ms={}\n", frames, (rt::ticks() - started) * 10));
            }
            desktop.present(damage);
            desktop.draw_cursor();
        } else if moved {
            desktop.draw_cursor();
        }
        if count == 0 { rt::sleep(1); }
    }
    let ids: Vec<u32> = desktop.windows.iter().map(|w| w.id).collect();
    for id in ids { desktop.close(id); }
    rt::call(SYS_DISPLAY_CLOSE, 0, 0);
    rt::console_print("DESKTOP_EXIT\n");
}
