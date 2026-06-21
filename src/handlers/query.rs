//! Connection checks and query execution.

use serde_json::{json, Value};

use crate::client;
use crate::error::PluginError;
use crate::handlers::{connect_from, require_str};
use crate::utils::pagination::{count_query, is_select, paginate};

/// Lightweight liveness check. Opens a connection and runs a trivial query.
pub fn ping(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    client::run_select(&conn, "SELECT FIRST 1 tabid FROM systables")?;
    Ok(Value::Null)
}

/// Verifies that a connection can be established.
pub fn test_connection(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    client::run_select(&conn, "SELECT FIRST 1 tabid FROM systables")?;
    Ok(json!({ "success": true }))
}

/// Executes a SQL query. SELECT statements are paginated with `SKIP`/`FIRST`
/// and reported with a total row count; other statements run as-is and report
/// the number of affected rows.
pub fn execute_query(top: &Value) -> Result<Value, PluginError> {
    let query = require_str(top, "query")?.to_string();
    let page = top.get("page").and_then(Value::as_u64).unwrap_or(1).max(1);
    let page_size = top.get("limit").and_then(Value::as_u64);

    let conn = connect_from(top)?;

    if !is_select(&query) {
        client::run_exec(&conn, &query)?;
        let affected = client::affected_rows(&conn);
        return Ok(json!({
            "columns": [],
            "rows": [],
            "affected_rows": affected,
            "truncated": false,
            "pagination": Value::Null,
        }));
    }

    match page_size {
        Some(ps) if ps > 0 => {
            let total = client::run_scalar_i64(&conn, &count_query(&query))
                .filter(|n| *n >= 0)
                .map(|n| n as u64);
            let paged = paginate(&query, page, ps);
            let outcome = client::run_select(&conn, &paged)?;
            let returned = outcome.rows.len() as u64;
            let has_more = match total {
                Some(t) => page.saturating_mul(ps) < t,
                None => returned == ps,
            };
            Ok(json!({
                "columns": outcome.columns,
                "rows": outcome.rows,
                "affected_rows": returned,
                "truncated": false,
                "pagination": {
                    "page": page,
                    "page_size": ps,
                    "total_rows": total,
                    "has_more": has_more,
                },
            }))
        }
        _ => {
            let outcome = client::run_select(&conn, &query)?;
            let returned = outcome.rows.len() as u64;
            Ok(json!({
                "columns": outcome.columns,
                "rows": outcome.rows,
                "affected_rows": returned,
                "truncated": false,
                "pagination": Value::Null,
            }))
        }
    }
}
