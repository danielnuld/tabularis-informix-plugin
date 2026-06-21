//! Builds the ODBC connection string for the IBM Informix ODBC driver.
//!
//! Two shapes are supported:
//!   * **DSN-based** — when `Config::dsn` is set, the string uses `DSN=<name>`
//!     and only overrides credentials / database / locales.
//!   * **DSN-less** — otherwise the full string is assembled from the driver
//!     name, host, server, service (port) and protocol.
//!
//! `DELIMIDENT=Y` is always appended so double-quoted identifiers work.

use crate::config::Config;
use crate::error::PluginError;
use crate::models::ConnectionParams;

/// Wraps a value in `{...}` when it contains characters that would otherwise
/// break the `key=value;` connection-string grammar.
fn attr_value(v: &str) -> String {
    if v.contains([';', '{', '}', '=']) {
        format!("{{{v}}}")
    } else {
        v.to_string()
    }
}

fn push_attr(out: &mut String, key: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    out.push_str(key);
    out.push('=');
    out.push_str(&attr_value(value));
    out.push(';');
}

/// Like `push_attr` but always brace-wraps the value. The Informix `DRIVER`
/// attribute must be brace-wrapped even though the name contains only spaces.
fn push_attr_braced(out: &mut String, key: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    out.push_str(key);
    out.push_str("={");
    out.push_str(value);
    out.push_str("};");
}

/// Builds the full ODBC connection string. Returns an error when required
/// pieces (database, and for DSN-less mode the Informix server) are missing.
pub fn build_connection_string(
    params: &ConnectionParams,
    cfg: &Config,
) -> Result<String, PluginError> {
    let (database, server_from_db) = params.database_and_server();
    let (host, server_from_host) = params.host_and_server();
    // When no database is selected yet (e.g. while listing databases in the
    // connection modal), connect to `sysmaster`, which exists on every Informix
    // instance and exposes the server-wide database catalog.
    let database = database.unwrap_or_else(|| "sysmaster".to_string());

    let mut out = String::new();

    if !cfg.dsn.is_empty() {
        push_attr(&mut out, "DSN", &cfg.dsn);
    } else {
        push_attr_braced(&mut out, "DRIVER", &cfg.driver_name);

        if let Some(host) = host.as_deref() {
            push_attr(&mut out, "HOST", host);
        }
        if let Some(port) = params.port {
            push_attr(&mut out, "SERVICE", &port.to_string());
        }
        // Server precedence: dbname@server, then host@server, then the global
        // plugin setting.
        let server = server_from_db
            .or(server_from_host)
            .or_else(|| {
                if cfg.informixserver.is_empty() {
                    None
                } else {
                    Some(cfg.informixserver.clone())
                }
            })
            .ok_or_else(|| {
                PluginError::invalid_params(
                    "Informix server name (dbservername) is required: type the Host as \
                     IP@dbservername, or set 'informixserver' in the plugin settings",
                )
            })?;
        push_attr(&mut out, "SERVER", &server);
        push_attr(&mut out, "PROTOCOL", &cfg.protocol);
    }

    push_attr(&mut out, "DATABASE", &database);
    if let Some(uid) = params.username.as_deref() {
        push_attr(&mut out, "UID", uid);
    }
    if let Some(pwd) = params.password.as_deref() {
        push_attr(&mut out, "PWD", pwd);
    }
    push_attr(&mut out, "DB_LOCALE", &cfg.db_locale);
    push_attr(&mut out, "CLIENT_LOCALE", &cfg.client_locale);
    push_attr(&mut out, "DELIMIDENT", "Y");
    if !cfg.extra.is_empty() {
        out.push_str(cfg.extra.trim());
        if !cfg.extra.trim_end().ends_with(';') {
            out.push(';');
        }
    }

    Ok(out)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // clearer than struct-init in these tests
mod tests {
    use super::*;

    fn params(db: &str) -> ConnectionParams {
        ConnectionParams {
            host: Some("dbhost".to_string()),
            port: Some(9088),
            database: Some(db.to_string()),
            username: Some("informix".to_string()),
            password: Some("secret".to_string()),
            ssl_mode: None,
        }
    }

    #[test]
    fn dsnless_with_server_from_settings() {
        let mut cfg = Config::default();
        cfg.informixserver = "ol_ids".to_string();
        let s = build_connection_string(&params("stores"), &cfg).unwrap();
        assert!(s.contains("DRIVER={IBM INFORMIX ODBC DRIVER};"), "got: {s}");
        assert!(s.contains("HOST=dbhost;"));
        assert!(s.contains("SERVICE=9088;"));
        assert!(s.contains("SERVER=ol_ids;"));
        assert!(s.contains("PROTOCOL=onsoctcp;"));
        assert!(s.contains("DATABASE=stores;"));
        assert!(s.contains("UID=informix;"));
        assert!(s.contains("PWD=secret;"));
        assert!(s.contains("DELIMIDENT=Y;"));
    }

    #[test]
    fn server_from_database_field_overrides_settings() {
        let mut cfg = Config::default();
        cfg.informixserver = "ignored".to_string();
        let s = build_connection_string(&params("stores@ol_real"), &cfg).unwrap();
        assert!(s.contains("SERVER=ol_real;"), "got: {s}");
        assert!(s.contains("DATABASE=stores;"), "got: {s}");
    }

    #[test]
    fn server_from_host_field() {
        let cfg = Config::default(); // no global informixserver
        let mut p = params("stores");
        p.host = Some("192.0.2.10@ol_ids".to_string());
        let s = build_connection_string(&p, &cfg).unwrap();
        assert!(s.contains("HOST=192.0.2.10;"), "got: {s}");
        assert!(s.contains("SERVER=ol_ids;"), "got: {s}");
    }

    #[test]
    fn dsn_mode_skips_driver_and_server() {
        let mut cfg = Config::default();
        cfg.dsn = "MyInformixDSN".to_string();
        let s = build_connection_string(&params("stores"), &cfg).unwrap();
        assert!(s.contains("DSN=MyInformixDSN;"), "got: {s}");
        assert!(!s.contains("DRIVER="), "got: {s}");
        assert!(s.contains("DATABASE=stores;"));
        assert!(s.contains("DELIMIDENT=Y;"));
    }

    #[test]
    fn missing_server_is_an_error() {
        let cfg = Config::default(); // no informixserver, no dsn
        let err = build_connection_string(&params("stores"), &cfg).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn missing_database_defaults_to_sysmaster() {
        let mut cfg = Config::default();
        cfg.informixserver = "ol_ids".to_string();
        let mut p = params("stores");
        p.database = None;
        let s = build_connection_string(&p, &cfg).unwrap();
        assert!(s.contains("DATABASE=sysmaster;"), "got: {s}");
    }
}
