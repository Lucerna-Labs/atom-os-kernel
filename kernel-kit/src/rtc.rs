//! CMOS real-time clock, read as UTC seconds since the Unix epoch.
use crate::io::Port;

fn register(index: u8) -> u8 { Port::new(0x70).write(index); Port::new(0x71).read() }

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Converts raw CMOS fields (honouring the BCD and 12-hour flags in status
/// register B) to Unix seconds.
pub fn to_unix(raw: [u8; 6], status_b: u8) -> u64 {
    let bcd = |v: u8| if status_b & 4 == 0 { (v & 0x0f) + (v >> 4) * 10 } else { v };
    let [second, minute, hour, day, month, year] = raw;
    let mut hours = bcd(hour & 0x7f) as u64;
    if status_b & 2 == 0 && hour & 0x80 != 0 { hours = (hours % 12) + 12; }
    else if status_b & 2 == 0 && hours == 12 { hours = 0; }
    let days = days_from_civil(2000 + bcd(year) as i64, bcd(month) as u32, bcd(day) as u32);
    days.max(0) as u64 * 86400 + hours * 3600 + bcd(minute) as u64 * 60 + bcd(second) as u64
}

pub fn now() -> u64 {
    let read = || {
        while register(0x0a) & 0x80 != 0 { core::hint::spin_loop(); }
        [register(0x00), register(0x02), register(0x04), register(0x07), register(0x08), register(0x09)]
    };
    // Read until two consecutive samples agree, so no field rolls over mid-read.
    let mut sample = read();
    loop { let next = read(); if next == sample { break; } sample = next; }
    to_unix(sample, register(0x0b))
}
