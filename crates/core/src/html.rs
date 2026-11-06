//! HTML escaping and unescaping, following Python's `html.escape` and `html.unescape`.

mod entities;

use std::fmt::{self, Write};

use entities::ENTITIES;

/// Escape `&`, `<` and `>` in `text`, and also quotes when `quote` is set.
#[must_use]
pub fn escape(text: &str, quote: bool) -> Escape<'_> {
    Escape { text, quote }
}

/// Decode the numeric and named character references in `text`.
///
/// Text that is not a valid reference is kept as is.
#[must_use]
pub fn unescape(text: &str) -> Unescape<'_> {
    Unescape(text)
}

/// Displays its text HTML-escaped; see [`escape`].
#[derive(Clone, Copy, Debug)]
pub struct Escape<'a> {
    text: &'a str,
    quote: bool,
}

impl fmt::Display for Escape<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut unescaped = 0;
        for (index, byte) in self.text.bytes().enumerate() {
            let replacement = match byte {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' if self.quote => "&quot;",
                b'\'' if self.quote => "&#x27;",
                _ => continue,
            };
            f.write_str(&self.text[unescaped..index])?;
            f.write_str(replacement)?;
            unescaped = index + 1;
        }
        f.write_str(&self.text[unescaped..])
    }
}

/// Displays its text with character references decoded; see [`unescape`].
#[derive(Clone, Copy, Debug)]
pub struct Unescape<'a>(&'a str);

impl fmt::Display for Unescape<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rest = self.0;
        while let Some(ampersand) = rest.find('&') {
            f.write_str(&rest[..ampersand])?;
            let reference = &rest[ampersand + 1..];
            rest = if let Some((code_point, len)) = numeric_reference(reference) {
                write_code_point(f, code_point)?;
                &reference[len..]
            } else if let Some((text, len)) = named_reference(reference) {
                f.write_str(text)?;
                &reference[len..]
            } else {
                f.write_char('&')?;
                reference
            };
        }
        f.write_str(rest)
    }
}

/// Parse a reference like `#38;` or `#x26` into its code point and length in bytes.
fn numeric_reference(reference: &str) -> Option<(u32, usize)> {
    let number = reference.strip_prefix('#')?;
    let (radix, digits) = match number.strip_prefix(['x', 'X']) {
        Some(hex) => (16, hex),
        None => (10, number),
    };
    let len = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    if len == 0 {
        return None;
    }
    // Values beyond `u32` are as invalid as any other out-of-range code point.
    let code_point = u32::from_str_radix(&digits[..len], radix).unwrap_or(0);
    let semicolon = usize::from(digits[len..].starts_with(';'));
    Some((code_point, reference.len() - digits.len() + len + semicolon))
}

/// Look up a reference like `amp;`, or else the longest legacy name that prefixes it, like `amp` in `ampx`.
fn named_reference(reference: &str) -> Option<(&'static str, usize)> {
    let len = reference
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(reference.len());
    if reference[len..].starts_with(';') {
        return lookup(&reference[..=len]).map(|text| (text, len + 1));
    }
    (1..=len)
        .rev()
        .find_map(|prefix| lookup(&reference[..prefix]).map(|text| (text, prefix)))
}

fn lookup(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(entity, _)| entity.cmp(&name))
        .ok()
        .map(|index| ENTITIES[index].1)
}

/// Windows-1252 interpretations of the C1 control characters U+0080 to U+009F.
/// Undefined positions are written as escape sequences rather than raw control characters.
const C1_REPLACEMENTS: [&str; 32] = [
    "\u{20ac}", "\\x81", "\u{201a}", "\u{0192}", "\u{201e}", "\u{2026}", "\u{2020}", "\u{2021}",
    "\u{02c6}", "\u{2030}", "\u{0160}", "\u{2039}", "\u{0152}", "\\x8d", "\u{017d}", "\\x8f",
    "\\x90", "\u{2018}", "\u{2019}", "\u{201c}", "\u{201d}", "\u{2022}", "\u{2013}", "\u{2014}",
    "\u{02dc}", "\u{2122}", "\u{0161}", "\u{203a}", "\u{0153}", "\\x9d", "\u{017e}", "\u{0178}",
];

/// Write a numeric reference's character, dropping control characters and noncharacters.
/// A carriage return is written as an escape sequence, like the undefined C1 characters.
fn write_code_point(out: &mut impl Write, code_point: u32) -> fmt::Result {
    match code_point {
        0x0d => out.write_str("\\r"),
        0x00 | 0xd800..=0xdfff | 0x11_0000.. => out.write_char(char::REPLACEMENT_CHARACTER),
        0x80..=0x9f => out.write_str(C1_REPLACEMENTS[code_point as usize - 0x80]),
        0x01..=0x08 | 0x0b | 0x0e..=0x1f | 0x7f | 0xfdd0..=0xfdef => Ok(()),
        _ if code_point & 0xfffe == 0xfffe => Ok(()),
        _ => char::from_u32(code_point).map_or(Ok(()), |c| out.write_char(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape(text: &str, quote: bool) -> String {
        super::escape(text, quote).to_string()
    }

    fn unescape(text: &str) -> String {
        super::unescape(text).to_string()
    }

    #[test]
    fn entities_are_sorted() {
        assert!(ENTITIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape("'<script>\"&foo;\"</script>'", true),
            "&#x27;&lt;script&gt;&quot;&amp;foo;&quot;&lt;/script&gt;&#x27;"
        );
        assert_eq!(
            escape("'<script>\"&foo;\"</script>'", false),
            "'&lt;script&gt;\"&amp;foo;\"&lt;/script&gt;'"
        );
        assert_eq!(escape("ß & 🦆", true), "ß &amp; 🦆");
        assert_eq!(escape("", true), "");
    }

    #[test]
    fn unescapes_named_references() {
        assert_eq!(unescape("I&heartsuit;duckdb"), "I♥duckdb");
        assert_eq!(unescape("&notin;"), "∉");
        assert_eq!(unescape("not &notin not"), "not ¬in not");
        assert_eq!(unescape("&notnot"), "¬not");
        assert_eq!(
            unescape("&Eacuteric&Eacute;ric&alphacentauri&alpha;centauri"),
            "ÉricÉric&alphacentauriαcentauri"
        );
        assert_eq!(unescape("&no valid &charref;"), "&no valid &charref;");
        assert_eq!(unescape("abc&a;&&not;"), "abc&a;&¬");
        assert_eq!(unescape("&CounterClockwiseContourIntegral;"), "∳");
    }

    #[test]
    fn unescapes_numeric_references() {
        assert_eq!(unescape("&#129414"), "🦆");
        assert_eq!(unescape("&#1294d14"), "Ԏd14");
        assert_eq!(unescape("&#x80z"), "€z");
        assert_eq!(unescape("&#x8w0"), "w0");
        assert_eq!(unescape("&#x22;quot;"), "\"quot;");
        assert_eq!(unescape("&#x0d"), "\\r");
        assert_eq!(unescape("&#x9D;"), "\\x9d");
        assert_eq!(unescape("&#x0000000000000000b"), "");
    }

    #[test]
    fn replaces_invalid_code_points() {
        for reference in [
            "&#0;",
            "&#xD800;",
            "&#x110000",
            "&#xffffffff",
            "&#x1000000000000000000",
            "&#99999999999999999999;",
        ] {
            assert_eq!(unescape(reference), "\u{fffd}", "{reference}");
        }
    }

    #[test]
    fn keeps_incomplete_references() {
        for text in [
            "&", "&#", "&#x", "&#x&#x", "&#X;", "&#y", "&#xy", "&#Xy;", "&#-1;",
        ] {
            assert_eq!(unescape(text), text);
        }
    }

    #[test]
    fn copies_plain_text() {
        assert_eq!(unescape("ß&amp;é"), "ß&é");
        assert_eq!(unescape("a\r\u{b}b\u{1}"), "a\r\u{b}b\u{1}");
    }
}
