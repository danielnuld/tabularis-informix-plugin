//! Request shapes the host sends to the plugin.
//!
//! The host serialises `ConnectionParams` with the `database` field as an
//! untagged `DatabaseSelection` enum, so on the wire it is *either* a plain
//! string (`"mydb"`) *or* an array of strings (`["a", "b"]`). We accept both.

use serde_json::Value;

/// Connection parameters as entered in the Tabularis connection form.
#[derive(Debug, Clone, Default)]
pub struct ConnectionParams {
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Raw database value. May embed the Informix server as `dbname@server`.
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Reserved for a future SSL mode (capabilities.supports_ssl is off for now).
    #[allow(dead_code)]
    pub ssl_mode: Option<String>,
}

impl ConnectionParams {
    pub fn from_value(value: &Value) -> Self {
        let obj = value.as_object();
        let get_str = |k: &str| {
            obj.and_then(|o| o.get(k))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };
        let port = obj
            .and_then(|o| o.get("port"))
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok());

        let database = obj
            .and_then(|o| o.get("database"))
            .and_then(database_as_str);

        Self {
            host: get_str("host"),
            port,
            database,
            username: get_str("username"),
            password: get_str("password"),
            ssl_mode: get_str("ssl_mode"),
        }
    }

    /// Splits the `host` field into `(host, Some(server))` when the
    /// `host@server` form is used, otherwise `(host, None)`. This lets each
    /// connection carry its own Informix server (dbservername) even in
    /// multi-database mode, where the database field is not shown.
    pub fn host_and_server(&self) -> (Option<String>, Option<String>) {
        split_on_at(self.host.as_deref())
    }

    /// Splits the `database` field into `(database, Some(server))` when the
    /// Informix `dbname@server` form is used, otherwise `(database, None)`.
    pub fn database_and_server(&self) -> (Option<String>, Option<String>) {
        split_on_at(self.database.as_deref())
    }
}

/// Splits `"left@right"` into `(Some(left), Some(right))`, trimming each part
/// and mapping empty parts to `None`. A value without `@` yields `(value, None)`.
fn split_on_at(value: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = value else {
        return (None, None);
    };
    let non_empty = |s: &str| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    match raw.split_once('@') {
        Some((left, right)) => (non_empty(left), non_empty(right)),
        None => (non_empty(raw), None),
    }
}

/// Accepts the untagged `DatabaseSelection`: a bare string or `[strings]`.
fn database_as_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(arr) => arr
            .first()
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// The host wraps connection params in `params.params`. Extracts the inner
/// `ConnectionParams` object for the common case.
pub fn connection_params(top: &Value) -> ConnectionParams {
    ConnectionParams::from_value(top.get("params").unwrap_or(&Value::Null))
}

/// Reads a required string field from the top-level params object.
pub fn str_field<'a>(top: &'a Value, key: &str) -> Option<&'a str> {
    top.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_database_as_plain_string() {
        let p = ConnectionParams::from_value(&json!({ "database": "stores" }));
        assert_eq!(p.database.as_deref(), Some("stores"));
    }

    #[test]
    fn parses_database_as_array_first_element() {
        let p = ConnectionParams::from_value(&json!({ "database": ["stores", "other"] }));
        assert_eq!(p.database.as_deref(), Some("stores"));
    }

    #[test]
    fn splits_database_and_server() {
        let p = ConnectionParams::from_value(&json!({ "database": "stores@ol_ids" }));
        let (db, server) = p.database_and_server();
        assert_eq!(db.as_deref(), Some("stores"));
        assert_eq!(server.as_deref(), Some("ol_ids"));
    }

    #[test]
    fn database_without_server() {
        let p = ConnectionParams::from_value(&json!({ "database": "stores" }));
        let (db, server) = p.database_and_server();
        assert_eq!(db.as_deref(), Some("stores"));
        assert_eq!(server, None);
    }

    #[test]
    fn empty_strings_become_none() {
        let p = ConnectionParams::from_value(&json!({ "host": "", "username": "ix" }));
        assert_eq!(p.host, None);
        assert_eq!(p.username.as_deref(), Some("ix"));
    }
}
