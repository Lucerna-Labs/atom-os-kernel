//! Desktop applications. Each window hosts one `App`.
use alloc::boxed::Box;
use alloc::string::String;
use user_rt::abi::InputEvent;
use crate::gfx::{Canvas, Rect};
use crate::icons::Icon;

pub mod dialog;
pub mod picker;
pub mod files;
pub mod editor;
pub mod terminal;
pub mod monitor;
pub mod remote;

pub enum DialogResult { Cancel, Ok, Text(String) }

pub enum Action {
    None,
    /// Repaint (the app's state changed).
    Redraw,
    Close,
    Open(Box<dyn App>),
    /// Open a modal dialog over this window; its result returns via `dialog_result`.
    Dialog(Box<dyn App>, u32),
    /// Sent by a dialog to finish itself.
    Finish(DialogResult),
    Toast(String),
    Exit,
    Restart,
}

pub trait App {
    fn title(&self) -> String;
    fn icon(&self) -> Icon;
    fn size(&self) -> (i32, i32) { (640, 440) }
    fn resizable(&self) -> bool { true }
    /// Draws the client area (already clipped to `area`).
    fn draw(&mut self, c: &mut Canvas, area: Rect, focused: bool);
    fn key(&mut self, _event: &InputEvent, _area: Rect) -> Action { Action::None }
    /// Coordinates are absolute; `area` is the client rectangle.
    fn mouse_down(&mut self, _x: i32, _y: i32, _area: Rect, _double: bool) -> Action { Action::None }
    fn mouse_drag(&mut self, _x: i32, _y: i32, _area: Rect) -> Action { Action::None }
    fn mouse_up(&mut self, _x: i32, _y: i32, _area: Rect) -> Action { Action::None }
    fn wheel(&mut self, _delta: i32, _area: Rect) -> Action { Action::None }
    /// Periodic work; return Redraw when something changed.
    fn tick(&mut self, _ticks: u64) -> Action { Action::None }
    fn dialog_result(&mut self, _tag: u32, _result: DialogResult) -> Action { Action::None }
    /// Called before the window closes (release processes, pipes).
    fn closing(&mut self) {}
}
