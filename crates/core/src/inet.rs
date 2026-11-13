//! The `INET` logical type and its casts from and to `VARCHAR`.

use duckdb_neo::{
    AsDuckDB, DuckDBType, Error, Result,
    cast::{CastFunctionBuilder, CastFunctionCallbacks, CastMode},
    connection::{Context, FFILink},
    custom_type::CustomType,
    error::DuckDBError,
    extension::Extension,
    logical_type::LogicalType,
    types::HugeInt,
    vector::{RowReader, Vector},
};

use crate::ip::{Family, IpAddress, OffsetError, ParseError};
use crate::rows;

const IPV4: u8 = 1;
const IPV6: u8 = 2;

/// Flipping the top bit of IPv6 addresses makes the signed `address` sort like the unsigned address.
const IPV6_SORT_BIT: u128 = 1 << 127;

/// The physical layout of an `INET` value. Its `DuckDBType` is the bare `STRUCT`; [`logical_type`]
/// adds the `INET` alias.
///
/// Struct comparison orders values by family, then address, then prefix length.
#[derive(Debug, AsDuckDB)]
#[duckdb(reader, writer)]
pub(crate) struct InetStorage {
    ip_type: u8,
    address: HugeInt,
    mask: u16,
}

/// The `INET` logical type.
pub(crate) fn logical_type<C: FFILink + ?Sized>(link: &C) -> Result<LogicalType> {
    InetStorage::logical_type(link)?.with_alias(link, "INET")
}

impl From<IpAddress> for InetStorage {
    fn from(ip: IpAddress) -> Self {
        let (ip_type, bits) = match ip.family() {
            Family::V4 => (IPV4, ip.address()),
            Family::V6 => (IPV6, ip.address() ^ IPV6_SORT_BIT),
        };
        InetStorage {
            ip_type,
            address: HugeInt {
                lower: bits as u64,
                upper: (bits >> 64) as i64,
            },
            mask: ip.prefix().into(),
        }
    }
}

impl TryFrom<InetStorageRef<&u8, &HugeInt, &u16>> for IpAddress {
    type Error = Error;

    fn try_from(inet: InetStorageRef<&u8, &HugeInt, &u16>) -> Result<Self> {
        let (family, sort_bit) = match inet.ip_type {
            Some(&IPV4) => (Family::V4, 0),
            Some(&IPV6) => (Family::V6, IPV6_SORT_BIT),
            _ => return Err(Error::invalid_input("Invalid IP address type")),
        };
        let address = inet.address.map(|address| {
            (u128::from(address.upper as u64) << 64 | u128::from(address.lower)) ^ sort_bit
        });
        let prefix = inet.mask.and_then(|mask| u8::try_from(*mask).ok());
        address
            .zip(prefix)
            .and_then(|(address, prefix)| IpAddress::new(family, address, prefix))
            .ok_or_else(|| Error::invalid_input("Invalid INET value"))
    }
}

impl From<ParseError> for Error {
    fn from(error: ParseError) -> Self {
        Error {
            code: DuckDBError::TypeConversion,
            message: error.to_string(),
        }
    }
}

impl From<OffsetError> for Error {
    fn from(error: OffsetError) -> Self {
        Error {
            code: DuckDBError::InputOutOfRange,
            message: error.to_string(),
        }
    }
}

/// Discard the error of a row that a `TRY_CAST` turns into NULL.
fn try_row<T>(mode: CastMode, result: Result<T>) -> Result<Option<T>> {
    match (result, mode) {
        (Ok(value), _) => Ok(Some(value)),
        (Err(_), CastMode::Try) => Ok(None),
        (Err(error), CastMode::Normal) => Err(error),
    }
}

struct ParseInet;

impl CastFunctionCallbacks for ParseInet {
    type InputType = String;
    type OutputType = InetStorage;

    fn exec(
        &self,
        _context: &Context,
        mode: CastMode,
        input: Vector<'_, String>,
        output: Vector<'_, InetStorage>,
    ) -> Result<()> {
        let input = input.row_reader()?;
        rows::write_values(output, input.iter(), |text| {
            try_row(
                mode,
                text.parse::<IpAddress>()
                    .map(InetStorage::from)
                    .map_err(Error::from),
            )
        })
    }
}

struct FormatInet;

impl CastFunctionCallbacks for FormatInet {
    type InputType = InetStorage;
    type OutputType = String;

    fn exec(
        &self,
        _context: &Context,
        mode: CastMode,
        input: Vector<'_, InetStorage>,
        output: Vector<'_, String>,
    ) -> Result<()> {
        let input = input.row_reader()?;
        rows::write_text(output, input.iter(), |inet| {
            try_row(mode, IpAddress::try_from(inet))
        })
    }
}

/// Register the `INET` type with explicit casts from and to `VARCHAR`.
pub(crate) fn register(extension: &Extension) -> Result<()> {
    let inet = logical_type(extension)?;
    let varchar = String::logical_type(extension)?;
    CustomType::new("INET", inet.clone()).register(extension)?;
    CastFunctionBuilder::new(varchar.clone(), inet.clone(), ParseInet).register(extension)?;
    CastFunctionBuilder::new(inet, varchar, FormatInet).register(extension)
}
