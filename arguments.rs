//! Bounded command arguments shared by the kernel and userspace runtime.
use alloc::{string::String, vec::Vec};
use crate::abi::{MAX_ARGS, MAX_ARG_BYTES};

pub fn pack(name: &str, extra: &[u8]) -> Result<Vec<u8>, ()> {
    if name.is_empty() || name.len() > 63 || name.as_bytes().contains(&0)
        || name.len() + 1 + extra.len() > MAX_ARG_BYTES { return Err(()); }
    if !extra.is_empty() && extra.last() != Some(&0) { return Err(()); }
    core::str::from_utf8(extra).map_err(|_| ())?;
    if 1 + extra.iter().filter(|&&b| b == 0).count() > MAX_ARGS { return Err(()); }
    let mut packed = Vec::with_capacity(name.len() + 1 + extra.len());
    packed.extend_from_slice(name.as_bytes()); packed.push(0); packed.extend_from_slice(extra);
    Ok(packed)
}

/// Shell quoting for program launches: single/double quotes and backslash
/// escaping outside single quotes. Empty quoted arguments are retained.
pub fn words(input: &str) -> Result<Vec<String>, ()> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    let mut started = false;
    for ch in input.chars() {
        if ch == '\0' { return Err(()); }
        if escape { word.push(ch); escape = false; started = true; }
        else if ch == '\\' && quote != Some('\'') { escape = true; started = true; }
        else if quote == Some(ch) { quote = None; }
        else if quote.is_none() && (ch == '\'' || ch == '"') { quote = Some(ch); started = true; }
        else if quote.is_none() && ch.is_whitespace() {
            if started { result.push(core::mem::take(&mut word)); started = false; }
        } else { word.push(ch); started = true; }
        if result.len() > MAX_ARGS || input.len() > MAX_ARG_BYTES { return Err(()); }
    }
    if quote.is_some() || escape { return Err(()); }
    if started { result.push(word); }
    if result.is_empty() || result.len() > MAX_ARGS { return Err(()); }
    Ok(result)
}
