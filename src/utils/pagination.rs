//! Pagination for Informix.
//!
//! Informix uses `SELECT SKIP n FIRST m ...` (the `SKIP`/`FIRST` clause comes
//! right after the `SELECT` keyword, before any `DISTINCT`). For an arbitrary
//! user `SELECT` we inject the clause after the leading keyword; for anything
//! that does not start with `SELECT` we leave it untouched.

/// Strips a single trailing semicolon and surrounding whitespace.
pub fn strip_trailing_semicolon(query: &str) -> &str {
    let q = query.trim();
    q.strip_suffix(';').map(str::trim_end).unwrap_or(q)
}

/// Returns true when the (trimmed) query begins with the `SELECT` keyword.
pub fn is_select(query: &str) -> bool {
    let q = strip_trailing_semicolon(query).trim_start();
    let mut chars = q.chars();
    let head: String = (&mut chars).take(6).collect();
    if !head.eq_ignore_ascii_case("select") {
        return false;
    }
    // Next char must be whitespace or end-of-string (avoid matching "selectx").
    chars.next().map(|c| c.is_whitespace()).unwrap_or(true)
}

/// Injects `SKIP <offset> FIRST <page_size>` into a `SELECT`. Page numbers are
/// 1-indexed. Non-`SELECT` queries are returned unchanged.
///
/// ```
/// assert_eq!(
///     paginate("SELECT * FROM orders", 2, 50),
///     "SELECT SKIP 50 FIRST 50 * FROM orders"
/// );
/// ```
pub fn paginate(query: &str, page: u64, page_size: u64) -> String {
    let q = strip_trailing_semicolon(query);
    if !is_select(q) {
        return q.to_string();
    }
    let safe_page = page.max(1);
    let offset = (safe_page - 1).saturating_mul(page_size);
    let trimmed = q.trim_start();
    // Length of leading whitespace removed by trim_start.
    let lead = &q[..q.len() - trimmed.len()];
    let after_select = &trimmed[6..]; // everything past "SELECT"
    // If the query already limits itself with SKIP/FIRST, leave it untouched to
    // avoid producing an invalid double clause.
    let next = after_select.trim_start().to_ascii_uppercase();
    if next.starts_with("SKIP ") || next.starts_with("FIRST ") {
        return q.to_string();
    }
    format!("{lead}SELECT SKIP {offset} FIRST {page_size}{after_select}")
}

/// Wraps a query so its total row count can be obtained in one shot.
///
/// ```
/// assert_eq!(
///     count_query("SELECT a FROM t WHERE a > 1"),
///     "SELECT COUNT(*) FROM (SELECT a FROM t WHERE a > 1) AS tab_count"
/// );
/// ```
pub fn count_query(query: &str) -> String {
    let q = strip_trailing_semicolon(query);
    format!("SELECT COUNT(*) FROM ({q}) AS tab_count")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page() {
        assert_eq!(
            paginate("SELECT * FROM orders", 1, 100),
            "SELECT SKIP 0 FIRST 100 * FROM orders"
        );
    }

    #[test]
    fn later_page() {
        assert_eq!(
            paginate("SELECT a, b FROM t", 3, 25),
            "SELECT SKIP 50 FIRST 25 a, b FROM t"
        );
    }

    #[test]
    fn preserves_distinct() {
        assert_eq!(
            paginate("SELECT DISTINCT name FROM t", 1, 10),
            "SELECT SKIP 0 FIRST 10 DISTINCT name FROM t"
        );
    }

    #[test]
    fn page_zero_is_first_page() {
        assert_eq!(
            paginate("SELECT 1 FROM t", 0, 10),
            "SELECT SKIP 0 FIRST 10 1 FROM t"
        );
    }

    #[test]
    fn strips_trailing_semicolon() {
        assert_eq!(
            paginate("SELECT * FROM t;", 1, 5),
            "SELECT SKIP 0 FIRST 5 * FROM t"
        );
    }

    #[test]
    fn non_select_unchanged() {
        assert_eq!(
            paginate("UPDATE t SET a = 1", 1, 10),
            "UPDATE t SET a = 1"
        );
        assert!(!is_select("SELECTED FROM t"));
        assert!(is_select("  select * from t"));
    }

    #[test]
    fn skips_injection_when_already_limited() {
        assert_eq!(
            paginate("SELECT FIRST 10 * FROM t", 1, 100),
            "SELECT FIRST 10 * FROM t"
        );
        assert_eq!(
            paginate("SELECT SKIP 5 FIRST 10 * FROM t", 2, 100),
            "SELECT SKIP 5 FIRST 10 * FROM t"
        );
    }

    #[test]
    fn count_wraps_query() {
        assert_eq!(
            count_query("SELECT a FROM t"),
            "SELECT COUNT(*) FROM (SELECT a FROM t) AS tab_count"
        );
    }
}
