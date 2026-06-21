//! ODBC connection and execution layer for IBM Informix.
//!
//! Connections are opened per request from a single process-wide
//! `Environment`. Result sets are fetched as text (the most portable binding
//! across ODBC drivers) and converted to JSON; integer/float columns are
//! emitted as JSON numbers while decimals stay strings to preserve precision.

use odbc_api::buffers::TextRowSet;
use odbc_api::{environment, Connection, ConnectionOptions, Cursor, DataType, ResultSetMetadata};
use serde_json::Value;

use crate::config;
use crate::error::PluginError;
use crate::models::ConnectionParams;
use crate::utils::connstr::build_connection_string;

/// Rows fetched per ODBC round trip.
const BATCH_SIZE: usize = 2000;
/// Per-cell text buffer cap (1 MiB). Larger values are truncated.
const MAX_CELL_BYTES: usize = 1 << 20;

#[derive(Debug, Default)]
pub struct QueryOutcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// Opens a new Informix connection from the given parameters.
pub fn connect(params: &ConnectionParams) -> Result<Connection<'static>, PluginError> {
    let cfg = config::get();
    let conn_str = build_connection_string(params, &cfg)?;
    let env = environment().map_err(|e| PluginError::internal(format!("ODBC env: {e}")))?;
    env.connect_with_connection_string(&conn_str, ConnectionOptions::default())
        .map_err(PluginError::from)
}

/// Executes a SELECT and returns all rows. A statement that produces no result
/// set yields an empty outcome.
pub fn run_select(conn: &Connection, sql: &str) -> Result<QueryOutcome, PluginError> {
    let mut cursor = match conn.execute(sql, (), None)? {
        Some(c) => c,
        None => return Ok(QueryOutcome::default()),
    };

    let ncols = cursor.num_result_cols()? as u16;
    let mut columns = Vec::with_capacity(ncols as usize);
    let mut numeric = Vec::with_capacity(ncols as usize);
    for i in 1..=ncols {
        columns.push(cursor.col_name(i)?);
        numeric.push(numeric_kind(cursor.col_data_type(i)?));
    }

    let buffer = TextRowSet::for_cursor(BATCH_SIZE, &mut cursor, Some(MAX_CELL_BYTES))?;
    let mut block = cursor.bind_buffer(buffer)?;

    let mut rows = Vec::new();
    while let Some(batch) = block.fetch()? {
        for r in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(ncols as usize);
            for (c, &kind) in numeric.iter().enumerate() {
                let cell = batch.at_as_str(c, r).ok().flatten();
                row.push(cell_to_json(cell, kind));
            }
            rows.push(row);
        }
    }

    Ok(QueryOutcome { columns, rows })
}

/// Executes a statement that returns no rows (DML/DDL).
pub fn run_exec(conn: &Connection, sql: &str) -> Result<(), PluginError> {
    conn.execute(sql, (), None)?;
    Ok(())
}

/// Number of rows affected by the most recent statement on this connection,
/// using the Informix `DBINFO('sqlca.sqlerrd2')` pseudo-function.
pub fn affected_rows(conn: &Connection) -> u64 {
    run_scalar_i64(
        conn,
        "SELECT DBINFO('sqlca.sqlerrd2') FROM systables WHERE tabid = 1",
    )
    .unwrap_or(0)
    .max(0) as u64
}

/// Runs a query and returns the first column of the first row as i64.
pub fn run_scalar_i64(conn: &Connection, sql: &str) -> Option<i64> {
    let outcome = run_select(conn, sql).ok()?;
    let cell = outcome.rows.first()?.first()?;
    match cell {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NumKind {
    /// Emit as a JSON integer.
    Int,
    /// Emit as a JSON float.
    Float,
    /// Keep as a string (decimals, text, dates, everything else).
    Text,
}

fn numeric_kind(dt: DataType) -> NumKind {
    match dt {
        DataType::Integer | DataType::SmallInt | DataType::BigInt | DataType::TinyInt => {
            NumKind::Int
        }
        DataType::Float { .. } | DataType::Real | DataType::Double => NumKind::Float,
        _ => NumKind::Text,
    }
}

fn cell_to_json(cell: Option<&str>, kind: NumKind) -> Value {
    let Some(s) = cell else {
        return Value::Null;
    };
    match kind {
        NumKind::Int => s
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(s.to_string())),
        NumKind::Float => s
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(s.to_string())),
        NumKind::Text => Value::String(s.to_string()),
    }
}
