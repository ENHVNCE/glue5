//! Parts of the C library compiled into the game that the host does
//! faster with the same results.

use glue5_game::{Env, orig};

use crate::memory::Mem;

/// Hook for `strtod`: musl's goes through binary128 software floats, ~210
/// ns a number of the game's data (it has ~360,000 at boot: timecycles,
/// handling, weapons); Rust's parser takes ~10. Both round correctly: the
/// same double for every decimal. Hex, inf/nan, no digits, and results that
/// are out of range or subnormal (musl sets errno) stay musl's.
pub(crate) unsafe extern "C" fn strtod(env: *mut Env, s: u64, end: u64) -> f64 {
    let mem = unsafe { Mem::of(env) };
    let mut i = s;
    while matches!(mem.u8(i), b' ' | b'\t'..=b'\r') {
        i += 1;
    }
    let start = i;
    if matches!(mem.u8(i), b'+' | b'-') {
        i += 1;
    }
    if mem.u8(i) == b'0' && matches!(mem.u8(i + 1), b'x' | b'X') {
        return unsafe { orig::strtod(env, s, end) };
    }
    // Digits, a point and digits: how many, any but zeros.
    let (mut digits, mut nonzero) = (0u32, false);
    let mut point = false;
    loop {
        match mem.u8(i) {
            b'0'..=b'9' => {
                nonzero |= mem.u8(i) != b'0';
                digits += 1;
            }
            b'.' if !point => point = true,
            _ => break,
        }
        i += 1;
    }
    if digits == 0 {
        return unsafe { orig::strtod(env, s, end) };
    }
    // An exponent only with digits (else the number ends before the `e`).
    if matches!(mem.u8(i), b'e' | b'E') {
        let mut j = i + 1;
        if matches!(mem.u8(j), b'+' | b'-') {
            j += 1;
        }
        if mem.u8(j).is_ascii_digit() {
            while mem.u8(j).is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    // ASCII only (scanned above).
    let text = String::from_utf8_lossy(mem.bytes(start, (i - start) as usize));
    match text.parse::<f64>() {
        Ok(v) if v.is_finite() && !v.is_subnormal() && (v != 0.0 || !nonzero) => {
            if end != 0 {
                mem.set_u64(end, i);
            }
            v
        }
        _ => unsafe { orig::strtod(env, s, end) },
    }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut glue5_game::Hooks) {
    hooks.strtod = Some(strtod);
}

#[cfg(test)]
mod tests {
    #[test]
    fn rust_parses_what_c_does() {
        // The forms the scan hands over (C's strtod takes all of them).
        for (text, want) in [("1.", 1.0), (".5", 0.5), ("+2.25", 2.25), ("-0.0012345", -0.0012345), ("1e3", 1000.0), ("2.5E-3", 0.0025)] {
            assert_eq!(text.parse::<f64>().unwrap(), want, "{text}");
        }
        assert!("-0.0".parse::<f64>().unwrap().is_sign_negative());
    }
}
