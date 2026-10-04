//! The graphical console — mode 13h framebuffer edition (E41).
//!
//! The bootloader enters 320x200x8 video (vga_320x200 feature); the
//! framebuffer lives at physical 0xA0000 with the default VGA
//! palette (entries 0-15 are the classic 16 colors, so legacy
//! attribute bytes like 0x0f keep their meaning). Text renders as
//! 8x8 glyphs from the vendored public-domain font: 40 columns x 24
//! console rows, plus a taskbar strip on row 24.
//!
//! The PUBLIC API is the old text-mode API (VgaWriter::new /
//! write_byte / write_string / clear_screen, the persistent CURSOR,
//! move_cursor / cursor_position) — the chokepoint doctrine applied
//! to the console itself: every writer in the kernel became
//! graphical by changing only this sink. There is no hardware text
//! cursor in mode 13h, so the cursor is DRAWN (a two-pixel bar
//! under the glyph at the insertion cell).

use core::sync::atomic::{AtomicBool, Ordering};
use crate::atoms::project;
use crate::font8x8::FONT;
use crate::memory::Spinlock;

const FRAMEBUFFER: *mut u8 = 0xA0000 as *mut u8;
pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 200;
pub const COLS: usize = SCREEN_WIDTH / 8; // 40
pub const CONSOLE_ROWS: usize = 24; // row 24 is the taskbar
const GLYPH: usize = 8;

// ---------------------------------------------------------------------------
// Pixel primitives.
// ---------------------------------------------------------------------------

/// True while the desktop's linear framebuffer owns the screen: the console
/// keeps its grid and cursor up to date but draws nothing until `resume`.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Stops drawing (the grid still records everything written).
pub fn suspend() { SUSPENDED.store(true, Ordering::SeqCst); }

/// Takes the screen back: redraws the chrome, every console cell and the cursor.
pub fn resume() {
    if !SUSPENDED.swap(false, Ordering::SeqCst) { return; }
    desktop_init();
    render_all();
    let (col, row) = cursor_position();
    draw_cursor_bar(col, row, true);
}

fn put_pixel(x: usize, y: usize, color: u8) {
    if SUSPENDED.load(Ordering::Relaxed) { return; }
    if x < SCREEN_WIDTH && y < SCREEN_HEIGHT {
        unsafe {
            FRAMEBUFFER.add(y * SCREEN_WIDTH + x).write_volatile(color);
        }
    }
}

pub fn fill_rect(x0: usize, y0: usize, w: usize, h: usize, color: u8) {
    for y in y0..(y0 + h).min(SCREEN_HEIGHT) {
        for x in x0..(x0 + w).min(SCREEN_WIDTH) {
            put_pixel(x, y, color);
        }
    }
}

/// Blit one font glyph by ASCII byte at pixel coordinates.
pub fn draw_char(px: usize, py: usize, byte: u8, fg: u8, bg: u8) {
    let glyph = if (byte as usize) < 128 { FONT[byte as usize] } else { [0; 8] };
    for row in 0..GLYPH {
        let bits = glyph[row];
        for col in 0..GLYPH {
            let on = (bits >> col) & 1 == 1;
            put_pixel(px + col, py + row, if on { fg } else { bg });
        }
    }
}

/// Draw a string at pixel coordinates (taskbar / chrome use).
pub fn draw_str(px: usize, py: usize, text: &str, fg: u8, bg: u8) {
    for (i, byte) in text.bytes().enumerate() {
        draw_char(px + i * GLYPH, py, byte, fg, bg);
    }
}

// ---------------------------------------------------------------------------
// The console grid — the text buffer that scrolls, rendered to pixels.
// ---------------------------------------------------------------------------

/// (char, attribute) per cell; attribute nibbles are the legacy
/// fg/bg palette indices (0x0f = white on black).
#[derive(Clone, Copy)]
struct Cell(pub u8, pub u8);

static GRID: Spinlock<[Cell; COLS * CONSOLE_ROWS]> =
    Spinlock::new([Cell(0, 0x0f); COLS * CONSOLE_ROWS]);

/// C8 fix lineage: persistent cursor shared across VgaWriter
/// instances — the console position survives every syscall writer.
static CURSOR: Spinlock<(usize, usize)> = Spinlock::new((0, 0));

/// E41: the drawn cursor's underline bar (two rows, full cell width).
fn draw_cursor_bar(col: usize, row: usize, on: bool) {
    if col >= COLS || row >= CONSOLE_ROWS {
        return; // defensive: the cursor never draws off-grid
    }
    let cell = {
        let grid = GRID.lock();
        let cell = grid[row * COLS + col];
        GRID.unlock();
        cell
    };
    // Redraw the glyph (erases any previous bar), then the bar.
    render_cell(col, row, cell.0, cell.1);
    if on {
        let fg = cell.1 & 0x0F;
        for dx in 0..GLYPH {
            put_pixel(col * GLYPH + dx, row * GLYPH + 6, fg);
            put_pixel(col * GLYPH + dx, row * GLYPH + 7, fg);
        }
    }
}

/// Render one console cell to the framebuffer.
fn render_cell(col: usize, row: usize, byte: u8, attribute: u8) {
    if col >= COLS || row >= CONSOLE_ROWS {
        return;
    }
    let fg = attribute & 0x0F;
    let bg = (attribute >> 4) & 0x0F;
    draw_char(col * GLYPH, row * GLYPH, byte, fg, bg);
}

fn render_all() {
    let grid = GRID.lock();
    for row in 0..CONSOLE_ROWS {
        for col in 0..COLS {
            let cell = grid[row * COLS + col];
            render_cell(col, row, cell.0, cell.1);
        }
    }
    GRID.unlock();
}

pub struct VgaWriter {
    column_position: usize,
    row_position: usize,
}

impl VgaWriter {
    pub fn new() -> Self {
        let (col, row) = {
            let c = CURSOR.lock();
            let pos = *c;
            CURSOR.unlock();
            pos
        };
        Self { column_position: col.min(COLS - 1), row_position: row.min(CONSOLE_ROWS - 1) }
    }

    /// Persist the cursor, redraw its bar, and keep the legacy name
    /// (every mutation funnels through here).
    fn save_cursor(&self) {
        let old = {
            let mut c = CURSOR.lock();
            let old = *c;
            // Clamp on store: a writer parked at column COLS (awaiting
            // wrap) must not live in the cursor — the bar draws there.
            *c = (
                self.column_position.min(COLS - 1),
                self.row_position.min(CONSOLE_ROWS - 1),
            );
            CURSOR.unlock();
            old
        };
        draw_cursor_bar(old.0, old.1, false);
        let (col, row) = cursor_position();
        draw_cursor_bar(col, row, true);
    }

    pub fn write_byte(&mut self, byte: u8) {
        let (sport, sif) = crate::serial::SERIAL1.lock();
        sport.send(byte);
        crate::serial::SERIAL1.unlock(sif);

        if byte == b'\n' {
            self.new_line();
            self.save_cursor();
            return;
        }

        if byte == 0x08 {
            // Backspace: step back one cell and blank it.
            if self.column_position > 0 {
                self.column_position -= 1;
            } else if self.row_position > 0 {
                self.row_position -= 1;
                self.column_position = COLS - 1;
            }
            self.store(b' ', 0x0f);
            self.save_cursor();
            return;
        }

        if self.column_position >= COLS {
            self.new_line();
        }

        let (ascii, color) = project(byte, |b| (b, 0x0f));
        self.store(ascii, color);

        self.column_position += 1;
        self.save_cursor();
    }

    fn store(&mut self, byte: u8, attribute: u8) {
        let col = self.column_position;
        let row = self.row_position;
        {
            let mut grid = GRID.lock();
            grid[row * COLS + col] = Cell(byte, attribute);
            GRID.unlock();
        }
        render_cell(col, row, byte, attribute);
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }

    fn new_line(&mut self) {
        self.column_position = 0;
        self.row_position += 1;
        if self.row_position >= CONSOLE_ROWS {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        {
            let mut grid = GRID.lock();
            for row in 1..CONSOLE_ROWS {
                for col in 0..COLS {
                    grid[(row - 1) * COLS + col] = grid[row * COLS + col];
                }
            }
            for col in 0..COLS {
                grid[(CONSOLE_ROWS - 1) * COLS + col] = Cell(b' ', 0x0f);
            }
            GRID.unlock();
        }
        self.row_position = CONSOLE_ROWS - 1;
        render_all();
    }

    pub fn clear_screen(&mut self) {
        {
            let mut grid = GRID.lock();
            for cell in grid.iter_mut() {
                *cell = Cell(b' ', 0x0f);
            }
            GRID.unlock();
        }
        self.column_position = 0;
        self.row_position = 0;
        render_all();
        self.save_cursor();
    }
}

// ---------------------------------------------------------------------------
// Desktop chrome: the window frame and the taskbar.
// ---------------------------------------------------------------------------

/// Draw the desktop chrome once at boot: a deep-blue desktop, the
/// console surface, a separator line, and the taskbar on row 24.
pub fn desktop_init() {
    fill_rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, 0x01); // deep blue desktop
    fill_rect(0, 0, SCREEN_WIDTH, CONSOLE_ROWS * GLYPH, 0x00); // console surface
    fill_rect(0, CONSOLE_ROWS * GLYPH - 1, SCREEN_WIDTH, 1, 0x07); // frame line
    taskbar_update(0);
}

/// Redraw the taskbar: brand left, uptime right. Called at boot and
/// on the kernel's heartbeat (rate-limited by the caller).
pub fn taskbar_update(ticks: u64) {
    let bar_y = CONSOLE_ROWS * GLYPH;
    fill_rect(0, bar_y, SCREEN_WIDTH, GLYPH, 0x01);
    draw_str(2, bar_y, "ATOM OS", 0x0f, 0x01);
    let seconds = ticks / 100;
    let mut text = [b' '; 8];
    let mut value = seconds;
    let mut index = 8;
    while index > 0 {
        index -= 1;
        text[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for (i, &byte) in text.iter().enumerate() {
        draw_char(SCREEN_WIDTH - (8 - i) * GLYPH - 2, bar_y, byte, 0x0e, 0x01);
    }
}

/// E40 lineage: move the persistent cursor by `delta` cells
/// (negative left). Line-local: clamps at column 0.
pub fn move_cursor(delta: i64) {
    let old = {
        let mut c = CURSOR.lock();
        let old = *c;
        let moved = old.0 as i64 + delta;
        *c = (moved.clamp(0, COLS as i64 - 1) as usize, old.1);
        CURSOR.unlock();
        old
    };
    let (new_col, new_row) = cursor_position();
    draw_cursor_bar(old.0, old.1, false);
    draw_cursor_bar(new_col, new_row, true);
}

/// E40 lineage: the persistent cursor position (col, row).
pub fn cursor_position() -> (usize, usize) {
    let c = CURSOR.lock();
    let pos = *c;
    CURSOR.unlock();
    pos
}
