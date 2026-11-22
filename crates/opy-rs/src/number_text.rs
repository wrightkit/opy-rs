/// JavaScript `toFixed(2)` followed by removal of `.00`: the exact binary value
/// is rounded to two decimals, exact ties away from zero, and a negative value
/// that rounds to zero keeps its sign (`-0`).
pub(crate) fn workshop_number_text(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let scaled = value.abs() * 100.0;
    // An exact tie needs a fraction that is an odd multiple of 1/8; Rust rounds
    // those to even, `toFixed` away from zero.
    let tie =
        (value.abs().fract() * 8.0).fract() == 0.0 && (value.abs().fract() * 8.0) % 2.0 == 1.0;
    let magnitude = if tie {
        scaled.ceil() / 100.0
    } else {
        value.abs()
    };
    let text = format!("{magnitude:.2}").replace(".00", "");
    if value.is_sign_negative() {
        format!("-{text}")
    } else {
        text
    }
}
