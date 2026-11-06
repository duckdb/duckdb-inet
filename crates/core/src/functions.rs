//! Scalar functions over `INET` values and HTML text.

use duckdb_neo::{
    DuckDBType, Result,
    connection::Context,
    data_chunk::VectorCollection,
    extension::Extension,
    logical_type::LogicalType,
    scalar::{ScalarCallbacks, ScalarFunctionBuilder},
    signature::{FunctionDocumentationBuilder, FunctionParameter, SignatureBuilder},
    types::HugeInt,
    vector::{Unknown, Vector},
};

use crate::html;
use crate::inet::Inet;
use crate::ip::{IpAddress, OffsetError};
use crate::rows::{self, binary, unary};

type Exec = fn(&VectorCollection, Vector<'_, Unknown>) -> Result<()>;

/// A scalar function without bind or init state.
struct Scalar(Exec);

impl ScalarCallbacks for Scalar {
    type BindData = ();
    type InitData = ();

    fn exec(
        &self,
        _bind_data: Option<&()>,
        _init_data: Option<&mut ()>,
        _context: &Context,
        args: &mut VectorCollection,
        output: Vector<'_, Unknown>,
    ) -> Result<()> {
        (self.0)(args, output)
    }
}

fn host(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_text(output.cast()?, unary::<Inet>(args)?, |inet| {
        Ok(Some(IpAddress::try_from(inet)?.host()))
    })
}

fn family(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_values(output.cast()?, unary::<Inet>(args)?, |inet| {
        Ok(Some(IpAddress::try_from(inet)?.family().version()))
    })
}

fn map_inet(
    args: &VectorCollection,
    output: Vector<'_, Unknown>,
    f: impl Fn(IpAddress) -> IpAddress,
) -> Result<()> {
    rows::write_values(output.cast()?, unary::<Inet>(args)?, |inet| {
        Ok(Some(Inet::from(f(IpAddress::try_from(inet)?))))
    })
}

fn netmask(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    map_inet(args, output, IpAddress::netmask)
}

fn network(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    map_inet(args, output, IpAddress::network)
}

fn broadcast(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    map_inet(args, output, IpAddress::broadcast)
}

fn offset_inet(
    args: &VectorCollection,
    output: Vector<'_, Unknown>,
    f: impl Fn(IpAddress, i128) -> std::result::Result<IpAddress, OffsetError>,
) -> Result<()> {
    rows::write_values(
        output.cast()?,
        binary::<Inet, HugeInt>(args)?,
        |(inet, offset)| {
            let offset = i128::from(offset.upper) << 64 | i128::from(offset.lower);
            Ok(Some(Inet::from(f(IpAddress::try_from(inet)?, offset)?)))
        },
    )
}

fn add(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    offset_inet(args, output, IpAddress::checked_add)
}

fn subtract(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    offset_inet(args, output, IpAddress::checked_sub)
}

fn contained_by(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_values(
        output.cast()?,
        binary::<Inet, Inet>(args)?,
        |(inet, network)| {
            Ok(Some(
                IpAddress::try_from(inet)?.is_subnet_of(IpAddress::try_from(network)?),
            ))
        },
    )
}

fn contains(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_values(
        output.cast()?,
        binary::<Inet, Inet>(args)?,
        |(network, inet)| {
            Ok(Some(
                IpAddress::try_from(inet)?.is_subnet_of(IpAddress::try_from(network)?),
            ))
        },
    )
}

fn html_escape(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_text(
        output.cast()?,
        binary::<String, bool>(args)?,
        |(text, quote)| Ok(Some(html::escape(text, *quote))),
    )
}

fn html_unescape(args: &VectorCollection, output: Vector<'_, Unknown>) -> Result<()> {
    rows::write_text(output.cast()?, unary::<String>(args)?, |text| {
        Ok(Some(html::unescape(text)))
    })
}

/// Register all scalar functions; `INET` must be registered first.
pub(crate) fn register(extension: &Extension) -> Result<()> {
    let inet = Inet::logical_type(extension)?;
    let varchar = String::logical_type(extension)?;
    let boolean = bool::logical_type(extension)?;

    let register = |name: &str,
                    description: &str,
                    params: Vec<FunctionParameter>,
                    return_type: &LogicalType,
                    exec: Exec| {
        let signature = params
            .into_iter()
            .fold(SignatureBuilder::new(), SignatureBuilder::param)
            .return_type(return_type.clone())
            .documentation(FunctionDocumentationBuilder::new().description(description));
        ScalarFunctionBuilder::new(name, signature, Scalar(exec)).register(extension)
    };
    let inet_param = |name: &str| FunctionParameter::new::<Inet>(name);
    let text_param = || FunctionParameter::new::<String>("text");

    register(
        "host",
        "Returns the IP address without its network prefix.",
        vec![inet_param("inet")],
        &varchar,
        host,
    )?;
    register(
        "family",
        "Returns the IP version of the address: 4 or 6.",
        vec![inet_param("inet")],
        &u8::logical_type(extension)?,
        family,
    )?;
    register(
        "netmask",
        "Returns the network mask of the address.",
        vec![inet_param("inet")],
        &inet,
        netmask,
    )?;
    register(
        "network",
        "Returns the network part of the address.",
        vec![inet_param("inet")],
        &inet,
        network,
    )?;
    register(
        "broadcast",
        "Returns the broadcast address of the network.",
        vec![inet_param("inet")],
        &inet,
        broadcast,
    )?;

    let offset_params = || vec![inet_param("inet"), FunctionParameter::new::<i128>("offset")];
    register(
        "+",
        "Adds an offset to the IP address.",
        offset_params(),
        &inet,
        add,
    )?;
    register(
        "-",
        "Subtracts an offset from the IP address.",
        offset_params(),
        &inet,
        subtract,
    )?;

    for name in ["<<=", "subnet_contained_by_or_equals"] {
        let description =
            "Returns whether the first network is contained by or equal to the second.";
        register(
            name,
            description,
            vec![inet_param("inet"), inet_param("network")],
            &boolean,
            contained_by,
        )?;
    }
    for name in [">>=", "subnet_contains_or_equals"] {
        let description = "Returns whether the first network contains or equals the second.";
        register(
            name,
            description,
            vec![inet_param("network"), inet_param("inet")],
            &boolean,
            contains,
        )?;
    }

    register(
        "html_escape",
        "Escapes &, < and > as HTML character references, and also quotes unless `quote` is false.",
        vec![text_param(), FunctionParameter::with_default("quote", true)],
        &varchar,
        html_escape,
    )?;
    register(
        "html_unescape",
        "Decodes the HTML character references in the text.",
        vec![text_param()],
        &varchar,
        html_unescape,
    )
}
