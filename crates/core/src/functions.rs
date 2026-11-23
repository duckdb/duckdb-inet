//! Scalar functions over `INET` values and HTML text.

use duckdb_neo::{
    DuckDBType, Result,
    data_chunk::VectorCollection,
    extension::Extension,
    logical_type::LogicalType,
    scalar::MicroScalar,
    signature::{FunctionDocumentationBuilder, FunctionParameter, SignatureBuilder},
    types::HugeInt,
    vector::{RowReader, Vector, WritableNativeElement},
};

use crate::html;
use crate::inet::{self, InetStorage};
use crate::ip::{IpAddress, OffsetError};
use crate::rows::{self, binary, unary, zip};

fn host(args: &VectorCollection, output: Vector<'_, String>) -> Result<()> {
    let input = unary::<InetStorage>(args)?;
    rows::write_text(output, input.iter(), |inet| {
        Ok(Some(IpAddress::try_from(inet)?.host()))
    })
}

fn family(args: &VectorCollection, output: Vector<'_, u8>) -> Result<()> {
    let input = unary::<InetStorage>(args)?;
    rows::write_values(output, input.iter(), |inet| {
        Ok(Some(IpAddress::try_from(inet)?.family().version()))
    })
}

fn map_inet(
    args: &VectorCollection,
    output: Vector<'_, InetStorage>,
    f: impl Fn(IpAddress) -> IpAddress,
) -> Result<()> {
    let input = unary::<InetStorage>(args)?;
    rows::write_values(output, input.iter(), |inet| {
        Ok(Some(InetStorage::from(f(IpAddress::try_from(inet)?))))
    })
}

fn netmask(args: &VectorCollection, output: Vector<'_, InetStorage>) -> Result<()> {
    map_inet(args, output, IpAddress::netmask)
}

fn network(args: &VectorCollection, output: Vector<'_, InetStorage>) -> Result<()> {
    map_inet(args, output, IpAddress::network)
}

fn broadcast(args: &VectorCollection, output: Vector<'_, InetStorage>) -> Result<()> {
    map_inet(args, output, IpAddress::broadcast)
}

fn offset_inet(
    args: &VectorCollection,
    output: Vector<'_, InetStorage>,
    f: impl Fn(IpAddress, i128) -> std::result::Result<IpAddress, OffsetError>,
) -> Result<()> {
    let input = binary::<InetStorage, HugeInt>(args)?;
    rows::write_values(output, zip(&input), |(inet, offset)| {
        let offset = i128::from(offset.upper) << 64 | i128::from(offset.lower);
        Ok(Some(InetStorage::from(f(
            IpAddress::try_from(inet)?,
            offset,
        )?)))
    })
}

fn add(args: &VectorCollection, output: Vector<'_, InetStorage>) -> Result<()> {
    offset_inet(args, output, IpAddress::checked_add)
}

fn subtract(args: &VectorCollection, output: Vector<'_, InetStorage>) -> Result<()> {
    offset_inet(args, output, IpAddress::checked_sub)
}

fn contained_by(args: &VectorCollection, output: Vector<'_, bool>) -> Result<()> {
    let input = binary::<InetStorage, InetStorage>(args)?;
    rows::write_values(output, zip(&input), |(inet, network)| {
        Ok(Some(
            IpAddress::try_from(inet)?.is_subnet_of(IpAddress::try_from(network)?),
        ))
    })
}

fn contains(args: &VectorCollection, output: Vector<'_, bool>) -> Result<()> {
    let input = binary::<InetStorage, InetStorage>(args)?;
    rows::write_values(output, zip(&input), |(network, inet)| {
        Ok(Some(
            IpAddress::try_from(inet)?.is_subnet_of(IpAddress::try_from(network)?),
        ))
    })
}

fn html_escape(args: &VectorCollection, output: Vector<'_, String>) -> Result<()> {
    let input = binary::<String, bool>(args)?;
    rows::write_text(output, zip(&input), |(text, quote)| {
        Ok(Some(html::escape(text, *quote)))
    })
}

fn html_unescape(args: &VectorCollection, output: Vector<'_, String>) -> Result<()> {
    let input = unary::<String>(args)?;
    rows::write_text(output, input.iter(), |text| Ok(Some(html::unescape(text))))
}

fn register_scalar<O: WritableNativeElement + 'static>(
    extension: &Extension,
    name: &str,
    description: &str,
    params: Vec<FunctionParameter>,
    return_type: &LogicalType,
    exec: fn(&VectorCollection, Vector<'_, O>) -> Result<()>,
) -> Result<()> {
    let signature = params
        .into_iter()
        .fold(SignatureBuilder::new(), SignatureBuilder::param)
        .return_type(return_type.clone())
        .documentation(FunctionDocumentationBuilder::new().description(description));
    MicroScalar::with_signature(name, signature, move |_, args, output| exec(args, output))
        .register(extension)
}

/// Register all scalar functions; `INET` must be registered first.
pub(crate) fn register(extension: &Extension) -> Result<()> {
    let inet = inet::logical_type(extension)?;
    let varchar = String::logical_type(extension)?;
    let boolean = bool::logical_type(extension)?;

    let inet_param = |name: &str| FunctionParameter::with_type(name, inet.clone());
    let text_param = || FunctionParameter::new::<String>("text");

    register_scalar(
        extension,
        "host",
        "Returns the IP address without its network prefix.",
        vec![inet_param("inet")],
        &varchar,
        host,
    )?;
    register_scalar(
        extension,
        "family",
        "Returns the IP version of the address: 4 or 6.",
        vec![inet_param("inet")],
        &u8::logical_type(extension)?,
        family,
    )?;
    register_scalar(
        extension,
        "netmask",
        "Returns the network mask of the address.",
        vec![inet_param("inet")],
        &inet,
        netmask,
    )?;
    register_scalar(
        extension,
        "network",
        "Returns the network part of the address.",
        vec![inet_param("inet")],
        &inet,
        network,
    )?;
    register_scalar(
        extension,
        "broadcast",
        "Returns the broadcast address of the network.",
        vec![inet_param("inet")],
        &inet,
        broadcast,
    )?;

    let offset_params = || vec![inet_param("inet"), FunctionParameter::new::<i128>("offset")];
    register_scalar(
        extension,
        "+",
        "Adds an offset to the IP address.",
        offset_params(),
        &inet,
        add,
    )?;
    register_scalar(
        extension,
        "-",
        "Subtracts an offset from the IP address.",
        offset_params(),
        &inet,
        subtract,
    )?;

    for name in ["<<=", "subnet_contained_by_or_equals"] {
        let description =
            "Returns whether the first network is contained by or equal to the second.";
        register_scalar(
            extension,
            name,
            description,
            vec![inet_param("inet"), inet_param("network")],
            &boolean,
            contained_by,
        )?;
    }
    for name in [">>=", "subnet_contains_or_equals"] {
        let description = "Returns whether the first network contains or equals the second.";
        register_scalar(
            extension,
            name,
            description,
            vec![inet_param("network"), inet_param("inet")],
            &boolean,
            contains,
        )?;
    }

    register_scalar(
        extension,
        "html_escape",
        "Escapes &, < and > as HTML character references, and also quotes unless `quote` is false.",
        vec![text_param(), FunctionParameter::with_default("quote", true)],
        &varchar,
        html_escape,
    )?;
    register_scalar(
        extension,
        "html_unescape",
        "Decodes the HTML character references in the text.",
        vec![text_param()],
        &varchar,
        html_unescape,
    )
}
