//! Row-level CRUD. Values are embedded as SQL literals (see `utils::values`)
//! and affected-row counts come from `DBINFO('sqlca.sqlerrd2')`.

use serde_json::{json, Value};

use odbc_api::Connection;

use crate::client;
use crate::error::PluginError;
use crate::handlers::{connect_from, require_str};
use crate::utils::identifiers::quote_identifier;
use crate::utils::values::format_sql_value;

/// Builds a `"col" = <value>` equality predicate for a row identifier.
fn where_eq(col: &str, val: &serde_json::Value) -> String {
    format!("{} = {}", quote_identifier(col), format_sql_value(val))
}

/// Safety net for grid edits. Tabularis identifies a row to update/delete by a
/// single primary-key column, so on a table with a *composite* primary key the
/// `WHERE` matches more than one row. We refuse the operation in that case to
/// avoid silently modifying many rows (e.g. on a production database).
fn guard_single_row(
    conn: &Connection,
    table: &str,
    where_clause: &str,
    op: &str,
) -> Result<(), PluginError> {
    let count_sql = format!(
        "SELECT COUNT(*) FROM {} WHERE {where_clause}",
        quote_identifier(table)
    );
    match client::run_scalar_i64(conn, &count_sql) {
        Some(n) if n > 1 => Err(PluginError::invalid_params(format!(
            "refusing to {op}: the row identifier ({where_clause}) matches {n} rows, not 1. \
             This table likely has a composite primary key — edit it from the SQL editor with a \
             WHERE clause that covers every key column."
        ))),
        _ => Ok(()),
    }
}

pub fn insert_record(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let data = top
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| PluginError::invalid_params("insert_record requires a 'data' object"))?;

    if data.is_empty() {
        return Err(PluginError::invalid_params(
            "insert_record requires at least one column",
        ));
    }

    let mut cols = Vec::with_capacity(data.len());
    let mut vals = Vec::with_capacity(data.len());
    for (k, v) in data {
        cols.push(quote_identifier(k));
        vals.push(format_sql_value(v));
    }

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_identifier(&table),
        cols.join(", "),
        vals.join(", ")
    );

    let conn = connect_from(top)?;
    client::run_exec(&conn, &sql)?;
    Ok(json!(client::affected_rows(&conn).max(1)))
}

pub fn update_record(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let pk_col = require_str(top, "pk_col")?.to_string();
    let col_name = require_str(top, "col_name")?.to_string();
    let pk_val = top.get("pk_val").cloned().unwrap_or(Value::Null);
    let new_val = top.get("new_val").cloned().unwrap_or(Value::Null);

    let where_clause = where_eq(&pk_col, &pk_val);
    let sql = format!(
        "UPDATE {} SET {} = {} WHERE {where_clause}",
        quote_identifier(&table),
        quote_identifier(&col_name),
        format_sql_value(&new_val),
    );

    let conn = connect_from(top)?;
    guard_single_row(&conn, &table, &where_clause, "update")?;
    client::run_exec(&conn, &sql)?;
    Ok(json!(client::affected_rows(&conn)))
}

pub fn delete_record(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let pk_col = require_str(top, "pk_col")?.to_string();
    let pk_val = top.get("pk_val").cloned().unwrap_or(Value::Null);

    let where_clause = where_eq(&pk_col, &pk_val);
    let sql = format!(
        "DELETE FROM {} WHERE {where_clause}",
        quote_identifier(&table)
    );

    let conn = connect_from(top)?;
    guard_single_row(&conn, &table, &where_clause, "delete")?;
    client::run_exec(&conn, &sql)?;
    Ok(json!(client::affected_rows(&conn)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn where_eq_quotes_identifier_and_value() {
        assert_eq!(where_eq("id", &json!(42)), "\"id\" = 42");
        assert_eq!(where_eq("name", &json!("O'Brien")), "\"name\" = 'O''Brien'");
        assert_eq!(where_eq("active", &json!(true)), "\"active\" = 't'");
    }
}
