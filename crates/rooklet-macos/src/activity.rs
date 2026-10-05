//! Persistent nettop CSV observations. Totals begin with this monitor's first sample.
mod identity;
mod monitor;
mod parser;
mod tracker;

pub(crate) use monitor::Monitor;

const MAX_LINE: usize = 64 * 1024;
const MAX_RECORDS: usize = 32768;

#[cfg(test)]
#[path = "activity/tests.rs"]
mod regression_tests;
