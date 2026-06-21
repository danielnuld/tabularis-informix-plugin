pub mod crud;
pub mod ddl;
pub mod metadata;
pub mod query;

use serde_json::Value;

use crate::client;
use crate::error::PluginError;
use crate::models::{connection_params, str_field};
use odbc_api::Connection;

/// Opens a connection from the top-level RPC params (`params.params`).
pub fn connect_from(top: &Value) -> Result<Connection<'static>, PluginError> {
    client::connect(&connection_params(top))
}

/// Reads a required string field from the params, erroring if absent.
pub fn require_str<'a>(top: &'a Value, key: &str) -> Result<&'a str, PluginError> {
    str_field(top, key)
        .ok_or_else(|| PluginError::invalid_params(format!("missing required field '{key}'")))
}
