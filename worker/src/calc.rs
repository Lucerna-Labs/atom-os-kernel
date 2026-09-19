//! calc — integer arithmetic with an explicit bounded stack (the pushdown parser in miniature).

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

const PARSE: u8 = 1; const DIV_ZERO: u8 = 2;

// Precedence: parens (structural) > unary minus ~ > * / % > + -. Left-associative.
fn precedence(op: u8) -> u8 {
    match op { b'+' | b'-' => 1, b'*' | b'/' | b'%' => 2, b'~' => 3, _ => 0 }
}

fn fold(nums: &mut [u64; 32], top: &mut usize, op: u8) -> Result<(), u8> {
    if op == b'~' {
        if *top == 0 { return Err(PARSE); }
        nums[*top - 1] = nums[*top - 1].wrapping_neg(); return Ok(());
    }
    if *top < 2 { return Err(PARSE); }
    let (a, b) = (nums[*top - 2], nums[*top - 1]);
    *top -= 1;
    nums[*top - 1] = match op {
        b'+' => a.wrapping_add(b), b'-' => a.wrapping_sub(b), b'*' => a.wrapping_mul(b),
        b'/' if b != 0 => a / b, b'%' if b != 0 => a % b, _ => return Err(DIV_ZERO),
    };
    Ok(())
}

fn eval(bytes: &[u8]) -> Result<u64, u8> {
    let mut nums = [0u64; 32];
    let mut ops = [0u8; 32];
    let (mut top, mut depth, mut i) = (0usize, 0usize, 0usize);
    let mut want_operand = true;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() { i += 1; } else if c.is_ascii_digit() {
            if !want_operand { return Err(PARSE); }
            let mut value = 0u64;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                value = value.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as u64);
                i += 1;
            }
            if top == 32 { return Err(PARSE); }
            nums[top] = value; top += 1;
            want_operand = false;
        } else {
            i += 1;
            match c {
                b'(' => {
                    if !want_operand || depth == 32 { return Err(PARSE); }
                    ops[depth] = b'('; depth += 1;
                }
                b')' => {
                    if want_operand { return Err(PARSE); }
                    while depth > 0 && ops[depth - 1] != b'(' {
                        fold(&mut nums, &mut top, ops[depth - 1])?; depth -= 1;
                    }
                    if depth == 0 { return Err(PARSE); }
                    depth -= 1;
                }
                op @ (b'+' | b'-' | b'*' | b'/' | b'%') => {
                    if want_operand {
                        // in operand position only unary minus is admitted
                        if op != b'-' || depth == 32 { return Err(PARSE); }
                        ops[depth] = b'~'; depth += 1;
                    } else {
                        while depth > 0 && precedence(ops[depth - 1]) >= precedence(op) {
                            fold(&mut nums, &mut top, ops[depth - 1])?; depth -= 1;
                        }
                        if depth == 32 { return Err(PARSE); }
                        ops[depth] = op; depth += 1;
                        want_operand = true;
                    }
                }
                _ => return Err(PARSE),
            }
        }
    }
    if want_operand { return Err(PARSE); }
    while depth > 0 {
        if ops[depth - 1] == b'(' { return Err(PARSE); }
        fold(&mut nums, &mut top, ops[depth - 1])?; depth -= 1;
    }
    if top != 1 { return Err(PARSE); }
    Ok(nums[0])
}

fn main() {
    let argv = rt::args();
    if argv.len() < 2 {
        rt::print("usage: calc 2+3*4\n");
        rt::exit(1);
    }
    // argv[0] is the program name; the expression is the words after it, joined with no separator.
    let mut expr = [0u8; MAX_ARG_BYTES];
    let mut len = 0usize;
    for word in argv.iter().skip(1) {
        let take = (expr.len() - len).min(word.len());
        expr[len..len + take].copy_from_slice(&word.as_bytes()[..take]);
        len += take;
    }
    match eval(&expr[..len]) {
        Ok(value) => { rt::print_args(format_args!("= {value}\n")); rt::exit(0); }
        Err(DIV_ZERO) => { rt::print("calc: division by zero\n"); rt::exit(3); }
        Err(_) => {
            rt::print_args(format_args!(
                "calc: cannot parse '{}'\n",
                core::str::from_utf8(&expr[..len]).unwrap_or("")
            ));
            rt::exit(2);
        }
    }
}
