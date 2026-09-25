//! The number spelling of the pinned OverPy: a number is written as its
//! JavaScript text cut after fifteen decimals, exponent kept.

use workshop_rs::Value;

/// Cut every number in a value to what the reference writes for it.
pub(super) fn trim_numbers(value: &mut Value) {
    match value {
        Value::Number(number) => *number = trimmed(*number),
        Value::Array(values) => values.iter_mut().for_each(trim_numbers),
        Value::Vector { x, y, z } => [x, y, z].into_iter().for_each(|v| trim_numbers(v)),
        Value::PlayerVariable { player, .. } => trim_numbers(player),
        Value::Call { args, .. } => args.iter_mut().for_each(trim_numbers),
        _ => {}
    }
}

fn trimmed(number: f64) -> f64 {
    if !number.is_finite() {
        return number;
    }
    let text = javascript_text(number);
    let mut cut = text.clone();
    if let Some(point) = text.find('.') {
        cut.truncate(text.len().min(point + 16));
    }
    if let Some(exponent) = text.find('e')
        && !cut.contains('e')
    {
        cut.push_str(&text[exponent..]);
    }
    cut.parse().unwrap_or(number)
}

/// `Number.prototype.toString` for a finite number.
fn javascript_text(number: f64) -> String {
    if number == 0.0 {
        return "0".to_string();
    }
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("scientific notation has an exponent");
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let point: i32 = exponent.parse::<i32>().expect("exponent is an integer") + 1;
    let length = digits.len() as i32;
    let body = if length <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - length) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let sign = if point - 1 < 0 { '-' } else { '+' };
        let power = (point - 1).abs();
        if length == 1 {
            format!("{digits}e{sign}{power}")
        } else {
            format!("{}.{}e{sign}{power}", &digits[..1], &digits[1..])
        }
    };
    if number < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str) -> f64 {
        text.parse().expect("test number")
    }

    #[test]
    fn cuts_after_fifteen_decimals() {
        assert_eq!(
            trimmed(number("0.05233595624294383")),
            number("0.052335956242943")
        );
        assert_eq!(
            trimmed(number("0.7071067811865476")),
            number("0.707106781186547")
        );
        assert_eq!(trimmed(-1.0 / 3.0), number("-0.333333333333333"));
        assert_eq!(
            trimmed(number("123456789.1234567")),
            number("123456789.1234567")
        );
        assert_eq!(trimmed(3.0), 3.0);
    }

    #[test]
    fn keeps_the_exponent_of_a_cut_number() {
        let tiny = number("1.2345678901234566e-7");
        assert_eq!(javascript_text(tiny), "1.2345678901234566e-7");
        assert_eq!(trimmed(tiny), number("1.234567890123456e-7"));
        assert_eq!(javascript_text(1e21), "1e+21");
        assert_eq!(javascript_text(0.000001), "0.000001");
    }
}
