# DuckDB inet extension

Adds the `INET` type for IPv4 and IPv6 addresses with an optional network prefix, and HTML escaping functions.
The extension is written in Rust on [`duckdb-neo`](https://crates.io/crates/duckdb-neo), using DuckDB's v2 C API.

```sql
SELECT '192.168.1.5/24'::INET AS ip, host(ip), family(ip), netmask(ip), network(ip), broadcast(ip);
SELECT INET '10.1.2.0/24' <<= INET '10.0.0.0/8', INET '::ffff:127.0.0.31' + 32;
SELECT html_escape('<b>"duck"</b>'), html_unescape('I&hearts;DuckDB');
```

| Function | Description |
|----------|-------------|
| `host(inet)` | The address as `VARCHAR`, without its prefix |
| `family(inet)` | The IP version: 4 or 6 |
| `netmask(inet)` | The network mask, e.g. `255.255.255.0/24` |
| `network(inet)` | The first address of the network |
| `broadcast(inet)` | The last address of the network |
| `inet + hugeint`, `inet - hugeint` | Offset the address, keeping the prefix |
| `a <<= b`, `subnet_contained_by_or_equals(a, b)` | Whether network `a` lies within `b` (false across families) |
| `a >>= b`, `subnet_contains_or_equals(a, b)` | Whether network `a` contains `b` (false across families) |
| `html_escape(text, quote := true)` | Escape `&`, `<`, `>`, and quotes unless `quote` is false |
| `html_unescape(text)` | Decode HTML character references |

`INET` values are stored as `STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT)`, and sort by family,
address and prefix length.

## Layout

- `crates/core` (`inet_core`): the `INET` type, casts and functions. `ip` and `html` are plain Rust with unit tests.
- `crates/native` (`inet`): the loadable extension (`cdylib`).
- `crates/wasm` (`inet_wasm`): the Wasm build (`staticlib`).
- `test/integration`: Rust tests that load the built extension into DuckDB through `duckdb-neo`, comparing
  against the `ipnet` crate. A separate Cargo workspace, since it links DuckDB instead of being loaded by it.
- `scripts/generate_html_entities.py`: regenerates `crates/core/src/html/entities.rs` from `scripts/html_entities.json`.

## Building

Clone with submodules, then configure once and build. Building needs Rust, Python 3 (with `venv`) and Make.

```shell
make configure
make release      # or: make debug
```

The extension is written to `build/release/inet.duckdb_extension`. It is built against the C API of DuckDB
`TARGET_DUCKDB_VERSION` (see `Makefile`) and only loads into that exact version:

```shell
duckdb -unsigned -cmd "LOAD '$PWD/build/release/inet.duckdb_extension';"
```

## Testing

```shell
cargo test --workspace   # unit tests
make test_release        # SQL logic tests in test/sql and test/integration, or make test_debug
```
