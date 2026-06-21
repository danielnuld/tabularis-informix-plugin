//! Conversion of JSON values into Informix SQL literals.
//!
//! The plugin builds CRUD statements as literal SQL (rather than binding
//! parameters) so that values of any JSON shape can be embedded uniformly.
//! String literals use single quotes with embedded single quotes doubled,
//! per the SQL standard (and Informix with `DELIMIDENT=Y`).

use serde_json::Value;

/// Formats a JSON value as an Informix SQL literal.
///
/// - `null` -> `NULL`
/// - booleans -> `'t'` / `'f'` (Informix BOOLEAN literals)
/// - numbers -> verbatim
/// - strings / everything else -> single-quoted, escaped
pub fn format_sql_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "'t'".to_string()
            } else {
                "'f'".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => quote_string(s),
        // Objects/arrays are stringified as JSON text and stored as a string.
        other => quote_string(&other.to_string()),
    }
}

/// Single-quotes a string and escapes embedded single quotes by doubling.
pub fn quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push('\'');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn null_literal() {
        assert_eq!(format_sql_value(&Value::Null), "NULL");
    }

    #[test]
    fn boolean_literals() {
        assert_eq!(format_sql_value(&json!(true)), "'t'");
        assert_eq!(format_sql_value(&json!(false)), "'f'");
    }

    #[test]
    fn numbers_are_verbatim() {
        assert_eq!(format_sql_value(&json!(42)), "42");
        assert_eq!(format_sql_value(&json!(3.5)), "3.5");
    }

    #[test]
    fn strings_are_quoted_and_escaped() {
        assert_eq!(format_sql_value(&json!("O'Brien")), "'O''Brien'");
        assert_eq!(format_sql_value(&json!("plain")), "'plain'");
    }
}
