//! DDL generation and mutation for Informix.
//!
//! The SQL-generation methods (`get_*_sql`) are pure: the host does not send
//! connection parameters, only the structural definitions, and expects back a
//! `Vec<String>` of statements. `drop_index` / `drop_foreign_key` do receive
//! params and execute against the database.

use serde_json::{json, Value};

use crate::client;
use crate::error::PluginError;
use crate::handlers::{connect_from, require_str};
use crate::utils::identifiers::quote_identifier;

/// A column definition as sent by the host (`ColumnDefinition`).
#[derive(Debug, Clone)]
struct ColumnDef {
    name: String,
    data_type: String,
    is_nullable: bool,
    is_pk: bool,
    is_auto_increment: bool,
    default_value: Option<String>,
}

impl ColumnDef {
    fn from_value(v: &Value) -> Result<Self, PluginError> {
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| PluginError::invalid_params("column is missing 'name'"))?
            .to_string();
        let data_type = v
            .get("data_type")
            .and_then(Value::as_str)
            .ok_or_else(|| PluginError::invalid_params("column is missing 'data_type'"))?
            .to_string();
        Ok(Self {
            name,
            data_type,
            is_nullable: v
                .get("is_nullable")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            is_pk: v.get("is_pk").and_then(Value::as_bool).unwrap_or(false),
            is_auto_increment: v
                .get("is_auto_increment")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            default_value: v
                .get("default_value")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        })
    }
}

// ---------------------------------------------------------------------------
// Pure SQL builders
// ---------------------------------------------------------------------------

/// Picks the effective Informix type, substituting a SERIAL family type for
/// auto-increment columns.
fn effective_type(col: &ColumnDef) -> String {
    if !col.is_auto_increment {
        return col.data_type.clone();
    }
    let up = col.data_type.to_uppercase();
    if up.contains("SERIAL") {
        col.data_type.clone()
    } else if up.contains("INT8") {
        "SERIAL8".to_string()
    } else if up.contains("BIGINT") {
        "BIGSERIAL".to_string()
    } else {
        "SERIAL".to_string()
    }
}

/// `"name" TYPE [DEFAULT d] [NOT NULL]` — Informix places DEFAULT before the
/// nullability constraint, and SERIAL columns are implicitly NOT NULL.
fn column_clause(col: &ColumnDef) -> String {
    let mut s = format!("{} {}", quote_identifier(&col.name), effective_type(col));
    if let Some(def) = &col.default_value {
        s.push_str(&format!(" DEFAULT {def}"));
    }
    if !col.is_nullable && !col.is_auto_increment {
        s.push_str(" NOT NULL");
    }
    s
}

fn build_create_table(table: &str, columns: &[ColumnDef]) -> String {
    let mut lines: Vec<String> = columns.iter().map(column_clause).collect();
    let pk: Vec<String> = columns
        .iter()
        .filter(|c| c.is_pk)
        .map(|c| quote_identifier(&c.name))
        .collect();
    if !pk.is_empty() {
        lines.push(format!("PRIMARY KEY ({})", pk.join(", ")));
    }
    format!(
        "CREATE TABLE {} (\n  {}\n)",
        quote_identifier(table),
        lines.join(",\n  ")
    )
}

fn build_add_column(table: &str, col: &ColumnDef) -> String {
    format!(
        "ALTER TABLE {} ADD {}",
        quote_identifier(table),
        column_clause(col)
    )
}

fn build_alter_column(table: &str, old: &ColumnDef, new: &ColumnDef) -> Vec<String> {
    let mut stmts = Vec::new();
    if old.name != new.name {
        stmts.push(format!(
            "RENAME COLUMN {}.{} TO {}",
            quote_identifier(table),
            quote_identifier(&old.name),
            quote_identifier(&new.name)
        ));
    }
    stmts.push(format!(
        "ALTER TABLE {} MODIFY ({})",
        quote_identifier(table),
        column_clause(new)
    ));
    stmts
}

fn build_create_index(table: &str, index: &str, columns: &[String], unique: bool) -> String {
    let cols: Vec<String> = columns.iter().map(|c| quote_identifier(c)).collect();
    format!(
        "CREATE {}INDEX {} ON {} ({})",
        if unique { "UNIQUE " } else { "" },
        quote_identifier(index),
        quote_identifier(table),
        cols.join(", ")
    )
}

fn build_create_foreign_key(
    table: &str,
    fk_name: &str,
    column: &str,
    ref_table: &str,
    ref_column: &str,
    on_delete: Option<&str>,
) -> String {
    let mut sql = format!(
        "ALTER TABLE {} ADD CONSTRAINT FOREIGN KEY ({}) REFERENCES {} ({})",
        quote_identifier(table),
        quote_identifier(column),
        quote_identifier(ref_table),
        quote_identifier(ref_column)
    );
    // Informix only supports ON DELETE CASCADE (no ON UPDATE actions).
    if matches!(on_delete, Some(a) if a.eq_ignore_ascii_case("CASCADE")) {
        sql.push_str(" ON DELETE CASCADE");
    }
    sql.push_str(&format!(" CONSTRAINT {}", quote_identifier(fk_name)));
    sql
}

// ---------------------------------------------------------------------------
// RPC handlers
// ---------------------------------------------------------------------------

fn columns_from(top: &Value, key: &str) -> Result<Vec<ColumnDef>, PluginError> {
    top.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| PluginError::invalid_params(format!("missing '{key}' array")))?
        .iter()
        .map(ColumnDef::from_value)
        .collect()
}

pub fn get_create_table_sql(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table_name")?;
    let columns = columns_from(top, "columns")?;
    Ok(json!([build_create_table(table, &columns)]))
}

pub fn get_add_column_sql(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?;
    let col = ColumnDef::from_value(
        top.get("column")
            .ok_or_else(|| PluginError::invalid_params("missing 'column'"))?,
    )?;
    Ok(json!([build_add_column(table, &col)]))
}

pub fn get_alter_column_sql(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?;
    let old = ColumnDef::from_value(
        top.get("old_column")
            .ok_or_else(|| PluginError::invalid_params("missing 'old_column'"))?,
    )?;
    let new = ColumnDef::from_value(
        top.get("new_column")
            .ok_or_else(|| PluginError::invalid_params("missing 'new_column'"))?,
    )?;
    Ok(json!(build_alter_column(table, &old, &new)))
}

pub fn get_create_index_sql(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?;
    let index = require_str(top, "index_name")?;
    let unique = top
        .get("is_unique")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let columns: Vec<String> = top
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!([build_create_index(table, index, &columns, unique)]))
}

pub fn get_create_foreign_key_sql(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?;
    let fk_name = require_str(top, "fk_name")?;
    let column = require_str(top, "column")?;
    let ref_table = require_str(top, "ref_table")?;
    let ref_column = require_str(top, "ref_column")?;
    let on_delete = top.get("on_delete").and_then(Value::as_str);
    Ok(json!([build_create_foreign_key(
        table, fk_name, column, ref_table, ref_column, on_delete
    )]))
}

pub fn drop_index(top: &Value) -> Result<Value, PluginError> {
    let index = require_str(top, "index_name")?.to_string();
    let conn = connect_from(top)?;
    client::run_exec(&conn, &format!("DROP INDEX {}", quote_identifier(&index)))?;
    Ok(Value::Null)
}

pub fn drop_foreign_key(top: &Value) -> Result<Value, PluginError> {
    let table = require_str(top, "table")?.to_string();
    let fk_name = require_str(top, "fk_name")?.to_string();
    let conn = connect_from(top)?;
    client::run_exec(
        &conn,
        &format!(
            "ALTER TABLE {} DROP CONSTRAINT {}",
            quote_identifier(&table),
            quote_identifier(&fk_name)
        ),
    )?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            data_type: ty.to_string(),
            is_nullable: true,
            is_pk: false,
            is_auto_increment: false,
            default_value: None,
        }
    }

    #[test]
    fn create_table_with_pk_and_serial() {
        let mut id = col("id", "INTEGER");
        id.is_auto_increment = true;
        id.is_pk = true;
        id.is_nullable = false;
        let mut name = col("name", "VARCHAR(40)");
        name.is_nullable = false;
        let sql = build_create_table("customer", &[id, name]);
        assert!(sql.contains("CREATE TABLE \"customer\" ("), "{sql}");
        assert!(sql.contains("\"id\" SERIAL"), "{sql}");
        assert!(sql.contains("\"name\" VARCHAR(40) NOT NULL"), "{sql}");
        assert!(sql.contains("PRIMARY KEY (\"id\")"), "{sql}");
    }

    #[test]
    fn serial_substitution_by_size() {
        let mut big = col("id", "BIGINT");
        big.is_auto_increment = true;
        assert_eq!(effective_type(&big), "BIGSERIAL");
        let mut i8 = col("id", "INT8");
        i8.is_auto_increment = true;
        assert_eq!(effective_type(&i8), "SERIAL8");
    }

    #[test]
    fn add_column_with_default() {
        let mut c = col("status", "VARCHAR(10)");
        c.default_value = Some("'new'".to_string());
        c.is_nullable = false;
        let sql = build_add_column("orders", &c);
        assert_eq!(
            sql,
            "ALTER TABLE \"orders\" ADD \"status\" VARCHAR(10) DEFAULT 'new' NOT NULL"
        );
    }

    #[test]
    fn alter_column_rename_and_modify() {
        let old = col("qty", "INTEGER");
        let new = col("quantity", "BIGINT");
        let stmts = build_alter_column("orders", &old, &new);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "RENAME COLUMN \"orders\".\"qty\" TO \"quantity\"");
        assert_eq!(
            stmts[1],
            "ALTER TABLE \"orders\" MODIFY (\"quantity\" BIGINT)"
        );
    }

    #[test]
    fn create_unique_index() {
        let sql = build_create_index("t", "idx_email", &["email".to_string()], true);
        assert_eq!(
            sql,
            "CREATE UNIQUE INDEX \"idx_email\" ON \"t\" (\"email\")"
        );
    }

    #[test]
    fn foreign_key_with_cascade() {
        let sql = build_create_foreign_key(
            "orders",
            "fk_cust",
            "customer_id",
            "customer",
            "id",
            Some("CASCADE"),
        );
        assert_eq!(
            sql,
            "ALTER TABLE \"orders\" ADD CONSTRAINT FOREIGN KEY (\"customer_id\") \
             REFERENCES \"customer\" (\"id\") ON DELETE CASCADE CONSTRAINT \"fk_cust\""
        );
    }

    #[test]
    fn foreign_key_without_action() {
        let sql = build_create_foreign_key("o", "fk", "c_id", "c", "id", None);
        assert!(!sql.contains("ON DELETE"), "{sql}");
        assert!(sql.ends_with("CONSTRAINT \"fk\""), "{sql}");
    }
}
