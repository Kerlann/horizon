//! Java's floating-point spelling, shared by SNBT and JSON numeric conversions.

/// Java `Double.toString`.
pub fn java_double(d: f64) -> String {
    java_fp_string(d, format!("{d:e}"))
}
/// Java `Float.toString`.
pub fn java_float(f: f32) -> String {
    java_fp_string(f64::from(f), format!("{f:e}"))
}
fn java_fp_string(d: f64, sci: String) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // `sci` is Rust's shortest round-trip form: d.ddde<exp>.
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        if exp >= 0 {
            let e = exp as usize;
            let (int, frac) = if digits.len() > e + 1 {
                (digits[..=e].to_string(), digits[e + 1..].to_string())
            } else {
                (format!("{digits:0<width$}", width = e + 1), String::new())
            };
            let frac = if frac.is_empty() { "0".into() } else { frac };
            format!("{sign}{int}.{frac}")
        } else {
            let zeros = "0".repeat((-exp - 1) as usize);
            format!("{sign}0.{zeros}{digits}")
        }
    } else {
        if digits.len() == 1 {
            // Select the nearest two-digit decimal, rather than padding the
            // one-digit round-trip form; Java can choose a different exponent.
            return format!("{d:.1e}").replace('e', "E");
        }
        let (first, rest) = digits.split_at(1);
        format!("{sign}{first}.{rest}E{exp}")
    }
}
/// Digits and decimal exponent of the shortest round-trip form.
fn shortest(text: &str) -> (String, i32) {
    // `{:e}` yields "d.ddde-5" or "de5".
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exponent.parse().unwrap_or(0))
}

fn render(negative: bool, digits: &str, exponent: i32) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let digits = if digits.is_empty() { "0" } else { digits };
    if (-3..7).contains(&exponent) {
        if exponent >= 0 {
            let int_len = exponent as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if digits.len() > 1 {
            out.push_str(&digits[1..]);
        } else {
            out.push('0');
        }
        out.push('E');
        out.push_str(&exponent.to_string());
    }
    out
}

/// `Double.toString`.
pub fn double_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    let (digits, exponent) = shortest(&format!("{:e}", value.abs()));
    render(value < 0.0, &digits, exponent)
}

/// `Float.toString`.
pub fn float_to_string(value: f32) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    let (digits, exponent) = shortest(&format!("{:e}", value.abs()));
    render(value < 0.0, &digits, exponent)
}
/// `String.format(Locale.ROOT, "%.<n>f", value)`: Java rounds the shortest
/// round-trip decimal expansion half-up (so 1.005 gives 1.01).
pub fn format_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    let negative = value.is_sign_negative();
    let (digits, exponent) = if value == 0.0 {
        ("0".to_owned(), 0)
    } else {
        shortest(&format!("{:e}", value.abs()))
    };
    let mut int_part: Vec<u8> = Vec::new();
    let mut frac: Vec<u8> = Vec::new();
    let d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let point = exponent + 1; // digits before the decimal point
    if point <= 0 {
        int_part.push(0);
        frac.extend(std::iter::repeat_n(0, (-point) as usize));
        frac.extend(&d);
    } else {
        let point = point as usize;
        for i in 0..point {
            int_part.push(d.get(i).copied().unwrap_or(0));
        }
        if d.len() > point {
            frac.extend(&d[point..]);
        }
    }
    let round_up = frac.get(decimals).is_some_and(|&x| x >= 5);
    frac.truncate(decimals);
    frac.resize(decimals, 0);
    if round_up {
        let mut carry = true;
        for x in frac.iter_mut().rev().chain(int_part.iter_mut().rev()) {
            if !carry {
                break;
            }
            if *x == 9 {
                *x = 0;
            } else {
                *x += 1;
                carry = false;
            }
        }
        if carry {
            int_part.insert(0, 1);
        }
    }
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.extend(int_part.iter().map(|&x| (b'0' + x) as char));
    if decimals > 0 {
        out.push('.');
        out.extend(frac.iter().map(|&x| (b'0' + x) as char));
    }
    out
}

/// `%.2f`.
pub fn format_fixed2(value: f64) -> String {
    format_fixed(value, 2)
}

#[cfg(test)]
mod moved_tests {
    use super::*;
    #[test]
    fn java_number_text() {
        assert_eq!(double_to_string(1.0), "1.0");
        assert_eq!(double_to_string(0.5), "0.5");
        assert_eq!(double_to_string(0.0001), "1.0E-4");
        assert_eq!(double_to_string(0.001), "0.001");
        assert_eq!(double_to_string(1234567.0), "1234567.0");
        assert_eq!(double_to_string(12345678.0), "1.2345678E7");
        assert_eq!(double_to_string(100.0), "100.0");
        assert_eq!(float_to_string(0.15), "0.15");
        assert_eq!(float_to_string(1.0e10), "1.0E10");
        assert_eq!(double_to_string(-2.5), "-2.5");
    }
}
