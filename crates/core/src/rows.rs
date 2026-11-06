//! Helpers that fill an output vector row by row, keeping NULL inputs NULL.

use std::fmt::{self, Write};

use duckdb_neo::{
    Error, Result,
    data_chunk::VectorCollection,
    vector::{CombinedVectorIterator, Vector, VectorElement, WritableVectorElement},
};

use crate::html::{Escape, Unescape};
use crate::ip::IpAddress;

/// A value written as text into a reused buffer.
pub(crate) trait Render {
    fn render(&self, out: &mut String) -> fmt::Result;
}

impl Render for IpAddress {
    fn render(&self, out: &mut String) -> fmt::Result {
        write!(out, "{self}")
    }
}

impl Render for Escape<'_> {
    fn render(&self, out: &mut String) -> fmt::Result {
        self.write_to(out)
    }
}

impl Render for Unescape<'_> {
    fn render(&self, out: &mut String) -> fmt::Result {
        self.write_to(out)
    }
}

/// The rows of the first argument.
pub(crate) fn unary<'a, A: VectorElement + 'a>(
    args: &'a VectorCollection,
) -> Result<impl ExactSizeIterator<Item = Option<A::Ref<'a>>>> {
    Ok(args.get_vector_at::<A>(0)?.reader()?.iter())
}

/// The rows of the first two arguments, `None` where either is NULL.
pub(crate) fn binary<'a, A: VectorElement + 'a, B: VectorElement + 'a>(
    args: &'a VectorCollection,
) -> Result<impl ExactSizeIterator<Item = Option<(A::Ref<'a>, B::Ref<'a>)>>> {
    let a = args.get_vector_at::<A>(0)?.reader()?;
    let b = args.get_vector_at::<B>(1)?.reader()?;
    Ok(CombinedVectorIterator::new((a, b))?.map(|(a, b)| a.zip(b)))
}

/// Write `f` of every non-NULL row; `f` returns `None` for a NULL result.
pub(crate) fn write_values<In, T>(
    mut output: Vector<'_, T>,
    rows: impl ExactSizeIterator<Item = Option<In>>,
    mut f: impl FnMut(In) -> Result<Option<T>>,
) -> Result<()>
where
    T: for<'a> WritableVectorElement<Write<'a> = T>,
{
    output.set_size(rows.len())?;
    let mut writer = output.writer()?;
    for (row, input) in rows.enumerate() {
        let value = match input {
            Some(input) => f(input)?,
            None => None,
        };
        writer.set(row, value)?;
    }
    Ok(())
}

/// Write the text of `f` of every non-NULL row; `f` returns `None` for a NULL result.
pub(crate) fn write_text<In, D: Render>(
    mut output: Vector<'_, String>,
    rows: impl ExactSizeIterator<Item = Option<In>>,
    mut f: impl FnMut(In) -> Result<Option<D>>,
) -> Result<()> {
    output.set_size(rows.len())?;
    let mut writer = output.writer()?;
    let mut text = String::new();
    for (row, input) in rows.enumerate() {
        let value = match input {
            Some(input) => f(input)?,
            None => None,
        };
        match value {
            Some(value) => {
                text.clear();
                value
                    .render(&mut text)
                    .map_err(|_| Error::api_error("failed to format a value"))?;
                writer.set(row, Some(&text))?;
            }
            None => writer.set(row, None)?,
        }
    }
    Ok(())
}
