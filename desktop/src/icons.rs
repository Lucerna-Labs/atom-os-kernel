//! Procedurally drawn icons, so no image assets are needed.
use crate::gfx::{rgb, Canvas, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon { Folder, Document, Terminal, Monitor, Info, Program, Atom, Save, Power, Exit }

/// Draws `icon` in a `size` x `size` square at (x, y).
pub fn draw(c: &mut Canvas, icon: Icon, x: i32, y: i32, size: i32) {
    let s = size as f32 / 32.0;
    let px = |v: f32| (v * s) as i32;
    let r = |ax: f32, ay: f32, w: f32, h: f32| Rect::new(x + px(ax), y + px(ay), px(w).max(1), px(h).max(1));
    match icon {
        Icon::Folder => {
            c.round_rect(r(2.0, 6.0, 12.0, 6.0), px(2.0), rgb(217, 145, 20), 255);
            c.round_rect(r(2.0, 9.0, 28.0, 19.0), px(3.0), rgb(234, 170, 40), 255);
            c.round_rect(r(2.0, 12.0, 28.0, 16.0), px(3.0), rgb(251, 196, 70), 255);
        }
        Icon::Document => {
            c.round_rect(r(6.0, 2.0, 20.0, 28.0), px(3.0), rgb(255, 255, 255), 255);
            c.round_outline(r(6.0, 2.0, 20.0, 28.0), px(3.0), rgb(148, 163, 184), 255);
            for (i, w) in [12.0, 14.0, 10.0, 13.0].iter().enumerate() {
                c.fill(r(10.0, 9.0 + i as f32 * 5.0, *w, 1.6), rgb(59, 130, 246));
            }
        }
        Icon::Terminal => {
            c.round_rect(r(2.0, 4.0, 28.0, 24.0), px(4.0), rgb(30, 35, 50), 255);
            c.fill(r(2.0, 8.0, 28.0, 1.0), rgb(60, 68, 90));
            let (ox, oy) = (x as f32, y as f32);
            c.line(ox + 7.0 * s, oy + 13.0 * s, ox + 12.0 * s, oy + 17.0 * s, 2.0 * s, rgb(74, 222, 128));
            c.line(ox + 12.0 * s, oy + 17.0 * s, ox + 7.0 * s, oy + 21.0 * s, 2.0 * s, rgb(74, 222, 128));
            c.fill(r(14.0, 21.0, 9.0, 2.0), rgb(226, 232, 240));
        }
        Icon::Monitor => {
            c.round_rect(r(2.0, 4.0, 28.0, 24.0), px(4.0), rgb(14, 116, 144), 255);
            let pts = [(6.0, 22.0), (11.0, 15.0), (16.0, 18.0), (21.0, 9.0), (26.0, 12.0)];
            for pair in pts.windows(2) {
                let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                c.line(x as f32 + x0 * s, y as f32 + y0 * s, x as f32 + x1 * s, y as f32 + y1 * s, 2.2 * s, rgb(165, 243, 252));
            }
        }
        Icon::Info => {
            c.circle(x + px(16.0), y + px(16.0), px(14.0), rgb(59, 130, 246), 255);
            c.circle(x + px(16.0), y + px(9.5), px(2.2).max(1), rgb(255, 255, 255), 255);
            c.round_rect(r(14.0, 14.0, 4.0, 11.0), px(1.5), rgb(255, 255, 255), 255);
        }
        Icon::Program => {
            c.round_rect(r(3.0, 3.0, 26.0, 26.0), px(6.0), rgb(124, 58, 237), 255);
            c.round_rect(r(9.0, 9.0, 14.0, 14.0), px(3.0), rgb(196, 181, 253), 255);
            c.round_rect(r(13.0, 13.0, 6.0, 6.0), px(2.0), rgb(124, 58, 237), 255);
        }
        Icon::Atom => {
            let (cx, cy) = (x as f32 + 16.0 * s, y as f32 + 16.0 * s);
            // Three orbits as rotated ellipses, sampled into short line segments.
            for angle in [0.0f32, 1.0472, 2.0944] {
                let (sin, cos) = (sin(angle), cos(angle));
                let mut last = None;
                for step in 0..=36 {
                    let t = step as f32 * core::f32::consts::TAU / 36.0;
                    let (ex, ey) = (13.0 * s * cos_t(t), 5.0 * s * sin_t(t));
                    let point = (cx + ex * cos - ey * sin, cy + ex * sin + ey * cos);
                    if let Some((lx, ly)) = last { c.line(lx, ly, point.0, point.1, 1.6 * s, rgb(125, 211, 252)); }
                    last = Some(point);
                }
            }
            c.circle(cx as i32, cy as i32, px(3.5).max(2), rgb(251, 191, 36), 255);
        }
        Icon::Save => {
            c.round_rect(r(5.0, 5.0, 22.0, 22.0), px(3.0), rgb(59, 130, 246), 255);
            c.fill(r(10.0, 5.0, 12.0, 8.0), rgb(219, 234, 254));
            c.fill(r(9.0, 17.0, 14.0, 10.0), rgb(255, 255, 255));
        }
        Icon::Power => {
            let (cx, cy) = (x as f32 + 16.0 * s, y as f32 + 17.0 * s);
            let mut last = None;
            for step in 0..=30 {
                let t = 0.9 + step as f32 * (core::f32::consts::TAU - 1.8) / 30.0 - core::f32::consts::FRAC_PI_2;
                let p = (cx + 10.0 * s * cos_t(t), cy + 10.0 * s * sin_t(t));
                if let Some((lx, ly)) = last { c.line(lx, ly, p.0, p.1, 2.4 * s, rgb(248, 113, 113)); }
                last = Some(p);
            }
            c.line(cx, cy - 13.0 * s, cx, cy - 3.0 * s, 2.4 * s, rgb(248, 113, 113));
        }
        Icon::Exit => {
            c.round_outline(r(4.0, 5.0, 16.0, 22.0), px(2.0), rgb(203, 213, 225), 255);
            let (ox, oy) = (x as f32, y as f32);
            c.line(ox + 13.0 * s, oy + 16.0 * s, ox + 28.0 * s, oy + 16.0 * s, 2.2 * s, rgb(203, 213, 225));
            c.line(ox + 23.0 * s, oy + 11.0 * s, ox + 28.0 * s, oy + 16.0 * s, 2.2 * s, rgb(203, 213, 225));
            c.line(ox + 23.0 * s, oy + 21.0 * s, ox + 28.0 * s, oy + 16.0 * s, 2.2 * s, rgb(203, 213, 225));
        }
    }
}

// Small Taylor-series trig, adequate for drawing.
fn sin_t(t: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let mut x = t % (2.0 * pi);
    if x > pi { x -= 2.0 * pi; } else if x < -pi { x += 2.0 * pi; }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}
fn cos_t(t: f32) -> f32 { sin_t(t + core::f32::consts::FRAC_PI_2) }
pub fn sin(t: f32) -> f32 { sin_t(t) }
pub fn cos(t: f32) -> f32 { cos_t(t) }
