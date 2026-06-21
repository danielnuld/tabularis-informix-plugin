//! Decoding of Informix `syscolumns.coltype` / `collength` into SQL type names.
//!
//! Informix packs three things into `coltype`:
//!   * the base type code in the low byte (`coltype & 0xFF`, values 0..=53),
//!   * the NOT NULL flag in bit 8 (`coltype & 0x100`),
//!   * other internal flags above that we ignore.
//!
//! `collength` is type-dependent: a byte length for CHAR/NCHAR, an encoded
//! `(min, max)` pair for VARCHAR/NVARCHAR, a `(precision, scale)` pair for
//! DECIMAL/MONEY, and an encoded qualifier for DATETIME/INTERVAL.
//!
//! Reference: IBM Informix Guide to SQL: Reference — the `syscolumns` table.

const NOT_NULL_BIT: i64 = 0x100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedType {
    /// SQL type as it would appear in DDL, e.g. `VARCHAR(40)`, `DECIMAL(10,2)`.
    pub sql_type: String,
    /// Maximum character length for character types, if applicable.
    pub char_max_len: Option<u64>,
    pub is_not_null: bool,
    pub is_auto_increment: bool,
}

/// Decodes a column's type. `ext_name` is the name from `sysxtdtypes` for
/// opaque / extended types (BOOLEAN, BLOB, CLOB, LVARCHAR, distinct types).
pub fn decode_type(coltype: i64, collength: i64, ext_name: Option<&str>) -> DecodedType {
    let base = coltype & 0xFF;
    let is_not_null = (coltype & NOT_NULL_BIT) != 0;
    let is_auto_increment = matches!(base, 6 | 18 | 53);

    let ext = ext_name
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase());

    let (sql_type, char_max_len) = match base {
        0 => char_type("CHAR", collength),
        1 => ("SMALLINT".to_string(), None),
        2 => ("INTEGER".to_string(), None),
        3 => ("FLOAT".to_string(), None),
        4 => ("SMALLFLOAT".to_string(), None),
        5 => (decimal_type("DECIMAL", collength), None),
        6 => ("SERIAL".to_string(), None),
        7 => ("DATE".to_string(), None),
        8 => (decimal_type("MONEY", collength), None),
        10 => (datetime_type(collength), None),
        11 => ("BYTE".to_string(), None),
        12 => ("TEXT".to_string(), None),
        13 => varchar_type("VARCHAR", collength),
        14 => ("INTERVAL".to_string(), None),
        15 => char_type("NCHAR", collength),
        16 => varchar_type("NVARCHAR", collength),
        17 => ("INT8".to_string(), None),
        18 => ("SERIAL8".to_string(), None),
        19 => (ext.unwrap_or_else(|| "SET".to_string()), None),
        20 => (ext.unwrap_or_else(|| "MULTISET".to_string()), None),
        21 => (ext.unwrap_or_else(|| "LIST".to_string()), None),
        22 => (ext.unwrap_or_else(|| "ROW".to_string()), None),
        23 => (ext.unwrap_or_else(|| "COLLECTION".to_string()), None),
        // Variable-length opaque (LVARCHAR family) and fixed-length opaque
        // (BOOLEAN, BLOB, CLOB...). Prefer the real extended-type name.
        40 | 43 => {
            let len = collength.max(0) as u64;
            match ext {
                Some(name) => (name, None),
                None if len > 0 => (format!("LVARCHAR({len})"), Some(len)),
                None => ("LVARCHAR".to_string(), None),
            }
        }
        41 => (ext.unwrap_or_else(|| "BLOB".to_string()), None),
        45 => ("BOOLEAN".to_string(), None),
        52 => ("BIGINT".to_string(), None),
        53 => ("BIGSERIAL".to_string(), None),
        _ => (ext.unwrap_or_else(|| format!("UNKNOWN({base})")), None),
    };

    DecodedType {
        sql_type,
        char_max_len,
        is_not_null,
        is_auto_increment,
    }
}

fn char_type(name: &str, collength: i64) -> (String, Option<u64>) {
    let len = collength.max(0) as u64;
    if len == 0 {
        (name.to_string(), None)
    } else {
        (format!("{name}({len})"), Some(len))
    }
}

fn varchar_type(name: &str, collength: i64) -> (String, Option<u64>) {
    // collength = min * 256 + max ; max is the declared size.
    let max = (collength % 256).max(0) as u64;
    if max == 0 {
        (name.to_string(), None)
    } else {
        (format!("{name}({max})"), Some(max))
    }
}

fn decimal_type(name: &str, collength: i64) -> String {
    let precision = collength / 256;
    let scale = collength % 256;
    if precision <= 0 {
        return name.to_string();
    }
    // scale 255 marks a floating-point DECIMAL (no fixed scale).
    if scale == 255 || scale == 0 {
        format!("{name}({precision})")
    } else {
        format!("{name}({precision},{scale})")
    }
}

fn datetime_type(collength: i64) -> String {
    let q = collength % 256;
    let start = q / 16;
    let end = q % 16;
    let s = tu_name(start);
    let e = tu_name(end);
    match (s, e) {
        (Some(s), Some(e)) if s == e => format!("DATETIME {s}"),
        (Some(s), Some(e)) => format!("DATETIME {s} TO {e}"),
        _ => "DATETIME".to_string(),
    }
}

/// Maps an Informix time-unit code to its keyword.
fn tu_name(code: i64) -> Option<&'static str> {
    match code {
        0 => Some("YEAR"),
        2 => Some("MONTH"),
        4 => Some("DAY"),
        6 => Some("HOUR"),
        8 => Some("MINUTE"),
        10 => Some("SECOND"),
        11..=15 => Some("FRACTION"),
        _ => None,
    }
}

/// Decodes a `sysdefaults` entry into a display/DDL default expression.
///
/// `type_code` is the single-character `sysdefaults.type`; `text` is the
/// `sysdefaults.default` value (only meaningful for literal defaults). Keyword
/// defaults map to their SQL keyword. Literal defaults are best-effort: the
/// internal numeric class prefix Informix stores before numeric literals is
/// stripped. Returns `None` when there is no usable default.
pub fn decode_default(type_code: &str, text: &str) -> Option<String> {
    match type_code.trim() {
        "T" => Some("TODAY".to_string()),
        "C" => Some("CURRENT".to_string()),
        "U" => Some("USER".to_string()),
        "S" => Some("DBSERVERNAME".to_string()),
        "N" => None,
        "L" => {
            let t = text.trim();
            if t.is_empty() {
                return None;
            }
            // Numeric literals are stored as "<class> <value>"; drop the class.
            match t.split_once(' ') {
                Some((prefix, rest)) if prefix.parse::<i64>().is_ok() && !rest.trim().is_empty() => {
                    Some(rest.trim().to_string())
                }
                _ => Some(t.to_string()),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(coltype: i64, collength: i64) -> DecodedType {
        decode_type(coltype, collength, None)
    }

    #[test]
    fn plain_integer() {
        let d = t(2, 4);
        assert_eq!(d.sql_type, "INTEGER");
        assert!(!d.is_not_null);
        assert!(!d.is_auto_increment);
    }

    #[test]
    fn not_null_flag_detected() {
        let d = t(2 + 0x100, 4);
        assert_eq!(d.sql_type, "INTEGER");
        assert!(d.is_not_null);
    }

    #[test]
    fn serial_is_auto_increment() {
        let d = t(6 + 0x100, 4);
        assert_eq!(d.sql_type, "SERIAL");
        assert!(d.is_auto_increment);
    }

    #[test]
    fn bigserial_is_auto_increment() {
        assert!(t(53, 8).is_auto_increment);
    }

    #[test]
    fn char_and_varchar_lengths() {
        let c = t(0, 20);
        assert_eq!(c.sql_type, "CHAR(20)");
        assert_eq!(c.char_max_len, Some(20));

        let v = t(13, 40);
        assert_eq!(v.sql_type, "VARCHAR(40)");
        assert_eq!(v.char_max_len, Some(40));

        // VARCHAR(20,5): collength = 5*256 + 20 = 1300
        let v2 = t(13, 1300);
        assert_eq!(v2.sql_type, "VARCHAR(20)");
    }

    #[test]
    fn decimal_precision_scale() {
        // DECIMAL(10,2): collength = 10*256 + 2 = 2562
        assert_eq!(t(5, 2562).sql_type, "DECIMAL(10,2)");
        // MONEY(16,2): collength = 16*256 + 2 = 4098
        assert_eq!(t(8, 4098).sql_type, "MONEY(16,2)");
        // Floating DECIMAL(10): scale 255
        assert_eq!(t(5, 10 * 256 + 255).sql_type, "DECIMAL(10)");
    }

    #[test]
    fn datetime_qualifier() {
        // YEAR TO SECOND: start=0, end=10 -> q = 0*16+10 = 10
        assert_eq!(t(10, 10).sql_type, "DATETIME YEAR TO SECOND");
        // HOUR TO MINUTE: start=6, end=8 -> q = 6*16+8 = 104
        assert_eq!(t(10, 104).sql_type, "DATETIME HOUR TO MINUTE");
    }

    #[test]
    fn bigint_and_boolean() {
        assert_eq!(t(52, 8).sql_type, "BIGINT");
        assert_eq!(t(45, 1).sql_type, "BOOLEAN");
    }

    #[test]
    fn opaque_uses_extended_name() {
        let d = decode_type(41, 0, Some("boolean"));
        assert_eq!(d.sql_type, "BOOLEAN");
        let blob = decode_type(41, 0, Some("blob"));
        assert_eq!(blob.sql_type, "BLOB");
    }

    #[test]
    fn default_keywords_and_literals() {
        assert_eq!(decode_default("T", ""), Some("TODAY".to_string()));
        assert_eq!(decode_default("C", ""), Some("CURRENT".to_string()));
        assert_eq!(decode_default("U", ""), Some("USER".to_string()));
        assert_eq!(decode_default("N", ""), None);
        // Numeric literal: class prefix stripped.
        assert_eq!(decode_default("L", "0 100"), Some("100".to_string()));
        // Plain literal kept as-is.
        assert_eq!(decode_default("L", "pending"), Some("pending".to_string()));
        assert_eq!(decode_default("L", ""), None);
    }
}
