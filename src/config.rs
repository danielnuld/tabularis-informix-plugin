//! Plugin-wide settings, populated by the `initialize` RPC call.
//!
//! These map to the `settings` array declared in `manifest.json`. They are
//! global to the plugin process (the host persists them in its `config.json`),
//! so per-connection values such as the Informix server name should come from
//! the connection form (see `ConnectionParams::database_and_server`).

use std::sync::{OnceLock, RwLock};

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Config {
    /// ODBC driver name registered on the machine.
    pub driver_name: String,
    /// Default Informix server (INFORMIXSERVER / dbservername) when the
    /// connection's database field does not use the `dbname@server` form.
    pub informixserver: String,
    /// Network protocol: usually `onsoctcp` (or `onsocssl` for TLS).
    pub protocol: String,
    /// Optional ODBC DSN name. When set, the connection string uses `DSN=`
    /// instead of building a DSN-less string from host/server/protocol.
    pub dsn: String,
    /// Optional DB_LOCALE (e.g. `en_US.819`).
    pub db_locale: String,
    /// Optional CLIENT_LOCALE (e.g. `en_US.819`).
    pub client_locale: String,
    /// Free-form extra connection-string attributes, appended verbatim.
    pub extra: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            driver_name: "IBM INFORMIX ODBC DRIVER".to_string(),
            informixserver: String::new(),
            protocol: "onsoctcp".to_string(),
            dsn: String::new(),
            db_locale: String::new(),
            client_locale: String::new(),
            extra: String::new(),
        }
    }
}

impl Config {
    /// Builds a `Config` from the `settings` object of an `initialize` call,
    /// falling back to defaults for missing keys.
    pub fn from_settings(settings: &Value) -> Self {
        let mut cfg = Config::default();
        let get = |k: &str| settings.get(k).and_then(Value::as_str).map(str::to_string);
        if let Some(v) = get("driver_name").filter(|s| !s.is_empty()) {
            cfg.driver_name = v;
        }
        if let Some(v) = get("informixserver") {
            cfg.informixserver = v;
        }
        if let Some(v) = get("protocol").filter(|s| !s.is_empty()) {
            cfg.protocol = v;
        }
        if let Some(v) = get("dsn") {
            cfg.dsn = v;
        }
        if let Some(v) = get("db_locale") {
            cfg.db_locale = v;
        }
        if let Some(v) = get("client_locale") {
            cfg.client_locale = v;
        }
        if let Some(v) = get("extra") {
            cfg.extra = v;
        }
        cfg
    }
}

fn store() -> &'static RwLock<Config> {
    static STORE: OnceLock<RwLock<Config>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(Config::default()))
}

/// Replaces the global config (called on `initialize`).
pub fn set(cfg: Config) {
    if let Ok(mut guard) = store().write() {
        *guard = cfg;
    }
}

/// Returns a clone of the current global config.
pub fn get() -> Config {
    store()
        .read()
        .map(|g| g.clone())
        .unwrap_or_default()
}
