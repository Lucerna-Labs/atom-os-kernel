//! ui-intent — the window vocabulary shared by Atom OS programs and the desktop.
//!
//! A program describes its window as a tree of *intent*: columns and rows of text,
//! buttons, toggles and fields, with spacing and theme tones. Nothing in the
//! vocabulary is a coordinate — the desktop derives every rectangle with its layout
//! solver ("author intent, let the solver compute the math"), paints with its own
//! widgets and theme, and reports interaction back as events naming element ids.
//!
//! The vocabulary is engine-neutral: no renderer types appear here, so the desktop's
//! engine (pmre-kit today) can change without touching programs.
//!
//! Wire format: line-oriented text, one message or node per line. It stays readable in
//! serial logs and carries ordinary prose structure, so a window description passes
//! the E24 egress cone like any other program output (binary would read as noise).
//!
//! ```text
//! window 300 420: Calculator        program -> desktop: open, size hint, title
//! tree                              a full tree follows, one node per line,
//! column gap=8 pad=12                 two spaces of indent per level
//!   text size=28 weight=bold align=end: 1234
//!   row gap=8
//!     button id=7: 7
//! end
//! click 7                           desktop -> program: an event
//! ```
//!
//! Decoding fails closed: depth, node count, line length and message size are capped
//! (`MAX_*`), and anything unknown is an error, never a guess.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Deepest tree accepted.
pub const MAX_DEPTH: usize = 24;
/// Most nodes in one tree.
pub const MAX_NODES: usize = 2048;
/// Longest line accepted.
pub const MAX_LINE: usize = 1024;
/// Largest encoded tree accepted (bytes).
pub const MAX_TREE_BYTES: usize = 128 * 1024;

/// Placement of children across a container's main axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align { #[default] Stretch, Start, Center, End }

/// A theme tone; the desktop maps tones to colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone { #[default] Normal, Muted, Accent, Danger, Success }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Weight { #[default] Regular, Bold, Mono }

/// A keyboard shortcut a program declares for a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKey { Char(char), Enter, Backspace, Escape }

/// A button's role, which the desktop shows with its own button styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonKind { #[default] Normal, Primary, Danger }

/// Spacing and placement of a container's children (lengths in logical pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Layout {
    pub gap: u16,
    pub pad: u16,
    pub align: Align,
    /// Takes a share of the free space along its parent's main axis.
    pub grow: bool,
    /// Drawn as a raised panel.
    pub panel: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle { pub size: u16, pub weight: Weight, pub tone: Tone, pub align: Align }

impl Default for TextStyle {
    fn default() -> Self { Self { size: 14, weight: Weight::Regular, tone: Tone::Normal, align: Align::Start } }
}

/// One element of a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Column { layout: Layout, children: Vec<Node> },
    Row { layout: Layout, children: Vec<Node> },
    Text { style: TextStyle, text: String },
    /// `key`: pressing it (with no field focused) clicks the button.
    Button { id: u32, kind: ButtonKind, grow: bool, key: Option<AccessKey>, label: String },
    Toggle { id: u32, on: bool, label: String },
    /// A one-line text field; edits come back as `Change`, Enter as `Submit`.
    Field { id: u32, grow: bool, value: String },
    /// A named icon from the desktop's set ("atom", "info", "folder", ...); an
    /// unknown name draws the desktop's generic icon.
    Icon { size: u16, name: String },
    /// Free space that grows along its parent's main axis.
    Spacer,
    Divider,
}

// ---- builders --------------------------------------------------------------------

pub fn column(children: Vec<Node>) -> Node { Node::Column { layout: Layout::default(), children } }
pub fn row(children: Vec<Node>) -> Node { Node::Row { layout: Layout::default(), children } }
pub fn text(text: &str) -> Node { Node::Text { style: TextStyle::default(), text: String::from(text) } }
pub fn button(id: u32, label: &str) -> Node { Node::Button { id, kind: ButtonKind::Normal, grow: false, key: None, label: String::from(label) } }
pub fn toggle(id: u32, label: &str, on: bool) -> Node { Node::Toggle { id, on, label: String::from(label) } }
pub fn field(id: u32, value: &str) -> Node { Node::Field { id, grow: false, value: String::from(value) } }
pub fn icon(name: &str, size: u16) -> Node { Node::Icon { size, name: String::from(name) } }

impl Node {
    fn layout_mut(&mut self) -> Option<&mut Layout> {
        match self { Node::Column { layout, .. } | Node::Row { layout, .. } => Some(layout), _ => None }
    }
    fn text_mut(&mut self) -> Option<&mut TextStyle> {
        match self { Node::Text { style, .. } => Some(style), _ => None }
    }
    /// Container: gap between children.
    pub fn gap(mut self, gap: u16) -> Self { if let Some(l) = self.layout_mut() { l.gap = gap; } self }
    /// Container: padding inside the edge.
    pub fn pad(mut self, pad: u16) -> Self { if let Some(l) = self.layout_mut() { l.pad = pad; } self }
    /// Container: raised panel.
    pub fn panel(mut self) -> Self { if let Some(l) = self.layout_mut() { l.panel = true; } self }
    /// Container, button or field: take a share of the free space.
    pub fn grow(mut self) -> Self {
        match &mut self {
            Node::Column { layout, .. } | Node::Row { layout, .. } => layout.grow = true,
            Node::Button { grow, .. } | Node::Field { grow, .. } => *grow = true,
            _ => {}
        }
        self
    }
    /// Container: cross-axis placement of children; text: horizontal alignment.
    pub fn align(mut self, align: Align) -> Self {
        if let Some(l) = self.layout_mut() { l.align = align; }
        if let Some(t) = self.text_mut() { t.align = align; }
        self
    }
    pub fn size(mut self, size: u16) -> Self { if let Some(t) = self.text_mut() { t.size = size; } self }
    pub fn bold(mut self) -> Self { if let Some(t) = self.text_mut() { t.weight = Weight::Bold; } self }
    pub fn mono(mut self) -> Self { if let Some(t) = self.text_mut() { t.weight = Weight::Mono; } self }
    pub fn tone(mut self, tone: Tone) -> Self { if let Some(t) = self.text_mut() { t.tone = tone; } self }
    pub fn primary(mut self) -> Self { if let Node::Button { kind, .. } = &mut self { *kind = ButtonKind::Primary; } self }
    pub fn danger(mut self) -> Self { if let Node::Button { kind, .. } = &mut self { *kind = ButtonKind::Danger; } self }
    /// Button: its keyboard shortcut.
    pub fn key(mut self, access: AccessKey) -> Self { if let Node::Button { key, .. } = &mut self { *key = Some(access); } self }
}

// ---- messages ----------------------------------------------------------------------

/// Program -> desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// First message: a size hint (logical pixels) and the title.
    Window { width: u16, height: u16, title: String },
    Title(String),
    Show(Node),
}

/// Desktop -> program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Click(u32),
    Toggle(u32, bool),
    Change(u32, String),
    Submit(u32, String),
    /// The client area's new size in logical pixels.
    Resize(u16, u16),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Syntax, TooDeep, TooMany, TooLong, Empty }

// ---- encoding ----------------------------------------------------------------------

/// Text after `: ` runs to the end of the line; newlines and backslashes are escaped.
fn push_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
}
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' { out.push(ch); continue; }
        match chars.next() { Some('n') => out.push('\n'), Some(c) => out.push(c), None => out.push('\\') }
    }
    out
}

fn align_word(a: Align) -> &'static str {
    match a { Align::Stretch => "stretch", Align::Start => "start", Align::Center => "center", Align::End => "end" }
}
fn tone_word(t: Tone) -> &'static str {
    match t { Tone::Normal => "normal", Tone::Muted => "muted", Tone::Accent => "accent", Tone::Danger => "danger", Tone::Success => "success" }
}

fn encode_node(out: &mut String, node: &Node, depth: usize) {
    for _ in 0..depth { out.push_str("  "); }
    let layout = |out: &mut String, l: &Layout| {
        let _ = write!(out, " gap={} pad={} align={}", l.gap, l.pad, align_word(l.align));
        if l.grow { out.push_str(" grow"); }
        if l.panel { out.push_str(" panel"); }
    };
    match node {
        Node::Column { layout: l, children } | Node::Row { layout: l, children } => {
            out.push_str(if matches!(node, Node::Column { .. }) { "column" } else { "row" });
            layout(out, l);
            out.push('\n');
            for child in children { encode_node(out, child, depth + 1); }
            return;
        }
        Node::Text { style, text } => {
            let weight = match style.weight { Weight::Regular => "regular", Weight::Bold => "bold", Weight::Mono => "mono" };
            let _ = write!(out, "text size={} weight={} tone={} align={}: ", style.size, weight, tone_word(style.tone), align_word(style.align));
            push_escaped(out, text);
        }
        Node::Button { id, kind, grow, key, label } => {
            let kind = match kind { ButtonKind::Normal => "normal", ButtonKind::Primary => "primary", ButtonKind::Danger => "danger" };
            let _ = write!(out, "button id={} kind={}{}", id, kind, if *grow { " grow" } else { "" });
            match key {
                Some(AccessKey::Char(c)) => { let _ = write!(out, " key={}", c); }
                Some(AccessKey::Enter) => out.push_str(" key=enter"),
                Some(AccessKey::Backspace) => out.push_str(" key=backspace"),
                Some(AccessKey::Escape) => out.push_str(" key=escape"),
                None => {}
            }
            out.push_str(": ");
            push_escaped(out, label);
        }
        Node::Toggle { id, on, label } => {
            let _ = write!(out, "toggle id={}{}: ", id, if *on { " on" } else { "" });
            push_escaped(out, label);
        }
        Node::Field { id, grow, value } => {
            let _ = write!(out, "field id={}{}: ", id, if *grow { " grow" } else { "" });
            push_escaped(out, value);
        }
        Node::Icon { size, name } => { let _ = write!(out, "icon size={}: ", size); push_escaped(out, name); }
        Node::Spacer => out.push_str("spacer"),
        Node::Divider => out.push_str("divider"),
    }
    out.push('\n');
}

impl Request {
    /// The message as wire lines (each ends in `\n`).
    pub fn encode(&self) -> String {
        let mut out = String::new();
        match self {
            Request::Window { width, height, title } => { let _ = write!(out, "window {} {}: ", width, height); push_escaped(&mut out, title); out.push('\n'); }
            Request::Title(title) => { out.push_str("title: "); push_escaped(&mut out, title); out.push('\n'); }
            Request::Show(node) => return encode_show(node),
        }
        out
    }
}

/// A `Show` request for `node`, without moving the tree into a `Request`.
pub fn encode_show(node: &Node) -> String {
    let mut out = String::from("tree\n");
    encode_node(&mut out, node, 0);
    out.push_str("end\n");
    out
}

impl Event {
    pub fn encode(&self) -> String {
        let mut out = String::new();
        match self {
            Event::Click(id) => { let _ = write!(out, "click {}", id); }
            Event::Toggle(id, on) => { let _ = write!(out, "toggle {} {}", id, if *on { "on" } else { "off" }); }
            Event::Change(id, text) => { let _ = write!(out, "change {}: ", id); push_escaped(&mut out, text); }
            Event::Submit(id, text) => { let _ = write!(out, "submit {}: ", id); push_escaped(&mut out, text); }
            Event::Resize(w, h) => { let _ = write!(out, "size {} {}", w, h); }
            Event::Close => out.push_str("close"),
        }
        out.push('\n');
        out
    }
    /// One event line (without its `\n`).
    pub fn decode(line: &str) -> Result<Event, Error> {
        if line.len() > MAX_LINE { return Err(Error::TooLong); }
        let (head, body) = split_text(line);
        let mut words = head.split_ascii_whitespace();
        let verb = words.next().ok_or(Error::Empty)?;
        let mut number = || -> Result<u32, Error> { words.next().ok_or(Error::Syntax)?.parse().map_err(|_| Error::Syntax) };
        let event = match verb {
            "click" => Event::Click(number()?),
            "toggle" => {
                let id = number()?;
                match words.next() { Some("on") => Event::Toggle(id, true), Some("off") => Event::Toggle(id, false), _ => return Err(Error::Syntax) }
            }
            "change" => Event::Change(number()?, unescape(body.ok_or(Error::Syntax)?)),
            "submit" => Event::Submit(number()?, unescape(body.ok_or(Error::Syntax)?)),
            "size" => {
                let w = number()?; let h = number()?;
                Event::Resize(w.min(u16::MAX as u32) as u16, h.min(u16::MAX as u32) as u16)
            }
            "close" => Event::Close,
            _ => return Err(Error::Syntax),
        };
        if words.next().is_some() { return Err(Error::Syntax); }
        Ok(event)
    }
}

// ---- decoding ----------------------------------------------------------------------

/// Splits `head: text` at the first `": "` (or a trailing `":"`).
fn split_text(line: &str) -> (&str, Option<&str>) {
    match line.find(": ") {
        Some(i) => (&line[..i], Some(&line[i + 2..])),
        None => match line.strip_suffix(':') { Some(head) => (head, Some("")), None => (line, None) },
    }
}

fn parse_align(v: &str) -> Result<Align, Error> {
    Ok(match v { "stretch" => Align::Stretch, "start" => Align::Start, "center" => Align::Center, "end" => Align::End, _ => return Err(Error::Syntax) })
}
fn parse_tone(v: &str) -> Result<Tone, Error> {
    Ok(match v { "normal" => Tone::Normal, "muted" => Tone::Muted, "accent" => Tone::Accent, "danger" => Tone::Danger, "success" => Tone::Success, _ => return Err(Error::Syntax) })
}
fn parse_u16(v: &str) -> Result<u16, Error> { v.parse().map_err(|_| Error::Syntax) }
fn parse_u32(v: &str) -> Result<u32, Error> { v.parse().map_err(|_| Error::Syntax) }

/// One node line (indent already removed) without its children.
fn parse_node(line: &str) -> Result<Node, Error> {
    let (head, body) = split_text(line);
    let mut words = head.split_ascii_whitespace();
    let kind = words.next().ok_or(Error::Empty)?;
    let mut layout = Layout::default();
    let mut style = TextStyle::default();
    let (mut id, mut on, mut grow, mut button_kind, mut access) = (None, false, false, ButtonKind::Normal, None);
    for word in words {
        let (key, value) = match word.split_once('=') { Some((k, v)) => (k, Some(v)), None => (word, None) };
        match (key, value) {
            ("gap", Some(v)) => layout.gap = parse_u16(v)?,
            ("pad", Some(v)) => layout.pad = parse_u16(v)?,
            ("align", Some(v)) => { let a = parse_align(v)?; layout.align = a; style.align = a; }
            ("grow", None) => { layout.grow = true; grow = true; }
            ("panel", None) => layout.panel = true,
            ("size", Some(v)) => style.size = parse_u16(v)?.clamp(6, 160),
            ("weight", Some("regular")) => style.weight = Weight::Regular,
            ("weight", Some("bold")) => style.weight = Weight::Bold,
            ("weight", Some("mono")) => style.weight = Weight::Mono,
            ("tone", Some(v)) => style.tone = parse_tone(v)?,
            ("id", Some(v)) => id = Some(parse_u32(v)?),
            ("on", None) => on = true,
            ("kind", Some("normal")) => button_kind = ButtonKind::Normal,
            ("kind", Some("primary")) => button_kind = ButtonKind::Primary,
            ("kind", Some("danger")) => button_kind = ButtonKind::Danger,
            ("key", Some("enter")) => access = Some(AccessKey::Enter),
            ("key", Some("backspace")) => access = Some(AccessKey::Backspace),
            ("key", Some("escape")) => access = Some(AccessKey::Escape),
            ("key", Some(v)) if v.chars().count() == 1 => access = v.chars().next().map(AccessKey::Char),
            _ => return Err(Error::Syntax),
        }
    }
    let label = || body.map(unescape).ok_or(Error::Syntax);
    Ok(match kind {
        "column" if body.is_none() => Node::Column { layout, children: Vec::new() },
        "row" if body.is_none() => Node::Row { layout, children: Vec::new() },
        "text" => Node::Text { style, text: label()? },
        "button" => Node::Button { id: id.ok_or(Error::Syntax)?, kind: button_kind, grow, key: access, label: label()? },
        "toggle" => Node::Toggle { id: id.ok_or(Error::Syntax)?, on, label: label()? },
        "field" => Node::Field { id: id.ok_or(Error::Syntax)?, grow, value: label()? },
        "icon" => Node::Icon { size: style.size, name: label()? },
        "spacer" if body.is_none() => Node::Spacer,
        "divider" if body.is_none() => Node::Divider,
        _ => return Err(Error::Syntax),
    })
}

/// Decodes the node lines between `tree` and `end`.
pub fn decode_tree(lines: &[&str]) -> Result<Node, Error> {
    if lines.is_empty() { return Err(Error::Empty); }
    if lines.len() > MAX_NODES { return Err(Error::TooMany); }
    // (depth, node) stack of open containers; a node attaches to the container one
    // level shallower than itself.
    let mut stack: Vec<(usize, Node)> = Vec::new();
    let mut root: Option<Node> = None;
    for line in lines {
        if line.len() > MAX_LINE { return Err(Error::TooLong); }
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if indent % 2 != 0 || trimmed.is_empty() { return Err(Error::Syntax); }
        let depth = indent / 2;
        if depth >= MAX_DEPTH { return Err(Error::TooDeep); }
        let node = parse_node(trimmed)?;
        if root.is_some() { return Err(Error::Syntax); } // one root only
        if depth == 0 && !stack.is_empty() { return Err(Error::Syntax); }
        // Close containers that are not this node's parent.
        while stack.len() > depth { close(&mut stack, &mut root)?; }
        if stack.len() != depth { return Err(Error::Syntax); } // indentation jumped
        let container = matches!(node, Node::Column { .. } | Node::Row { .. });
        if container { stack.push((depth, node)); }
        else if depth == 0 { root = Some(node); }
        else { attach(&mut stack, node)?; }
    }
    while !stack.is_empty() { close(&mut stack, &mut root)?; }
    root.ok_or(Error::Empty)
}

fn attach(stack: &mut [(usize, Node)], node: Node) -> Result<(), Error> {
    match stack.last_mut() {
        Some((_, Node::Column { children, .. })) | Some((_, Node::Row { children, .. })) => { children.push(node); Ok(()) }
        _ => Err(Error::Syntax),
    }
}
fn close(stack: &mut Vec<(usize, Node)>, root: &mut Option<Node>) -> Result<(), Error> {
    let (_, node) = stack.pop().ok_or(Error::Syntax)?;
    if stack.is_empty() { if root.is_some() { return Err(Error::Syntax); } *root = Some(node); Ok(()) }
    else { attach(stack, node) }
}

/// Reassembles requests from a byte stream (a program's output pipe).
pub struct RequestReader { pending: Vec<u8>, tree: Option<Vec<String>>, tree_bytes: usize }

impl Default for RequestReader { fn default() -> Self { Self::new() } }

impl RequestReader {
    pub const fn new() -> Self { Self { pending: Vec::new(), tree: None, tree_bytes: 0 } }
    /// Feeds bytes; returns every complete request (or error) they finish. An error
    /// discards the message it occurred in; reading resumes at the next line.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<Request, Error>> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            if let Some(result) = self.line(line) { out.push(result); }
        }
        if self.pending.len() > MAX_LINE {
            self.pending.clear();
            self.tree = None;
            out.push(Err(Error::TooLong));
        }
        out
    }
    fn line(&mut self, line: String) -> Option<Result<Request, Error>> {
        if let Some(lines) = &mut self.tree {
            if line == "end" {
                let lines = self.tree.take().unwrap();
                let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
                return Some(decode_tree(&refs).map(Request::Show));
            }
            self.tree_bytes += line.len() + 1;
            if self.tree_bytes > MAX_TREE_BYTES || lines.len() >= MAX_NODES {
                self.tree = None;
                return Some(Err(Error::TooMany));
            }
            lines.push(line);
            return None;
        }
        if line == "tree" { self.tree = Some(Vec::new()); self.tree_bytes = 0; return None; }
        if line.is_empty() { return None; }
        if line.len() > MAX_LINE { return Some(Err(Error::TooLong)); }
        let (head, body) = split_text(&line);
        let mut words = head.split_ascii_whitespace();
        Some(match (words.next(), body) {
            (Some("window"), Some(title)) => {
                let w = words.next().map(parse_u16);
                let h = words.next().map(parse_u16);
                match (w, h, words.next()) {
                    (Some(Ok(width)), Some(Ok(height)), None) => Ok(Request::Window { width, height, title: unescape(title) }),
                    _ => Err(Error::Syntax),
                }
            }
            (Some("title"), Some(title)) if words.next().is_none() => Ok(Request::Title(unescape(title))),
            _ => Err(Error::Syntax),
        })
    }
}

/// Splits a byte stream into event lines (a program's input pipe).
pub struct EventReader { pending: Vec<u8> }

impl Default for EventReader { fn default() -> Self { Self::new() } }

impl EventReader {
    pub const fn new() -> Self { Self { pending: Vec::new() } }
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<Event, Error>> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            out.push(Event::decode(&String::from_utf8_lossy(&line[..line.len() - 1])));
        }
        if self.pending.len() > MAX_LINE { self.pending.clear(); out.push(Err(Error::TooLong)); }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn calculator() -> Node {
        column(vec![
            text("12 + 30").size(28).bold().align(Align::End).tone(Tone::Accent),
            row(vec![button(7, "7").grow().key(AccessKey::Char('7')), button(8, "8").grow(),
                     button(100, "÷").primary().key(AccessKey::Char('/')), button(101, "=").key(AccessKey::Enter),
                     button(102, "eq").key(AccessKey::Char('='))]).gap(8),
            row(vec![toggle(3, "Degrees", true), Node::Spacer, field(4, "a: b\\c\nd").grow()]).gap(4),
            Node::Divider,
            icon("atom", 48),
        ]).gap(8).pad(12).panel()
    }

    fn roundtrip(request: Request) -> Request {
        let mut reader = RequestReader::new();
        let mut results = reader.feed(request.encode().as_bytes());
        assert_eq!(results.len(), 1);
        results.pop().unwrap().unwrap()
    }

    #[test]
    fn trees_round_trip_exactly() {
        let request = Request::Show(calculator());
        assert_eq!(roundtrip(request.clone()), request);
        let window = Request::Window { width: 300, height: 420, title: "Calc: one\nline".into() };
        assert_eq!(roundtrip(window.clone()), window);
    }

    #[test]
    fn encoding_is_deterministic_and_carries_no_coordinates() {
        let a = Request::Show(calculator()).encode();
        assert_eq!(a, Request::Show(calculator()).encode());
        for word in [" x=", " y=", " width=", " height=", " left=", " top="] { assert!(!a.contains(word), "{word}"); }
    }

    #[test]
    fn requests_reassemble_across_arbitrary_chunks() {
        let bytes = alloc::format!("{}{}", Request::Title("T".into()).encode(), Request::Show(calculator()).encode());
        for chunk in [1, 3, 7, 64, 4096] {
            let mut reader = RequestReader::new();
            let got: Vec<_> = bytes.as_bytes().chunks(chunk).flat_map(|c| reader.feed(c)).collect();
            assert_eq!(got, vec![Ok(Request::Title("T".into())), Ok(Request::Show(calculator()))]);
        }
    }

    #[test]
    fn events_round_trip() {
        for event in [Event::Click(7), Event::Toggle(3, false), Event::Change(4, "a: b\n".into()),
                      Event::Submit(4, "".into()), Event::Resize(640, 480), Event::Close] {
            let mut reader = EventReader::new();
            assert_eq!(reader.feed(event.encode().as_bytes()), vec![Ok(event.clone())]);
        }
    }

    #[test]
    fn malformed_input_fails_closed() {
        let bad = [
            &["column", "    text: skipped a level"][..],
            &["text: a", "text: two roots"][..],
            &["column", "  button: no id"][..],
            &["column x=10"][..],
            &["column", "  wobble id=1: unknown"][..],
            &[" column"][..],
            &["text size=abc: x"][..],
        ];
        for lines in bad { assert!(decode_tree(lines).is_err(), "{lines:?}"); }
        let deep: Vec<String> = (0..=MAX_DEPTH).map(|d| alloc::format!("{}column", "  ".repeat(d))).collect();
        let refs: Vec<&str> = deep.iter().map(String::as_str).collect();
        assert_eq!(decode_tree(&refs), Err(Error::TooDeep));
        for line in ["click", "click x", "toggle 1 maybe", "size 1", "close now", "launch 1"] {
            assert!(Event::decode(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_bad_message_does_not_poison_the_stream() {
        let mut reader = RequestReader::new();
        let mut stream = String::from("tree\ncolumn\n  nonsense\nend\n");
        stream.push_str(&Request::Title("after".into()).encode());
        let got = reader.feed(stream.as_bytes());
        assert!(got[0].is_err());
        assert_eq!(got[1], Ok(Request::Title("after".into())));
        let mut long = vec![b'a'; MAX_LINE + 10];
        long.push(b'\n');
        assert!(reader.feed(&long).iter().any(|r| r.is_err()));
    }
}
