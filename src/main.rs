//! Tabularis driver plugin for IBM Informix (11.70+).
//!
//! Speaks JSON-RPC 2.0 over stdin/stdout, one request per line, connecting to
//! Informix through the IBM Informix ODBC driver (requires the Informix Client
//! SDK to be installed on the host).
//!
//! Release builds use the Windows "windows" subsystem so that no console window
//! is allocated when Tabularis (a GUI app) spawns the plugin. stdin/stdout are
//! still wired through the pipes Tabularis sets up, so JSON-RPC is unaffected.
//! Debug builds keep the console subsystem for easier CLI testing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{self, BufRead, Write};

mod client;
mod config;
mod dllpath;
mod error;
mod handlers;
mod models;
mod rpc;
mod utils;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = rpc::handle_line(trimmed);
        let mut body = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(err) => format!(
                "{{\"jsonrpc\":\"2.0\",\"error\":{{\"code\":-32603,\"message\":\"serialization failed: {err}\"}},\"id\":null}}"
            ),
        };
        body.push('\n');
        if out.write_all(body.as_bytes()).is_err() {
            break;
        }
        let _ = out.flush();
    }
}
