//! A windowed program: a separate process that describes its window as a ui-intent
//! tree on its standard output and reads events from its standard input. This app
//! hosts it. pmre-kit's flex solver derives every rectangle from the tree (the
//! program never sends coordinates), the desktop's own widgets and theme paint them,
//! and clicks, toggles, typing and declared shortcuts go back as events.
//!
//! The bridge from the vocabulary to the kit's UXI tree is the one place that knows
//! both: every element becomes a kit box carrying an index into `elements`, so laid
//! boxes and hit tests map straight back to what the program described.
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use pmre_kit::layout::{self, LaidBox};
use pmre_kit::{Dim, Dir, Edges, Rgba, Span, Style as KitStyle, UxNode};
use pmre_kit::ux::Role;
use ui_intent::{AccessKey, Align, ButtonKind, Event, Node, Request, RequestReader, TextStyle, Tone, Weight};
use user_rt::{self as rt, abi::*};
use super::{Action, App};
use crate::font::{Font, Weight as FontWeight, UI};
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::ui::{self, TextField};

/// What an element is, for painting and input (indexed by its kit box id).
enum Element {
    Container { panel: bool },
    Text { style: TextStyle, text: String },
    Button { id: u32, kind: ButtonKind, key: Option<AccessKey>, label: String },
    Toggle { id: u32, on: bool, label: String },
    Field { id: u32 },
    Icon { name: String, size: u16 },
    Divider,
    Spacer,
}

pub struct Remote {
    program: String,
    title: String,
    icon: Icon,
    size: (i32, i32),
    pid: Option<u64>,
    /// Our ends of the two pipes: we write the program's input, read its output.
    input: u64,
    output: u64,
    outbox: Vec<u8>,
    reader: RequestReader,
    tree: Option<Node>,
    elements: Vec<Element>,
    laid: Vec<LaidBox>,
    /// The client area the current layout was solved for (None: solve again).
    laid_for: Option<Rect>,
    fields: BTreeMap<u32, TextField>,
    focus: Option<u32>,
    pressed: Option<usize>,
    reported: (i32, i32),
    error_logged: bool,
}

impl Remote {
    /// Starts `program` with its standard input and output connected to this window.
    pub fn run(program: &str, title: &str, icon: Icon, size: (i32, i32)) -> Self {
        let mut r = Remote {
            program: program.into(), title: title.into(), icon, size, pid: None, input: ERROR, output: ERROR,
            outbox: Vec::new(), reader: RequestReader::new(), tree: None, elements: Vec::new(), laid: Vec::new(),
            laid_for: None, fields: BTreeMap::new(), focus: None, pressed: None, reported: (0, 0), error_logged: false,
        };
        if let (Some(input), Some(output)) = (rt::pipe(), rt::pipe()) {
            let pid = rt::spawn_with(program, "", input, output);
            rt::pipe_close(input, PIPE_READ_END);
            rt::pipe_close(output, PIPE_WRITE_END);
            r.input = input; r.output = output;
            if pid != ERROR { r.pid = Some(pid); }
        }
        r
    }

    fn send(&mut self, event: Event) {
        self.outbox.extend_from_slice(event.encode().as_bytes());
        self.flush();
    }
    fn flush(&mut self) {
        while !self.outbox.is_empty() && self.input != ERROR {
            match rt::pipe_write(self.input, &self.outbox[..self.outbox.len().min(4096)]) {
                Some(0) => break, // full: the rest goes on a later tick
                Some(n) => { self.outbox.drain(..n); }
                None => { self.outbox.clear(); break; } // the program is gone
            }
        }
    }

    /// Rebuilds the element table and the kit tree for `area`, and solves the layout.
    fn solve(&mut self, area: Rect) {
        self.elements.clear();
        self.laid.clear();
        self.laid_for = Some(area);
        let Some(tree) = self.tree.take() else { return };
        let mut seen = Vec::new();
        let mut root = bridge(&tree, Dir::Column, &mut self.elements, &mut self.fields, self.focus, &mut seen);
        self.fields.retain(|id, _| seen.contains(id));
        if self.focus.is_some_and(|f| !seen.contains(&f)) { self.focus = None; }
        // The root fills the client area.
        if let UxNode::Box { style, .. } = &mut root {
            style.width = Dim::Px(area.w as f32);
            style.height = Dim::Px(area.h as f32);
        }
        let viewport = pmre_kit::Bounds { min: pmre_kit::Vec2::new(area.x as f32, area.y as f32),
            max: pmre_kit::Vec2::new(area.right() as f32, area.bottom() as f32) };
        self.laid = layout::solve(&root, viewport, &|_| 0.0);
        self.tree = Some(tree);
    }

    fn element_at(&self, x: i32, y: i32) -> Option<usize> {
        layout::hit_test(&self.laid, x as f32, y as f32).map(|(id, _)| id as usize)
    }

    fn handle(&mut self, request: Request) {
        match request {
            Request::Window { title, .. } | Request::Title(title) => self.title = title,
            Request::Show(tree) => { self.tree = Some(tree); self.laid_for = None; }
        }
    }
}

fn rect_of(b: &pmre_kit::Bounds) -> Rect {
    let (x, y) = ((b.min.x + 0.5) as i32, (b.min.y + 0.5) as i32);
    Rect::new(x, y, (b.max.x + 0.5) as i32 - x, (b.max.y + 0.5) as i32 - y)
}

fn kit_align(a: Align) -> pmre_kit::Align {
    match a { Align::Stretch => pmre_kit::Align::Stretch, Align::Start => pmre_kit::Align::Start,
              Align::Center => pmre_kit::Align::Center, Align::End => pmre_kit::Align::End }
}

/// Grows along the parent's main axis.
fn grown(mut style: KitStyle, parent: Dir) -> KitStyle {
    match parent { Dir::Row => style.width = Dim::Flex(1.0), Dir::Column => style.height = Dim::Flex(1.0) }
    style
}

fn measure_only() -> Rgba { Rgba::new(0.0, 0.0, 0.0, 0.0) }

fn text_span(text: &str, size: u16, weight: Weight) -> Span {
    let span = Span::new(text, size as f32, measure_only());
    if weight == Weight::Bold { span.bold() } else { span }
}

/// The vocabulary -> the kit's UXI tree. Every element is a kit box whose id indexes
/// `elements`; text inside it is measured by the kit with the desktop's fonts.
fn bridge(node: &Node, parent: Dir, elements: &mut Vec<Element>, fields: &mut BTreeMap<u32, TextField>,
          focus: Option<u32>, seen: &mut Vec<u32>) -> UxNode {
    let index = elements.len() as u32;
    let role = match node { Node::Button { .. } => Role::Button, Node::Toggle { .. } => Role::Toggle, Node::Field { .. } => Role::Input, _ => Role::None };
    let base = KitStyle::default().interactive(index, role);
    match node {
        Node::Column { layout: l, children } | Node::Row { layout: l, children } => {
            let dir = if matches!(node, Node::Column { .. }) { Dir::Column } else { Dir::Row };
            elements.push(Element::Container { panel: l.panel });
            let mut style = KitStyle { dir, gap: l.gap as f32, padding: Edges::all(l.pad as f32), align: kit_align(l.align), ..base };
            if l.grow { style = grown(style, parent); }
            let kids = children.iter().map(|child| bridge(child, dir, elements, fields, focus, seen)).collect();
            UxNode::boxed(style, kids)
        }
        Node::Text { style, text } => {
            elements.push(Element::Text { style: *style, text: text.clone() });
            // Proportional text wraps at the solved width (a one-span rich flow);
            // monospaced text stays on one line.
            let inner = match style.weight {
                Weight::Mono => UxNode::text(text.clone(), style.size as f32, measure_only()),
                weight => UxNode::Rich { spans: alloc::vec![text_span(text, style.size, weight)], align: kit_align(style.align) },
            };
            UxNode::boxed(base, alloc::vec![inner])
        }
        Node::Button { id, kind, grow, key, label } => {
            elements.push(Element::Button { id: *id, kind: *kind, key: *key, label: label.clone() });
            let mut style = KitStyle { padding: Edges::xy(16.0, 8.0), ..base };
            if *grow { style = grown(style, parent); }
            UxNode::boxed(style, alloc::vec![UxNode::text(label.clone(), UI.px(), measure_only())])
        }
        Node::Toggle { id, on, label } => {
            elements.push(Element::Toggle { id: *id, on: *on, label: label.clone() });
            let style = KitStyle { padding: Edges { l: 28.0, t: 4.0, r: 0.0, b: 4.0 }, ..base };
            UxNode::boxed(style, alloc::vec![UxNode::text(label.clone(), UI.px(), measure_only())])
        }
        Node::Field { id, grow, value } => {
            elements.push(Element::Field { id: *id });
            seen.push(*id);
            // The program owns the value; a field being edited keeps the user's text.
            let field = fields.entry(*id).or_insert_with(|| TextField::new(value));
            if focus != Some(*id) && field.text != *value { *field = TextField::new(value); }
            let mut style = KitStyle { height: Dim::Px(32.0), width: Dim::Px(180.0), ..base };
            if *grow { style = grown(style, parent); }
            UxNode::boxed(style, alloc::vec![])
        }
        Node::Icon { size, name } => {
            elements.push(Element::Icon { name: name.clone(), size: *size });
            UxNode::boxed(KitStyle { width: Dim::Px(*size as f32), height: Dim::Px(*size as f32), ..base }, alloc::vec![])
        }
        Node::Spacer => { elements.push(Element::Spacer); UxNode::boxed(grown(base, parent), alloc::vec![]) }
        Node::Divider => {
            elements.push(Element::Divider);
            let style = match parent { Dir::Column => KitStyle { height: Dim::Px(1.0), ..base }, Dir::Row => KitStyle { width: Dim::Px(1.0), ..base } };
            UxNode::boxed(style, alloc::vec![])
        }
    }
}

fn tone_color(tone: Tone) -> Color {
    match tone { Tone::Normal => TEXT, Tone::Muted => TEXT_MUTED, Tone::Accent => ACCENT, Tone::Danger => DANGER, Tone::Success => SUCCESS }
}

/// A desktop icon by name ("atom", "info", ...); unknown names get the program icon.
pub fn icon_named(name: &str) -> Icon {
    match name {
        "atom" => Icon::Atom, "info" => Icon::Info, "folder" => Icon::Folder, "document" => Icon::Document,
        "terminal" => Icon::Terminal, "monitor" => Icon::Monitor, "save" => Icon::Save, "power" => Icon::Power,
        "exit" => Icon::Exit, "calculator" => Icon::Calculator, _ => Icon::Program,
    }
}

impl App for Remote {
    fn title(&self) -> String { self.title.clone() }
    fn icon(&self) -> Icon { self.icon }
    fn size(&self) -> (i32, i32) { self.size }

    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool) {
        c.fill(area, WINDOW);
        if self.laid_for != Some(area) { self.solve(area); }
        if (area.w, area.h) != self.reported && self.tree.is_some() {
            self.reported = (area.w, area.h);
            self.send(Event::Resize(area.w.clamp(0, u16::MAX as i32) as u16, area.h.clamp(0, u16::MAX as i32) as u16));
        }
        if self.tree.is_none() {
            let message = if self.pid.is_none() { "Could not start the program" } else { "Starting..." };
            c.text_centered(&UI, area, message, TEXT_MUTED);
            return;
        }
        for b in &self.laid {
            let Some(index) = b.id else { continue };
            let r = rect_of(&b.rect);
            match &self.elements[index as usize] {
                Element::Container { panel: true } => { c.round_rect(r, 8, SURFACE, 255); c.round_outline(r, 8, DIVIDER, 255); }
                Element::Container { panel: false } | Element::Spacer => {}
                Element::Text { style, text } if style.weight == Weight::Mono => {
                    let font = Font { weight: FontWeight::Mono, size: style.size as u32 };
                    let width = font.width(text);
                    let x = match style.align { Align::Center => r.x + (r.w - width) / 2, Align::End => r.right() - width, _ => r.x };
                    c.text(&font, x, r.y + (r.h - font.line_height()) / 2, text, tone_color(style.tone));
                }
                Element::Text { style, text } => {
                    // The same line breaks the kit measured, placed in the solved rect.
                    let (lines, line_h) = layout::rich_lines(&[text_span(text, style.size, style.weight)], Some(r.w as f32));
                    let font = Font { weight: if style.weight == Weight::Bold { FontWeight::Bold } else { FontWeight::Regular }, size: style.size as u32 };
                    for (i, line) in lines.iter().enumerate() {
                        let width = line.width as i32;
                        let x = match style.align { Align::Center => r.x + (r.w - width) / 2, Align::End => r.right() - width, _ => r.x };
                        let y = r.y + (i as f32 * line_h) as i32;
                        for piece in &line.pieces { c.text(&font, x + piece.x as i32, y, &piece.text, tone_color(style.tone)); }
                    }
                }
                Element::Button { kind, label, .. } => {
                    let style = match kind { ButtonKind::Normal => ui::Style::Normal, ButtonKind::Primary => ui::Style::Primary, ButtonKind::Danger => ui::Style::Danger };
                    ui::button(c, r, label, style, true);
                }
                Element::Toggle { on, label, .. } => {
                    let bx = Rect::new(r.x + 2, r.y + (r.h - 18) / 2, 18, 18);
                    c.round_rect(bx, 4, if *on { ACCENT } else { WINDOW }, 255);
                    c.round_outline(bx, 4, if *on { ACCENT_DARK } else { BORDER }, 255);
                    if *on {
                        let (x, y) = (bx.x as f32, bx.y as f32);
                        c.polyline(&[(x + 4.0, y + 9.5), (x + 7.5, y + 13.0), (x + 14.0, y + 5.5)], 2.0, TEXT_ON_ACCENT, false);
                    }
                    c.text(&UI, r.x + 28, r.y + (r.h - UI.line_height()) / 2, label, TEXT);
                }
                Element::Field { id } => {
                    if let Some(field) = self.fields.get(id) { field.draw(c, r, focused && self.focus == Some(*id)); }
                }
                Element::Icon { name, size } => icons::draw(c, icon_named(name), r.x, r.y, *size as i32),
                Element::Divider => c.fill(r, DIVIDER),
            }
        }
    }

    fn key(&mut self, e: &InputEvent, _area: Rect) -> Action {
        if e.pressed == 0 { return Action::None; }
        if let Some(id) = self.focus {
            let Some(field) = self.fields.get_mut(&id) else { return Action::None };
            let event = match field.key(e) {
                ui::FieldEvent::Changed => Some(Event::Change(id, field.text.clone())),
                ui::FieldEvent::Submit => Some(Event::Submit(id, field.text.clone())),
                ui::FieldEvent::Cancel => { self.focus = None; None }
                ui::FieldEvent::None => None,
            };
            if let Some(event) = event { self.send(event); }
            return Action::Redraw;
        }
        // A shortcut the program declared for one of its buttons.
        let pressed = match e.key {
            k if k == b'\n' as u16 => Some(AccessKey::Enter),
            8 => Some(AccessKey::Backspace),
            27 => Some(AccessKey::Escape),
            k if (32..127).contains(&k) && e.modifiers & (MOD_CTRL | MOD_ALT) == 0 => Some(AccessKey::Char(k as u8 as char)),
            _ => None,
        };
        let target = pressed.and_then(|p| self.elements.iter().find_map(|el| match el {
            Element::Button { id, key: Some(k), .. } if *k == p => Some(*id),
            _ => None,
        }));
        if let Some(id) = target { self.send(Event::Click(id)); }
        Action::None
    }

    fn mouse_down(&mut self, x: i32, y: i32, _area: Rect, _double: bool) -> Action {
        let Some(index) = self.element_at(x, y) else { self.focus = None; return Action::Redraw };
        let mut toggled = None;
        match &self.elements[index] {
            Element::Button { .. } => { self.pressed = Some(index); }
            Element::Toggle { id, on, .. } => toggled = Some(Event::Toggle(*id, !*on)),
            Element::Field { id } => {
                let id = *id;
                self.focus = Some(id);
                let rect = self.laid.iter().find(|b| b.id == Some(index as u32)).map(|b| rect_of(&b.rect));
                if let (Some(field), Some(rect)) = (self.fields.get_mut(&id), rect) { field.click(x, rect); }
                return Action::Redraw;
            }
            _ => {}
        }
        self.focus = None;
        if let Some(event) = toggled { self.send(event); }
        Action::Redraw
    }

    fn mouse_up(&mut self, x: i32, y: i32, _area: Rect) -> Action {
        let Some(pressed) = self.pressed.take() else { return Action::None };
        if self.element_at(x, y) == Some(pressed) {
            if let Element::Button { id, .. } = self.elements[pressed] { self.send(Event::Click(id)); }
        }
        Action::None
    }

    fn tick(&mut self, _ticks: u64) -> Action {
        self.flush();
        if self.output == ERROR { return Action::None; }
        let mut changed = false;
        let mut buffer = [0u8; 4096];
        for _ in 0..16 {
            match rt::pipe_read(self.output, &mut buffer) {
                // Every writer is gone: the program ended, so its window closes.
                Some(0) => return Action::Close,
                Some(n) => {
                    for result in self.reader.feed(&buffer[..n]) {
                        match result {
                            Ok(request) => { self.handle(request); changed = true; }
                            Err(error) if !self.error_logged => {
                                // A malformed message is dropped (the last good tree stays).
                                self.error_logged = true;
                                rt::console_print(&alloc::format!("REMOTE_PROTOCOL_ERROR {} {:?}\n", self.program, error));
                            }
                            Err(_) => {}
                        }
                    }
                }
                None => break,
            }
        }
        if changed { Action::Redraw } else { Action::None }
    }

    fn closing(&mut self) {
        self.send(Event::Close);
        if let Some(pid) = self.pid.take() { rt::kill(pid); rt::wait(pid); }
        if self.input != ERROR { rt::pipe_close(self.input, PIPE_BOTH); }
        if self.output != ERROR { rt::pipe_close(self.output, PIPE_BOTH); }
    }
}
