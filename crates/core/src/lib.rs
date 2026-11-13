//! The `inet` DuckDB extension: an `INET` type for IPv4 and IPv6 networks, and HTML escaping.

pub mod html;
pub mod ip;

mod functions;
mod inet;
mod rows;

use duckdb_neo::{Result, extension::Extension};

/// Register the `INET` type, its casts and all scalar functions.
pub fn register(extension: &Extension) -> Result<()> {
    inet::register(extension)?;
    functions::register(extension)
}
