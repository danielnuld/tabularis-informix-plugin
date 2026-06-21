# Tabularis Informix Plugin

A [Tabularis](https://github.com/TabularisDB/tabularis) database driver plugin for
**IBM Informix 11.70 and newer**, implemented in Rust. It speaks Tabularis'
JSON-RPC-over-stdio protocol and connects to Informix through the **IBM Informix
ODBC driver**.

## Requirements

- **IBM Informix Client SDK (CSDK)** installed on the machine running Tabularis.
  It provides the ODBC driver (`IBM INFORMIX ODBC DRIVER`, `iclit09b.dll`) and
  the network libraries. Without it the plugin loads but every connection fails
  with ODBC error `IM002` (data source / driver not found).
- A Rust toolchain (`stable`, MSVC on Windows) to build from source.

### ⚠️ Bitness must match the Informix ODBC driver

An ODBC application can only load a driver of its **own bitness**. The Informix
ODBC driver shipped in `C:\Program Files (x86)\IBM Informix Client SDK` is
**32-bit**, so the plugin must be built 32-bit too. Tabularis (64-bit) talks to
the plugin over stdio, so the plugin's bitness is independent of the host.

This repo pins the 32-bit target in `.cargo/config.toml`. A 64-bit build against
a 32-bit driver fails with `IM002` even though the driver is installed.

To check your driver's bitness: it is registered under
`HKLM\SOFTWARE\WOW6432Node\ODBC\ODBCINST.INI` (32-bit) vs
`HKLM\SOFTWARE\ODBC\ODBCINST.INI` (64-bit).

## Building

```sh
cargo test                                          # unit tests (no database needed)

# Build matching your Informix ODBC driver's bitness:
rustup target add i686-pc-windows-msvc              # once, for 32-bit driver
cargo build --release --target i686-pc-windows-msvc # 32-bit (typical Informix CSDK)
# …or for a 64-bit Informix ODBC driver:
cargo build --release --target x86_64-pc-windows-msvc
```

The binary lands in `target/<target>/release/tabularis-informix-plugin(.exe)`.

> **Tip:** to avoid passing `--target` every time, create a local
> `.cargo/config.toml` with `[build]\ntarget = "i686-pc-windows-msvc"`
> (this file is git-ignored so it stays a per-machine choice).

The unit tests cover the pure logic: identifier quoting, `coltype`/`collength`
decoding, SQL-literal formatting, `SKIP`/`FIRST` pagination, connection-string
assembly, and DDL generation.

## Installing

Copy the manifest and the compiled binary into a folder named `informix`
(matching the manifest `id`) inside the Tabularis plugins directory:

- **Windows:** `%APPDATA%\debba\tabularis\data\plugins\informix\`
- **Linux:** `~/.local/share/tabularis/plugins/informix/`
- **macOS:** `~/Library/Application Support/tabularis/plugins/informix/`

```
informix/
├── manifest.json
└── tabularis-informix-plugin(.exe)
```

Restart Tabularis (or install via **Settings → Installed Plugins**). "IBM
Informix" then appears in the Database Type list.

## Connecting

Fill the connection form as usual (host, port, user, password). Two
Informix-specific points:

1. **Informix server (dbservername).** Informix needs the `INFORMIXSERVER`
   (dbservername) in addition to the host/port — it is the *logical* instance
   name, distinct from the machine address. Provide it either:
   - **per connection (recommended), by typing the Host as `address@dbservername`**
     (e.g. `192.0.2.10@ol_informix1210`). This is the only per-connection option
     in multi-database mode, where the database field is hidden.
   - per connection via the database, as `dbname@dbservername`, when a single
     database field is shown, or
   - globally, via the **Default Informix Server** plugin setting (only useful
     when every connection targets the same dbservername).

   Because this driver is multi-database (you browse all databases on the
   instance from one connection), use the **Host = `address@dbservername`** form
   so each connection can point at a different server.
2. **ODBC driver / DSN.** By default the plugin builds a DSN-less connection
   string using the **ODBC Driver Name** setting (`IBM INFORMIX ODBC DRIVER`).
   If you prefer a pre-configured ODBC DSN, set the **ODBC DSN** setting and the
   host/server/protocol fields are taken from the DSN instead.

`DELIMIDENT=Y` is always set so double-quoted identifiers work and single
quotes delimit string literals.

### Plugin settings

| Setting | Purpose |
|---|---|
| ODBC Driver Name | Registered Informix ODBC driver name (DSN-less mode). |
| Default Informix Server | `INFORMIXSERVER` used when the database field has no `@server`. |
| Network Protocol | `onsoctcp` (default), `onsocssl`, `onipcshm`, `olsoctcp`. |
| ODBC DSN | Optional pre-configured DSN; bypasses DSN-less assembly. |
| DB_LOCALE / CLIENT_LOCALE | Optional locales (e.g. `en_US.819`). |
| Extra Connection Attributes | Verbatim `key=value;` ODBC attributes. |

## Feature coverage

| Area | Status |
|---|---|
| Connect / ping / test_connection | ✅ |
| Tables, columns, indexes, foreign keys | ✅ |
| Views (list, definition, columns) | ✅ |
| Routines (list, definition) | ✅ |
| Query execution with `SKIP`/`FIRST` pagination + total count | ✅ |
| Insert / update / delete rows | ✅ |
| Create table / add column / alter column | ✅ |
| Create / drop index, create / drop foreign key | ✅ |
| ER-diagram batch metadata (`get_schema_snapshot`, batches) | ✅ |

## Known limitations

- **Schemas / owners** are not exposed as a separate namespace. Tables are
  listed and referenced unqualified within the connected database; two objects
  with the same name under different owners are not disambiguated.
- **Column defaults** are best-effort. Keyword defaults (`TODAY`, `CURRENT`,
  `USER`, `DBSERVERNAME`) are mapped exactly; literal defaults are read from
  `sysdefaults` with the internal numeric class prefix stripped.
- **Routine parameters** are not introspected (Informix lacks a reliable
  catalog for named parameters); `get_routine_parameters` returns an empty list.
- **DECIMAL / MONEY** values are returned as strings to preserve precision;
  integer and floating-point columns are returned as JSON numbers.
- **SSL** is not wired up yet (`supports_ssl` is off). You can still select the
  `onsocssl` protocol and supply keystore attributes via *Extra Connection
  Attributes* if your environment is configured for it.
- `ON UPDATE` foreign-key actions are ignored (Informix does not support them);
  `ON DELETE CASCADE` is supported.

## Manual JSON-RPC smoke test

```sh
echo '{"jsonrpc":"2.0","method":"get_create_table_sql","params":{"table_name":"t","columns":[{"name":"id","data_type":"INTEGER","is_nullable":false,"is_pk":true,"is_auto_increment":true}],"schema":null},"id":1}' \
  | ./target/release/tabularis-informix-plugin
```

## License

Apache-2.0.
