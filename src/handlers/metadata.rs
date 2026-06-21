//! Schema metadata extracted from the Informix system catalogs
//! (`systables`, `syscolumns`, `sysindexes`, `sysconstraints`,
//! `sysreferences`, `sysviews`, `sysprocedures`, ...).
//!
//! Schemas (owners) are not exposed as a separate namespace; tables are listed
//! and referenced unqualified within the connected database.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::client;
use crate::error::PluginError;
use crate::handlers::{connect_from, require_str};
use crate::models::connection_params;
use crate::utils::types::{decode_default, decode_type};
use crate::utils::values::quote_string;
use odbc_api::Connection;

// ---------------------------------------------------------------------------
// Row helpers
// ---------------------------------------------------------------------------

fn cint(row: &[Value], i: usize) -> i64 {
    match row.get(i) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn cstr(row: &[Value], i: usize) -> String {
    match row.get(i) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Catalog primitives
// ---------------------------------------------------------------------------

/// Lists user object names of the given `tabtype` ('T' table, 'V' view).
fn list_objects(conn: &Connection, tabtype: &str) -> Result<Vec<String>, PluginError> {
    let sql = format!(
        "SELECT TRIM(tabname) FROM systables \
         WHERE tabid >= 100 AND tabtype = '{tabtype}' ORDER BY tabname"
    );
    let out = client::run_select(conn, &sql)?;
    Ok(out.rows.iter().map(|r| cstr(r, 0)).collect())
}

/// Resolves a table/view name to its `tabid`.
fn tabid_of(conn: &Connection, name: &str) -> Result<Option<i64>, PluginError> {
    let sql = format!(
        "SELECT tabid FROM systables WHERE tabname = {} AND tabid >= 100",
        quote_string(name)
    );
    let out = client::run_select(conn, &sql)?;
    Ok(out.rows.first().map(|r| cint(r, 0)))
}

/// Maps column numbers to names for a table.
fn colno_names(conn: &Connection, tabid: i64) -> Result<HashMap<i64, String>, PluginError> {
    let sql = format!("SELECT colno, TRIM(colname) FROM syscolumns WHERE tabid = {tabid}");
    let out = client::run_select(conn, &sql)?;
    Ok(out
        .rows
        .iter()
        .map(|r| (cint(r, 0), cstr(r, 1)))
        .collect())
}

/// Column numbers participating in the primary key.
fn pk_colnos(conn: &Connection, tabid: i64) -> Result<HashSet<i64>, PluginError> {
    let sql = format!(
        "SELECT {parts} FROM sysindexes i \
         JOIN sysconstraints c ON c.idxname = i.idxname AND c.tabid = i.tabid \
         WHERE c.tabid = {tabid} AND c.constrtype = 'P'",
        parts = part_columns()
    );
    let out = client::run_select(conn, &sql)?;
    let mut set = HashSet::new();
    if let Some(row) = out.rows.first() {
        for i in 0..16 {
            let p = cint(row, i);
            if p != 0 {
                set.insert(p.abs());
            }
        }
    }
    Ok(set)
}

/// `part1, part2, ..., part16` for use in a SELECT list.
fn part_columns() -> String {
    (1..=16)
        .map(|n| format!("part{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolves an index's columns to names in key order (descending parts use the
/// absolute column number).
fn index_part_cols(
    conn: &Connection,
    tabid: i64,
    idxname: &str,
    names: &HashMap<i64, String>,
) -> Result<Vec<String>, PluginError> {
    // `sysindexes.idxname` is a space-padded CHAR column, so it must be TRIMmed
    // before comparing against the (already trimmed) constraint index name.
    let sql = format!(
        "SELECT {parts} FROM sysindexes WHERE tabid = {tabid} AND TRIM(idxname) = {idx}",
        parts = part_columns(),
        idx = quote_string(idxname)
    );
    let out = client::run_select(conn, &sql)?;
    let mut cols = Vec::new();
    if let Some(row) = out.rows.first() {
        for i in 0..16 {
            let p = cint(row, i);
            if p != 0 {
                if let Some(name) = names.get(&p.abs()) {
                    cols.push(name.clone());
                }
            }
        }
    }
    Ok(cols)
}

/// Default expressions per column number.
fn defaults_of(conn: &Connection, tabid: i64) -> HashMap<i64, String> {
    let sql = format!(
        "SELECT colno, TRIM(type), NVL(default, '') FROM sysdefaults WHERE tabid = {tabid}"
    );
    let mut map = HashMap::new();
    if let Ok(out) = client::run_select(conn, &sql) {
        for r in &out.rows {
            if let Some(def) = decode_default(&cstr(r, 1), &cstr(r, 2)) {
                map.insert(cint(r, 0), def);
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Composite extractors (return host-shaped JSON)
// ---------------------------------------------------------------------------

/// Columns of a table/view as `TableColumn` JSON objects.
fn columns_of(conn: &Connection, name: &str) -> Result<Vec<Value>, PluginError> {
    let Some(tabid) = tabid_of(conn, name)? else {
        return Ok(vec![]);
    };
    let pk = pk_colnos(conn, tabid)?;
    let defaults = defaults_of(conn, tabid);

    let sql = format!(
        "SELECT TRIM(c.colname), c.coltype, c.collength, c.colno, NVL(TRIM(x.name), '') \
         FROM syscolumns c \
         LEFT OUTER JOIN sysxtdtypes x ON x.extended_id = c.extended_id \
         WHERE c.tabid = {tabid} ORDER BY c.colno"
    );
    let out = client::run_select(conn, &sql)?;

    let mut cols = Vec::with_capacity(out.rows.len());
    for r in &out.rows {
        let colname = cstr(r, 0);
        let coltype = cint(r, 1);
        let collength = cint(r, 2);
        let colno = cint(r, 3);
        let ext = cstr(r, 4);
        let ext_opt = if ext.is_empty() { None } else { Some(ext.as_str()) };
        let decoded = decode_type(coltype, collength, ext_opt);

        let mut col = json!({
            "name": colname,
            "data_type": decoded.sql_type,
            "is_pk": pk.contains(&colno),
            "is_nullable": !decoded.is_not_null,
            "is_auto_increment": decoded.is_auto_increment,
        });
        if let Some(len) = decoded.char_max_len {
            col["character_maximum_length"] = json!(len);
        }
        if let Some(def) = defaults.get(&colno) {
            col["default_value"] = json!(def);
        }
        cols.push(col);
    }
    Ok(cols)
}

/// Foreign keys of a table as `ForeignKey` JSON objects.
fn foreign_keys_of(conn: &Connection, name: &str) -> Result<Vec<Value>, PluginError> {
    let Some(tabid) = tabid_of(conn, name)? else {
        return Ok(vec![]);
    };

    let sql = format!(
        "SELECT TRIM(con.constrname), TRIM(con.idxname), r.\"primary\", NVL(TRIM(r.delrule), '') \
         FROM sysconstraints con \
         JOIN sysreferences r ON r.constrid = con.constrid \
         WHERE con.tabid = {tabid} AND con.constrtype = 'R'"
    );
    let out = client::run_select(conn, &sql)?;

    let child_names = colno_names(conn, tabid)?;
    let mut fks = Vec::new();

    for r in &out.rows {
        let constr_name = cstr(r, 0);
        let child_idx = cstr(r, 1);
        let primary_constrid = cint(r, 2);
        let delrule = cstr(r, 3);
        let on_delete = match delrule.trim() {
            "C" => Some("CASCADE"),
            _ => None,
        };

        let child_cols = index_part_cols(conn, tabid, &child_idx, &child_names)?;

        // Resolve the referenced (primary) constraint: table + index columns.
        let psql = format!(
            "SELECT TRIM(pt.tabname), TRIM(pc.idxname), pt.tabid \
             FROM sysconstraints pc JOIN systables pt ON pt.tabid = pc.tabid \
             WHERE pc.constrid = {primary_constrid}"
        );
        let pout = client::run_select(conn, &psql)?;
        let Some(prow) = pout.rows.first() else {
            continue;
        };
        let ref_table = cstr(prow, 0);
        let parent_idx = cstr(prow, 1);
        let parent_tabid = cint(prow, 2);
        let parent_names = colno_names(conn, parent_tabid)?;
        let parent_cols = index_part_cols(conn, parent_tabid, &parent_idx, &parent_names)?;

        for (i, col) in child_cols.iter().enumerate() {
            let ref_column = parent_cols.get(i).cloned().unwrap_or_default();
            fks.push(json!({
                "name": constr_name,
                "column_name": col,
                "ref_table": ref_table,
                "ref_column": ref_column,
                "on_delete": on_delete,
                "on_update": Value::Null,
            }));
        }
    }
    Ok(fks)
}

/// Indexes of a table as `Index` JSON objects (one row per column).
fn indexes_of(conn: &Connection, name: &str) -> Result<Vec<Value>, PluginError> {
    let Some(tabid) = tabid_of(conn, name)? else {
        return Ok(vec![]);
    };
    let names = colno_names(conn, tabid)?;

    // Index names backing a PRIMARY KEY constraint.
    let pk_sql = format!(
        "SELECT TRIM(idxname) FROM sysconstraints WHERE tabid = {tabid} AND constrtype = 'P'"
    );
    let pk_idx: HashSet<String> = client::run_select(conn, &pk_sql)?
        .rows
        .iter()
        .map(|r| cstr(r, 0))
        .collect();

    let sql = format!(
        "SELECT TRIM(idxname), TRIM(idxtype), {parts} FROM sysindexes WHERE tabid = {tabid} \
         ORDER BY idxname",
        parts = part_columns()
    );
    let out = client::run_select(conn, &sql)?;

    let mut indexes = Vec::new();
    for r in &out.rows {
        let idxname = cstr(r, 0);
        let idxtype = cstr(r, 1);
        let is_unique = idxtype.trim() == "U";
        let is_primary = pk_idx.contains(&idxname);
        let mut seq = 0i32;
        for i in 0..16 {
            let p = cint(r, 2 + i); // parts start at column index 2
            if p != 0 {
                seq += 1;
                if let Some(col) = names.get(&p.abs()) {
                    indexes.push(json!({
                        "name": idxname,
                        "column_name": col,
                        "is_unique": is_unique,
                        "is_primary": is_primary,
                        "seq_in_index": seq,
                    }));
                }
            }
        }
    }
    Ok(indexes)
}

// ---------------------------------------------------------------------------
// RPC handlers
// ---------------------------------------------------------------------------

pub fn get_databases(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    // sysmaster holds the server-wide database list; fall back to the current
    // database if it is not readable.
    match client::run_select(
        &conn,
        "SELECT TRIM(name) FROM sysmaster:sysdatabases ORDER BY name",
    ) {
        Ok(out) if !out.rows.is_empty() => {
            Ok(json!(out.rows.iter().map(|r| cstr(r, 0)).collect::<Vec<_>>()))
        }
        _ => {
            // Fall back to the database from the connection parameters.
            let (db, _) = connection_params(top).database_and_server();
            Ok(json!(db.into_iter().collect::<Vec<_>>()))
        }
    }
}

pub fn get_tables(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let names = list_objects(&conn, "T")?;
    Ok(json!(names
        .into_iter()
        .map(|name| json!({ "name": name }))
        .collect::<Vec<_>>()))
}

pub fn get_columns(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let conn = connect_from(top)?;
    Ok(json!(columns_of(&conn, &table)?))
}

pub fn get_foreign_keys(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let conn = connect_from(top)?;
    Ok(json!(foreign_keys_of(&conn, &table)?))
}

pub fn get_indexes(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let conn = connect_from(top)?;
    Ok(json!(indexes_of(&conn, &table)?))
}

pub fn get_views(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let names = list_objects(&conn, "V")?;
    Ok(json!(names
        .into_iter()
        .map(|name| json!({ "name": name, "definition": Value::Null }))
        .collect::<Vec<_>>()))
}

pub fn get_view_definition(top: &Value) -> Result<Value, PluginError> {
    let view = require_str(top, "view_name")?.to_string();
    let conn = connect_from(top)?;
    let sql = format!(
        "SELECT v.viewtext FROM sysviews v JOIN systables t ON t.tabid = v.tabid \
         WHERE t.tabname = {} ORDER BY v.seqno",
        quote_string(&view)
    );
    let out = client::run_select(&conn, &sql)?;
    let def: String = out.rows.iter().map(|r| cstr(r, 0)).collect();
    Ok(json!(def))
}

pub fn get_view_columns(top: &Value) -> Result<Value, PluginError> {
    let view = require_str(top, "view_name")?.to_string();
    let conn = connect_from(top)?;
    Ok(json!(columns_of(&conn, &view)?))
}

pub fn get_routines(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let sql = "SELECT TRIM(procname), TRIM(isproc) FROM sysprocedures \
               WHERE TRIM(owner) <> 'informix' ORDER BY procname";
    let out = client::run_select(&conn, sql)?;
    let routines = out
        .rows
        .iter()
        .map(|r| {
            let routine_type = if cstr(r, 1).trim() == "t" {
                "PROCEDURE"
            } else {
                "FUNCTION"
            };
            json!({ "name": cstr(r, 0), "routine_type": routine_type, "definition": Value::Null })
        })
        .collect::<Vec<_>>();
    Ok(json!(routines))
}

pub fn get_routine_definition(top: &Value) -> Result<Value, PluginError> {
    let routine = require_str(top, "routine_name")?.to_string();
    let conn = connect_from(top)?;
    let sql = format!(
        "SELECT b.data FROM sysprocbody b JOIN sysprocedures p ON p.procid = b.procid \
         WHERE p.procname = {} AND b.datakey = 'T' ORDER BY b.seqno",
        quote_string(&routine)
    );
    let out = client::run_select(&conn, &sql)?;
    let def: String = out.rows.iter().map(|r| cstr(r, 0)).collect();
    Ok(json!(def))
}

pub fn get_schema_snapshot(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let tables = list_objects(&conn, "T")?;
    let mut schemas = Vec::with_capacity(tables.len());
    for name in tables {
        let columns = columns_of(&conn, &name)?;
        let foreign_keys = foreign_keys_of(&conn, &name)?;
        schemas.push(json!({
            "name": name,
            "columns": columns,
            "foreign_keys": foreign_keys,
        }));
    }
    Ok(json!(schemas))
}

pub fn get_all_columns_batch(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let tables = list_objects(&conn, "T")?;
    let mut map = serde_json::Map::new();
    for name in tables {
        map.insert(name.clone(), json!(columns_of(&conn, &name)?));
    }
    Ok(Value::Object(map))
}

pub fn get_all_foreign_keys_batch(top: &Value) -> Result<Value, PluginError> {
    let conn = connect_from(top)?;
    let tables = list_objects(&conn, "T")?;
    let mut map = serde_json::Map::new();
    for name in tables {
        map.insert(name.clone(), json!(foreign_keys_of(&conn, &name)?));
    }
    Ok(Value::Object(map))
}
