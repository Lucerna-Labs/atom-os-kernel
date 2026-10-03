//! Rasterization primitives — the cheap, exact-AA coverage generator.
//!
//! `signed_distance` is the `compare` atom (a distance). `coverage` is the graphics
//! `smoothstep` primitive (analytic anti-aliasing). `scan_convert` wires
//! `scan` (pixel grid) · `project` (inverse transform) · `compare` (SDF) · `scale` (AA band)
//! · `combine` (alpha-over) into one shape → pixels operation. Mechanism only: it never
//! decides draw order, clipping, or which generator to use — that is the orchestrator's job.
#[allow(unused_imports)]
use crate::prelude::*;

use crate::framebuffer::Surface;
use crate::geom::Vec2;
use crate::paint::{Bounds, DrawCmd, Paint, Shape};

/// Signed distance to the shape boundary in its local space: negative inside, positive outside.
pub fn signed_distance(shape: &Shape, p: Vec2) -> f32 {
    match *shape {
        Shape::Rect { half } => sd_box(p, half),
        Shape::RoundedRect { half, radius } => sd_box(p, half - Vec2::new(radius, radius)) - radius,
        Shape::Circle { radius } => p.length() - radius,
        Shape::Line { a, b, width } => sd_segment(p, a, b) - width * 0.5,
        Shape::RoundedRectOutline {
            half,
            radius,
            width,
        } => ring(sd_box(p, half - Vec2::new(radius, radius)) - radius, width),
        Shape::CircleOutline { radius, width } => ring(p.length() - radius, width),
    }
}

/// A band `width` thick just inside the zero contour of the distance `d`.
fn ring(d: f32, width: f32) -> f32 {
    (d + width * 0.5).abs() - width * 0.5
}

/// For a horizontal row at local height `py`: the half-width `h` (centered on the local
/// origin) within which the filled shape's distance is at most `-margin`. Analytic per
/// shape, and conservative by a small epsilon, so every pixel it admits is one the
/// per-pixel path would also find at that depth. `None` when no such run exists.
fn inner_half(shape: &Shape, py: f32, margin: f32) -> Option<f32> {
    const EPS: f32 = 1e-3;
    let py = py.abs();
    let h = match *shape {
        Shape::Rect { half } => {
            if py > half.y - margin {
                return None;
            }
            half.x - margin
        }
        Shape::RoundedRect { half, radius } | Shape::RoundedRectOutline { half, radius, .. } => {
            let (a, b) = (half.x - radius, half.y - radius);
            if a < 0.0 || b < 0.0 || radius < 0.0 {
                return None;
            }
            let dy = py - b;
            let reach = radius - margin;
            if dy > reach {
                return None;
            }
            if dy <= 0.0 {
                half.x - margin
            } else {
                a + (reach * reach - dy * dy).max(0.0).sqrt()
            }
        }
        Shape::Circle { radius } | Shape::CircleOutline { radius, .. } => {
            let reach = radius - margin;
            if py > reach {
                return None;
            }
            (reach * reach - py * py).max(0.0).sqrt()
        }
        Shape::Line { .. } => return None,
    } - EPS;
    (h >= 0.0).then_some(h)
}

fn sd_box(p: Vec2, half: Vec2) -> f32 {
    let d = p.abs() - half;
    d.max_scalar(0.0).length() + d.x.max(d.y).min(0.0)
}

fn sd_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
    (pa - ba.scale(h)).length()
}

/// Analytic coverage from a signed distance: 1 inside, 0 outside, Hermite band of half-width `aa`.
pub fn coverage(dist: f32, aa: f32) -> f32 {
    1.0 - smoothstep(-aa, aa, dist)
}

/// Hermite smoothstep: 0 below `edge0`, 1 above `edge1`, C¹-continuous in between.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scan-convert one command into `surf` using the SDF coverage generator. Generic over the
/// pixel sink so it can target a whole framebuffer or one row-band of it (see `Surface`).
/// Pure translations (every box the layout solver emits) take a fast path that replaces
/// the per-pixel inverse-matrix multiply with a subtraction.
pub fn scan_convert<S: Surface>(cmd: &DrawCmd, surf: &mut S, clip: Option<Bounds>) {
    // One device pixel measured in local units — the width of the anti-aliasing band.
    // A `soft` command widens the band for smooth falloff (shadows, glows).
    let aa = (1.0 / cmd.transform.scale_factor().max(1e-6))
        .max(1e-4)
        .max(cmd.soft);
    let bounds = device_bounds(cmd, surf.width(), surf.height(), surf.row_range(), clip);
    let t = cmd.transform;
    if t.a == 1.0 && t.d == 1.0 && t.b == 0.0 && t.c == 0.0 {
        convert_rows_spans(cmd, surf, bounds, aa, t.e, t.f);
    } else {
        let inv = t.inverse();
        convert_rows(cmd, surf, bounds, aa, move |x, y| {
            inv.apply(Vec2::new(x, y))
        });
    }
}

/// The pure-translation path. Per row, the analytic interior run is handled in bulk: for
/// filled shapes it is fully covered (one `fill_span` for solid paint), for outlines it is
/// the hollow middle and is skipped. Only the pixels near the edges evaluate the SDF.
/// Output is identical to the per-pixel path.
fn convert_rows_spans<S: Surface>(
    cmd: &DrawCmd,
    surf: &mut S,
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    aa: f32,
    tx: f32,
    ty: f32,
) {
    let outline = match cmd.shape {
        Shape::RoundedRectOutline { width, .. } | Shape::CircleOutline { width, .. } => Some(width),
        _ => None,
    };
    let edge = |surf: &mut S, x: u32, y: u32, py: f32| {
        let local = Vec2::new(x as f32 + 0.5 - tx, py);
        let cov = coverage(signed_distance(&cmd.shape, local), aa);
        if cov > 0.0 {
            let col = cmd.paint.sample(local);
            surf.blend_over(x, y, col.with_alpha(col.a * cov));
        }
    };
    for y in y0..y1 {
        let py = y as f32 + 0.5 - ty;
        // Inside an outline the hollow lies deeper than the band; inside a fill, the full
        // coverage starts one AA half-band in.
        let margin = outline.map_or(aa, |w| w + aa);
        let run = inner_half(&cmd.shape, py, margin).map(|h| {
            // min/max rather than clamp: the run may lie wholly outside the clipped
            // columns (a > x1), which must give an empty run, not a panic.
            let a = ((tx - h - 0.5).ceil().max(x0 as f32) as u32).min(x1);
            let b = (((tx + h - 0.5).floor() + 1.0).max(0.0) as u32).clamp(a, x1);
            (a, b)
        });
        let Some((a, b)) = run.filter(|(a, b)| a < b) else {
            for x in x0..x1 {
                edge(surf, x, y, py);
            }
            continue;
        };
        for x in x0..a {
            edge(surf, x, y, py);
        }
        if outline.is_none() {
            match cmd.paint {
                Paint::Solid(c) => surf.fill_span(y, a, b, c),
                _ => {
                    for x in a..b {
                        surf.blend_over(x, y, cmd.paint.sample(Vec2::new(x as f32 + 0.5 - tx, py)));
                    }
                }
            }
        }
        for x in b..x1 {
            edge(surf, x, y, py);
        }
    }
}

fn convert_rows<S: Surface, M: Fn(f32, f32) -> Vec2>(
    cmd: &DrawCmd,
    surf: &mut S,
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    aa: f32,
    to_local: M,
) {
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let local = to_local(x as f32 + 0.5, py);
            let d = signed_distance(&cmd.shape, local);
            let cov = coverage(d, aa);
            if cov > 0.0 {
                // Sample the paint at the shape-local point (gradients move with the shape).
                let col = cmd.paint.sample(local);
                surf.blend_over(x, y, col.with_alpha(col.a * cov));
            }
        }
    }
}

/// Device-space pixel bounds the command can touch (its transformed, padded local box),
/// intersected with the optional clip rectangle and the surface's accepted row range `rows`.
fn device_bounds(
    cmd: &DrawCmd,
    w: u32,
    h: u32,
    rows: (u32, u32),
    clip: Option<Bounds>,
) -> (u32, u32, u32, u32) {
    let lb = cmd.shape.local_bounds().pad(2.0 + cmd.soft);
    let corners = [
        Vec2::new(lb.min.x, lb.min.y),
        Vec2::new(lb.max.x, lb.min.y),
        Vec2::new(lb.min.x, lb.max.y),
        Vec2::new(lb.max.x, lb.max.y),
    ];
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for c in corners {
        let d = cmd.transform.apply(c);
        min = Vec2::new(min.x.min(d.x), min.y.min(d.y));
        max = Vec2::new(max.x.max(d.x), max.y.max(d.y));
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (min.x, min.y, max.x, max.y);
    if let Some(c) = clip {
        minx = minx.max(c.min.x);
        miny = miny.max(c.min.y);
        maxx = maxx.min(c.max.x);
        maxy = maxy.min(c.max.y);
    }
    let (rlo, rhi) = rows;
    let x0 = minx.floor().max(0.0) as u32;
    let y0 = (miny.floor().max(0.0) as u32).max(rlo);
    let x1 = (maxx.ceil().max(0.0) as u32).min(w);
    let y1 = (maxy.ceil().max(0.0) as u32).min(h).min(rhi);
    (x0, y0, x1, y1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::Framebuffer;
    use crate::geom::Affine;
    use crate::paint::{Paint, Rgba};

    /// The original per-pixel path over the whole bounding box, as the reference.
    fn reference(cmd: &DrawCmd, fb: &mut Framebuffer) {
        reference_clipped(cmd, fb, None);
    }

    fn reference_clipped(cmd: &DrawCmd, fb: &mut Framebuffer, clip: Option<Bounds>) {
        let aa = (1.0 / cmd.transform.scale_factor().max(1e-6))
            .max(1e-4)
            .max(cmd.soft);
        let bounds = device_bounds(cmd, fb.width, fb.height, (0, fb.height), clip);
        let t = cmd.transform;
        convert_rows(cmd, fb, bounds, aa, |x, y| Vec2::new(x - t.e, y - t.f));
    }

    #[test]
    fn span_fast_path_matches_per_pixel_rendering() {
        let bg = Rgba::new(0.2, 0.3, 0.4, 1.0);
        let solid = Paint::Solid(Rgba::new(0.9, 0.5, 0.1, 1.0));
        let translucent = Paint::Solid(Rgba::new(0.1, 0.8, 0.3, 0.6));
        let gradient = Paint::Linear {
            from: Vec2::new(-20.0, 0.0),
            to: Vec2::new(20.0, 0.0),
            c0: Rgba::new(1.0, 0.0, 0.0, 1.0),
            c1: Rgba::new(0.0, 0.0, 1.0, 0.8),
        };
        let shapes = [
            Shape::Rect {
                half: Vec2::new(23.3, 11.7),
            },
            Shape::RoundedRect {
                half: Vec2::new(30.0, 18.0),
                radius: 10.0,
            },
            Shape::RoundedRect {
                half: Vec2::new(12.0, 12.0),
                radius: 12.0,
            },
            Shape::RoundedRect {
                half: Vec2::new(40.0, 9.0),
                radius: 0.4,
            },
            Shape::Circle { radius: 17.25 },
            Shape::Circle { radius: 0.8 },
            Shape::RoundedRectOutline {
                half: Vec2::new(30.0, 20.0),
                radius: 10.0,
                width: 1.0,
            },
            Shape::RoundedRectOutline {
                half: Vec2::new(25.0, 25.0),
                radius: 4.0,
                width: 3.5,
            },
            Shape::CircleOutline {
                radius: 16.0,
                width: 2.0,
            },
            Shape::Line {
                a: Vec2::new(-10.0, -5.0),
                b: Vec2::new(12.0, 7.0),
                width: 2.5,
            },
        ];
        for shape in shapes {
            for paint in [solid, translucent, gradient] {
                for (tx, ty) in [(50.0, 40.0), (50.37, 40.81), (3.5, 2.25)] {
                    for soft in [0.0, 6.0] {
                        let cmd = DrawCmd {
                            shape,
                            paint,
                            transform: Affine::translate(tx, ty),
                            soft,
                        };
                        let mut fast = Framebuffer::new(100, 80, bg);
                        let mut slow = Framebuffer::new(100, 80, bg);
                        scan_convert(&cmd, &mut fast, None);
                        reference(&cmd, &mut slow);
                        for (i, (a, b)) in fast.pixels().iter().zip(slow.pixels()).enumerate() {
                            assert!(
                                a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a,
                                "{shape:?} soft {soft} at ({tx},{ty}) pixel {i}: {a:?} != {b:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn span_fast_path_handles_clips_beside_the_interior() {
        // A clip strip left or right of a shape's interior run (or narrower than it) must
        // give an empty or partial run, never a panic, and match the per-pixel path.
        let bg = Rgba::new(0.2, 0.3, 0.4, 1.0);
        let paint = Paint::Solid(Rgba::new(0.9, 0.5, 0.1, 1.0));
        let shapes = [
            Shape::Rect {
                half: Vec2::new(30.0, 20.0),
            },
            Shape::RoundedRect {
                half: Vec2::new(30.0, 20.0),
                radius: 8.0,
            },
            Shape::Circle { radius: 20.0 },
            Shape::RoundedRectOutline {
                half: Vec2::new(30.0, 20.0),
                radius: 8.0,
                width: 1.0,
            },
        ];
        let strips = [
            (0.0, 15.0),
            (10.0, 22.0),
            (40.0, 60.0),
            (85.0, 100.0),
            (45.0, 46.0),
        ];
        for shape in shapes {
            for (sx0, sx1) in strips {
                let clip = Bounds {
                    min: Vec2::new(sx0, 0.0),
                    max: Vec2::new(sx1, 80.0),
                };
                let cmd = DrawCmd {
                    shape,
                    paint,
                    transform: Affine::translate(50.0, 40.0),
                    soft: 0.0,
                };
                let mut fast = Framebuffer::new(100, 80, bg);
                let mut slow = Framebuffer::new(100, 80, bg);
                scan_convert(&cmd, &mut fast, Some(clip));
                reference_clipped(&cmd, &mut slow, Some(clip));
                for (i, (a, b)) in fast.pixels().iter().zip(slow.pixels()).enumerate() {
                    assert!(
                        a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a,
                        "{shape:?} clipped to x {sx0}..{sx1}, pixel {i}: {a:?} != {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn outline_is_a_band_inside_the_edge() {
        let shape = Shape::RoundedRectOutline {
            half: Vec2::new(20.0, 10.0),
            radius: 4.0,
            width: 2.0,
        };
        assert!(
            signed_distance(&shape, Vec2::new(0.0, 9.0)) < 0.0,
            "inside the band"
        );
        assert!(
            signed_distance(&shape, Vec2::new(0.0, 0.0)) > 0.0,
            "hollow middle"
        );
        assert!(
            signed_distance(&shape, Vec2::new(0.0, 11.0)) > 0.0,
            "outside the edge"
        );
    }
}
