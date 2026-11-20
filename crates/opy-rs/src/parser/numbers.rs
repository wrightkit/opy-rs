pub(crate) fn number_token_value(text: &str) -> f64 {
    // OverPy 9.7.17 (#512): lowercase 0x reads a hex prefix, other radixes require a whole token.
    if let Some(hex) = text.strip_prefix("0x") {
        let mut digits = hex.chars().map_while(|c| c.to_digit(16));
        return digits.next().map_or(f64::NAN, |first| {
            digits.fold(first as f64, |value, digit| value * 16.0 + digit as f64)
        });
    }
    let radix = text.strip_prefix('0').and_then(|tail| {
        let radix = match tail.as_bytes().first()? {
            b'X' => 16,
            b'b' | b'B' => 2,
            b'o' | b'O' => 8,
            _ => return None,
        };
        Some((&tail[1..], radix))
    });
    if let Some((tail, radix)) = radix {
        let mut digits = tail.chars().map(|c| c.to_digit(radix));
        return digits.next().flatten().map_or(f64::NAN, |first| {
            digits
                .try_fold(first as f64, |value, digit| {
                    digit.map(|digit| value * radix as f64 + digit as f64)
                })
                .unwrap_or(f64::NAN)
        });
    }
    text.parse().unwrap_or(f64::NAN)
}
