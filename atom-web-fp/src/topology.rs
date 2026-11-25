//! Shared 16-strand, 4-arc spiderweb graph topology.

pub const STRANDS: usize = 16;
pub const ARCS: usize = 4;
pub const BINS: usize = 8;

/// Clockwise spiral neighbor (`(v + 1) % 16` when connected, else `v`).
#[inline]
pub const fn cw(v: usize, connected: bool) -> usize {
    if connected { (v + 1) & 15 } else { v }
}

/// Counter-clockwise spiral neighbor (`(v + 15) % 16` when connected, else `v`).
#[inline]
pub const fn ccw(v: usize, connected: bool) -> usize {
    if connected { (v + 15) & 15 } else { v }
}

/// Cross-web chord partner (`v ^ 8` when connected, else `v`).
#[inline]
pub const fn chord(v: usize, connected: bool) -> usize {
    if connected { v ^ 8 } else { v }
}
