//! SQL identifier quoting for Informix.
//!
//! Informix only honours double-quoted identifiers when `DELIMIDENT` is
//! enabled — the connection string sets `DELIMIDENT=Y` (see `connstr`) so the
//! quoting below is safe. A literal double quote inside an identifier is
//! escaped by doubling it.

pub const QUOTE: char = '"';

/// Quotes an identifier with double quotes, doubling embedded quotes.
///
/// ```
/// assert_eq!(quote_identifier("orders"), "\"orders\"");
/// ```
pub fn quote_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push(QUOTE);
    for c in name.chars() {
        if c == QUOTE {
            out.push(QUOTE);
        }
        out.push(c);
    }
    out.push(QUOTE);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_plain_names() {
        assert_eq!(quote_identifier("orders"), "\"orders\"");
        assert_eq!(quote_identifier("customer_num"), "\"customer_num\"");
    }

    #[test]
    fn escapes_embedded_quotes() {
        assert_eq!(quote_identifier("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn handles_empty() {
        assert_eq!(quote_identifier(""), "\"\"");
    }
}
