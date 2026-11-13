//! Helpers that fill an output vector row by row, keeping NULL inputs NULL.

use std::fmt::{self, Write};

use duckdb_neo::{
    Error, Result,
    data_chunk::VectorCollection,
    vector::{RowReader, RowWriter, Vector, WithReader, WithWriter},
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

/// The row reader of the first argument.
pub(crate) fn unary<'a, A: WithReader + 'a>(args: &'a VectorCollection) -> Result<A::Reader<'a>> {
    args.get_vector_at::<A>(0)?.row_reader()
}

/// The row readers of the first two arguments.
pub(crate) fn binary<'a, A: WithReader + 'a, B: WithReader + 'a>(
    args: &'a VectorCollection,
) -> Result<(A::Reader<'a>, B::Reader<'a>)> {
    let a = args.get_vector_at::<A>(0)?.row_reader()?;
    let b = args.get_vector_at::<B>(1)?.row_reader()?;
    if a.len() != b.len() {
        return Err(Error::invalid_input("argument vectors differ in length"));
    }
    Ok((a, b))
}

/// The rows of two readers, `None` where either is NULL.
pub(crate) fn zip<'r, A: RowReader, B: RowReader>(
    (a, b): &'r (A, B),
) -> impl ExactSizeIterator<Item = Option<(A::Item<'r>, B::Item<'r>)>> {
    a.iter().zip(b.iter()).map(|(a, b)| a.zip(b))
}

/// Write `f` of every non-NULL row; `f` returns `None` for a NULL result.
pub(crate) fn write_values<In, T>(
    mut output: Vector<'_, T>,
    rows: impl ExactSizeIterator<Item = Option<In>>,
    mut f: impl FnMut(In) -> Result<Option<T>>,
) -> Result<()>
where
    T: WithWriter,
    for<'w> T::Writer<'w>: RowWriter<Value = T>,
{
    output.set_size(rows.len())?;
    let mut writer = output.row_writer()?;
    for (row, input) in rows.enumerate() {
        let value = match input {
            Some(input) => f(input)?,
            None => None,
        };
        writer.set_option(row, value.as_ref())?;
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
                writer.set(row, &text)?;
            }
            None => writer.set_null(row)?,
        }
    }
    Ok(())
}
