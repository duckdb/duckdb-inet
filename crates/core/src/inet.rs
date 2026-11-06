//! The `INET` logical type and its casts from and to `VARCHAR`.

use duckdb_loadable_macros::{AsVectorElement, AsWritableVectorElement};
use duckdb_neo::{
    DuckDBType, Error, Result,
    cast::{CastFunctionBuilder, CastFunctionCallbacks, CastMode},
    connection::{Context, FFILink},
    custom_type::CustomType,
    error::DuckDBError,
    extension::Extension,
    logical_type::LogicalType,
    types::HugeInt,
    vector::Vector,
};

use crate::ip::{Family, IpAddress, OffsetError, ParseError};
use crate::rows;

const IPV4: u8 = 1;
const IPV6: u8 = 2;

/// Flipping the top bit of IPv6 addresses makes the signed `address` sort like the unsigned address.
const IPV6_SORT_BIT: u128 = 1 << 127;

/// The physical layout of an `INET` value, a `STRUCT` aliased as `INET`.
///
/// Struct comparison orders values by family, then address, then prefix length.
#[derive(Debug, AsVectorElement, AsWritableVectorElement)]
pub(crate) struct Inet {
    ip_type: u8,
    address: HugeInt,
    mask: u16,
}

impl DuckDBType for Inet {
    fn logical_type<C: FFILink + ?Sized>(link: &C) -> Result<LogicalType> {
        LogicalType::from_text(
            link,
            "STRUCT(ip_type UTINYINT, address HUGEINT, mask USMALLINT)",
        )?
        .with_alias(link, "INET")
    }
}

impl From<IpAddress> for Inet {
    fn from(ip: IpAddress) -> Self {
        let (ip_type, bits) = match ip.family() {
            Family::V4 => (IPV4, ip.address()),
            Family::V6 => (IPV6, ip.address() ^ IPV6_SORT_BIT),
        };
        Inet {
            ip_type,
            address: HugeInt {
                lower: bits as u64,
                upper: (bits >> 64) as i64,
            },
            mask: ip.prefix().into(),
        }
    }
}

impl TryFrom<InetRef<'_>> for IpAddress {
    type Error = Error;

    fn try_from(inet: InetRef<'_>) -> Result<Self> {
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
    type OutputType = Inet;

    fn exec(
        &self,
        _context: &Context,
        mode: CastMode,
        input: Vector<'_, String>,
        output: Vector<'_, Inet>,
    ) -> Result<()> {
        let input = input.reader()?;
        rows::write_values(output, input.iter(), |text| {
            try_row(
                mode,
                text.parse::<IpAddress>()
                    .map(Inet::from)
                    .map_err(Error::from),
            )
        })
    }
}

struct FormatInet;

impl CastFunctionCallbacks for FormatInet {
    type InputType = Inet;
    type OutputType = String;

    fn exec(
        &self,
        _context: &Context,
        mode: CastMode,
        input: Vector<'_, Inet>,
        output: Vector<'_, String>,
    ) -> Result<()> {
        let input = input.reader()?;
        rows::write_text(output, input.iter(), |inet| {
            try_row(mode, IpAddress::try_from(inet))
        })
    }
}

/// Register the `INET` type with explicit casts from and to `VARCHAR`.
pub(crate) fn register(extension: &Extension) -> Result<()> {
    let inet = Inet::logical_type(extension)?;
    let varchar = String::logical_type(extension)?;
    CustomType::new("INET", inet.clone()).register(extension)?;
    CastFunctionBuilder::new(varchar.clone(), inet.clone(), ParseInet).register(extension)?;
    CastFunctionBuilder::new(inet, varchar, FormatInet).register(extension)
}
