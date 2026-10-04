//! Persistent nettop CSV observations. Totals begin with this monitor's first sample.
mod identity;
mod monitor;
mod parser;
mod tracker;

pub use monitor::Monitor;
pub use parser::{CsvParser, RawFlow, RawProcess, parse_csv, parse_flow};
pub use tracker::Tracker;

const MAX_LINE: usize = 64 * 1024;
const MAX_RECORDS: usize = 32768;
