//! Colours and metrics shared by the window manager and applications.
use crate::gfx::{rgb, Color};

pub const ACCENT: Color = rgb(59, 130, 246);
pub const ACCENT_DARK: Color = rgb(37, 99, 235);
pub const ACCENT_SOFT: Color = rgb(219, 234, 254);
pub const DANGER: Color = rgb(220, 38, 38);
pub const SUCCESS: Color = rgb(22, 163, 74);

pub const WINDOW: Color = rgb(250, 251, 253);
pub const SURFACE: Color = rgb(243, 245, 249);
pub const TITLE_ACTIVE: Color = rgb(232, 236, 244);
pub const TITLE_INACTIVE: Color = rgb(243, 244, 247);
pub const BORDER: Color = rgb(203, 210, 222);
pub const DIVIDER: Color = rgb(226, 231, 239);
pub const TEXT: Color = rgb(30, 41, 59);
pub const TEXT_MUTED: Color = rgb(100, 116, 139);
pub const TEXT_ON_ACCENT: Color = rgb(255, 255, 255);
pub const ROW_ALT: Color = rgb(246, 248, 251);
pub const SELECTION: Color = rgb(191, 219, 254);

pub const TASKBAR: Color = rgb(17, 20, 31);
pub const TASKBAR_ITEM: Color = rgb(44, 50, 70);
pub const TASKBAR_TEXT: Color = rgb(226, 232, 240);
pub const MENU: Color = rgb(28, 32, 46);
pub const MENU_HOVER: Color = rgb(52, 60, 84);

pub const TERMINAL_BG: Color = rgb(15, 18, 28);
pub const TERMINAL_TEXT: Color = rgb(214, 222, 235);

pub const TITLE_HEIGHT: i32 = 34;
pub const TASKBAR_HEIGHT: i32 = 46;
pub const RADIUS: i32 = 10;
pub const ROW_HEIGHT: i32 = 26;
