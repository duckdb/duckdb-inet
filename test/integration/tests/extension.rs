//! Persistence and differential tests that load the built `inet` extension into DuckDB.
//!
//! `INET_EXTENSION` selects the extension file; it defaults to the release build.

use std::{
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    process::Command,
};

use duckdb_neo::{
    Parameters,
    connection::Connection,
    environment::{Environment, StorageLocation},
    instance::{AttachBuilder, InstanceBuilder},
};
use ipnet::IpNet;

fn extension() -> PathBuf {
    let path = env::var_os("INET_EXTENSION")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/release/inet.duckdb_extension")
        });
    path.canonicalize()
        .unwrap_or_else(|error| panic!("extension {}: {error}", path.display()))
}

fn open(location: StorageLocation, load_inet: bool) -> Connection {
    let environment = Environment::new().unwrap();
    let instance = InstanceBuilder::new()
        .option("allow_unsigned_extensions", "true")
        .option("autoload_known_extensions", "false")
        .option("autoinstall_known_extensions", "false")
        .build(&environment)
        .unwrap();
    instance
        .attach(AttachBuilder::new(location).make_default())
        .unwrap();
    let connection = instance.connect().unwrap();
    if load_inet {
        let path = extension().display().to_string().replace('\'', "''");
        execute(&connection, &format!("LOAD '{path}'"));
    }
    connection
}

fn on_disk(path: &Path) -> StorageLocation {
    StorageLocation::OnDisk(path.to_str().unwrap().to_owned())
}

fn execute(connection: &Connection, sql: &str) {
    connection
        .execute(sql, Parameters::None)
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

/// Every row of `sql`, with each value rendered as `VARCHAR` and `None` for NULL.
fn query(connection: &Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    let text = format!("SELECT COLUMNS(*)::VARCHAR FROM ({sql})");
    let mut result = connection
        .query(text.as_str(), Parameters::None)
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    let mut rows = Vec::new();
    while let Some(chunk) = result.next_chunk().unwrap() {
        let columns = (0..chunk.col_count())
            .map(|column| chunk.get_vector_at::<String>(column).unwrap())
            .collect::<Vec<_>>();
        let readers = columns
            .iter()
            .map(|column| column.reader().unwrap())
            .collect::<Vec<_>>();
        for row in 0..chunk.row_count().unwrap() {
            rows.push(
                readers
                    .iter()
                    .map(|reader| reader.get(row).unwrap().map(str::to_owned))
                    .collect(),
            );
        }
    }
    rows
}

/// The only row of `sql`, with NULLs as `"NULL"`.
fn query_row(connection: &Connection, sql: &str) -> Vec<String> {
    let mut rows = query(connection, sql);
    assert_eq!(rows.len(), 1, "{sql}");
    rows.remove(0)
        .into_iter()
        .map(|value| value.unwrap_or_else(|| "NULL".to_owned()))
        .collect()
}

fn column(connection: &Connection, sql: &str) -> Vec<Option<String>> {
    query(connection, sql)
        .into_iter()
        .map(|mut row| row.remove(0))
        .collect()
}

const STORED_VALUES: [Option<&str>; 8] = [
    Some("127.0.0.1"),
    None,
    Some("255.255.255.255/31"),
    Some("0.0.0.0/0"),
    Some("::1"),
    Some("2266:25::12:0:ad12/96"),
    Some("::/0"),
    Some("8000::"),
];

const DATABASE_VARIABLE: &str = "INET_TEST_DATABASE";

#[test]
fn restart_and_nested_storage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inet.db");
    {
        let connection = open(on_disk(&path), true);
        execute(&connection, "CREATE TABLE addresses(id INTEGER, ip INET)");
        let values = STORED_VALUES
            .iter()
            .enumerate()
            .map(|(id, value)| match value {
                Some(value) => format!("({id}, '{value}')"),
                None => format!("({id}, NULL)"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        execute(
            &connection,
            &format!("INSERT INTO addresses VALUES {values}"),
        );
        execute(
            &connection,
            "CREATE VIEW iview AS SELECT INET '127.0.0000.1' AS ip",
        );
        execute(
            &connection,
            "CREATE TABLE nested AS SELECT list(ip ORDER BY id) AS ips FROM addresses",
        );
        execute(&connection, "CHECKPOINT");
    }

    let expected = STORED_VALUES.map(|value| value.map(str::to_owned));
    for _ in 0..2 {
        let connection = open(on_disk(&path), true);
        assert_eq!(
            column(&connection, "SELECT ip FROM addresses ORDER BY id"),
            expected
        );
        assert_eq!(
            column(&connection, "SELECT ip FROM iview"),
            [Some("127.0.0.1".to_owned())]
        );
        assert_eq!(
            column(&connection, "SELECT unnest(ips) FROM nested"),
            expected
        );
        let hosts = expected
            .iter()
            .step_by(2)
            .map(|value| {
                value
                    .as_deref()
                    .map(|value| value.split('/').next().unwrap().to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            column(
                &connection,
                "SELECT host(ip) FROM addresses WHERE id % 2 = 0 ORDER BY id"
            ),
            hosts
        );
    }

    let status = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "reads_database_in_fresh_process",
            "--ignored",
            "--quiet",
        ])
        .env(DATABASE_VARIABLE, &path)
        .env("INET_EXTENSION", extension())
        .status()
        .unwrap();
    assert!(status.success(), "the fresh process failed: {status}");
}

#[test]
#[ignore = "run in a fresh process by restart_and_nested_storage"]
fn reads_database_in_fresh_process() {
    let path = PathBuf::from(env::var_os(DATABASE_VARIABLE).expect("INET_TEST_DATABASE is set"));
    let connection = open(on_disk(&path), true);
    assert_eq!(
        query_row(&connection, "SELECT count(ip), count(*) FROM addresses"),
        ["7", "8"]
    );
    assert_eq!(
        query_row(&connection, "SELECT ip FROM addresses WHERE id = 7"),
        ["8000::"]
    );
}

/// The persisted layout, built without the extension, reads as the expected addresses.
#[test]
fn legacy_physical_layout_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("layout.db");
    {
        let connection = open(on_disk(&path), false);
        execute(
            &connection,
            "CREATE TABLE fixture(id INTEGER, ip STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT))",
        );
        let rows = [
            (1, 2_130_706_433_i128, 32),
            (2, i128::MIN + 1, 128),
            (2, 0, 64),
            (2, i128::MAX, 128),
        ];
        for (id, (ip_type, address, mask)) in rows.into_iter().enumerate() {
            execute(
                &connection,
                &format!(
                    "INSERT INTO fixture VALUES ({id}, {{'ip_type': {ip_type}, 'address': '{address}'::HUGEINT, 'mask': {mask}}})"
                ),
            );
        }
    }
    let connection = open(on_disk(&path), true);
    assert_eq!(
        column(&connection, "SELECT ip::INET FROM fixture ORDER BY id"),
        [
            "127.0.0.1",
            "::1",
            "8000::/64",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"
        ]
        .map(|text| Some(text.to_owned()))
    );
}

/// A deterministic SplitMix64 generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn bits(&mut self, bits: u32) -> u128 {
        let value = u128::from(self.next()) << 64 | u128::from(self.next());
        value >> (128 - bits)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

struct Case {
    text: String,
    delta: i128,
    address: u128,
    network: IpNet,
}

impl Case {
    fn ip(&self, address: u128) -> IpAddr {
        match self.network {
            IpNet::V4(_) => IpAddr::V4(Ipv4Addr::from(address as u32)),
            IpNet::V6(_) => IpAddr::V6(Ipv6Addr::from(address)),
        }
    }
}

fn generate_cases() -> Vec<Case> {
    let mut rng = Rng(0x1a37);
    let mut cases = Vec::new();
    for bits in [32_u32, 128] {
        let max = u128::MAX >> (128 - bits);
        for index in 0..2500 {
            let address = match index {
                0 => 0,
                1 => 1,
                2 => max,
                3 => 1 << (bits - 1),
                _ => rng.bits(bits),
            };
            let prefix = rng.below(u64::from(bits) + 1) as u8;
            let delta = (i128::from(rng.below(2001)) - 1000).clamp(
                -(address.min(1000) as i128),
                (max - address).min(1000) as i128,
            );
            let ip = match bits {
                32 => IpAddr::V4(Ipv4Addr::from(address as u32)),
                _ => IpAddr::V6(Ipv6Addr::from(address)),
            };
            cases.push(Case {
                text: format!("{ip}/{prefix}"),
                delta,
                address,
                network: IpNet::new(ip, prefix).unwrap(),
            });
        }
    }
    cases
}

fn parse_network(text: &str) -> IpNet {
    match text.parse() {
        Ok(network) => network,
        Err(_) => IpNet::from(
            text.parse::<IpAddr>()
                .unwrap_or_else(|error| panic!("{text}: {error}")),
        ),
    }
}

fn parse_host(text: &Option<String>) -> IpAddr {
    let text = text.as_deref().expect("a non-NULL host");
    text.parse()
        .unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// INET operations agree with the `ipnet` crate on 5,000 generated addresses and 5,000 pairs.
#[test]
fn randomized_ip_operations_and_selected_vectors() {
    let cases = generate_cases();
    let connection = open(StorageLocation::InMemory, true);
    execute(
        &connection,
        "CREATE TABLE cases(id INTEGER, text VARCHAR, delta HUGEINT)",
    );
    let values = cases
        .iter()
        .enumerate()
        .map(|(id, case)| format!("({id}, '{}', {})", case.text, case.delta))
        .collect::<Vec<_>>()
        .join(", ");
    execute(&connection, &format!("INSERT INTO cases VALUES {values}"));
    execute(
        &connection,
        "CREATE TABLE parsed AS SELECT id, text::INET AS ip, delta FROM cases",
    );

    let select = "SELECT id, ip, family(ip), host(ip), host(network(ip)), host(broadcast(ip)), host(netmask(ip)),
        host(ip + delta), (ip::STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT)).address,
        (ip::STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT)).mask,
        subnet_contained_by_or_equals(ip, network(ip)), ip <<= broadcast(ip), ip >>= network(ip),
        ((ip + delta)::STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT)).mask
        FROM parsed";
    for (filter, ids) in [
        (" ORDER BY id", (0..cases.len()).collect::<Vec<_>>()),
        (
            " WHERE id % 7 = 3 ORDER BY id DESC",
            (0..cases.len()).filter(|id| id % 7 == 3).rev().collect(),
        ),
    ] {
        let rows = query(&connection, &format!("{select}{filter}"));
        let row_ids = rows
            .iter()
            .map(|row| row[0].as_deref().unwrap().parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(row_ids, ids);
        for (row, id) in rows.iter().zip(ids) {
            let case = &cases[id];
            let network = case.network;
            let text = |column: usize| row[column].as_deref().unwrap();
            assert_eq!(parse_network(text(1)), network, "{}", case.text);
            let (version, stored) = match network {
                IpNet::V4(_) => ("4", case.address as i128),
                IpNet::V6(_) => ("6", (case.address ^ 1 << 127) as i128),
            };
            assert_eq!(text(2), version);
            assert_eq!(parse_host(&row[3]), case.ip(case.address));
            assert_eq!(parse_host(&row[4]), network.network(), "{}", case.text);
            assert_eq!(parse_host(&row[5]), network.broadcast(), "{}", case.text);
            assert_eq!(parse_host(&row[6]), network.netmask(), "{}", case.text);
            let shifted = case.address.checked_add_signed(case.delta).unwrap();
            assert_eq!(
                parse_host(&row[7]),
                case.ip(shifted),
                "{} + {}",
                case.text,
                case.delta
            );
            assert_eq!(text(8), stored.to_string());
            assert_eq!(text(9), network.prefix_len().to_string());
            assert_eq!([text(10), text(11), text(12)], ["true"; 3], "{}", case.text);
            assert_eq!(text(13), network.prefix_len().to_string());
        }
    }

    assert_eq!(
        query_row(
            &connection,
            "SELECT count(*) FROM parsed a JOIN parsed b USING (id) WHERE a.ip = b.ip"
        ),
        [cases.len().to_string()]
    );

    let pairs = query(
        &connection,
        "SELECT a.id, b.id, a.ip <<= b.ip, a.ip >>= b.ip,
            subnet_contained_by_or_equals(a.ip, b.ip), subnet_contains_or_equals(a.ip, b.ip)
         FROM parsed a JOIN parsed b ON b.id = (a.id * 13 + 7) % 5000",
    );
    assert_eq!(pairs.len(), cases.len());
    for row in pairs {
        let id = |column: usize| row[column].as_deref().unwrap().parse::<usize>().unwrap();
        let (left, right) = (cases[id(0)].network, cases[id(1)].network);
        let contained = right.contains(&left).to_string();
        let contains = left.contains(&right).to_string();
        let actual = row[2..]
            .iter()
            .map(|value| value.clone().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [contained.as_str(), &contains, &contained, &contains],
            "{left} vs {right}"
        );
    }
}

#[test]
fn independent_databases_and_signatures() {
    for _ in 0..2 {
        let connection = open(StorageLocation::InMemory, true);
        assert_eq!(
            query_row(
                &connection,
                "SELECT 1 + 2, 5 - 2, html_escape('\"'), html_escape('\"', false)"
            ),
            ["3", "3", "&quot;", "\""]
        );
        assert_eq!(
            query_row(
                &connection,
                "SELECT parameters, parameter_types FROM duckdb_functions() WHERE function_name = 'html_escape'"
            ),
            ["[text, quote]", "[VARCHAR, BOOLEAN]"]
        );
        assert_eq!(
            query_row(&connection, "SELECT html_escape('\"', quote := false)"),
            ["\""]
        );
        assert_eq!(
            query_row(
                &connection,
                "SELECT can_cast_implicitly(NULL::VARCHAR, NULL::INET)"
            ),
            ["false"]
        );
    }
}
