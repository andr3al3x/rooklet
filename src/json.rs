//! Consistent JSON CLI output.
use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};

pub(crate) fn print_json(value: &impl Serialize) -> Result<()> {
    write_json(&mut io::stdout().lock(), value)
}
pub(crate) fn write_json(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut *output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}
