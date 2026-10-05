//! macOS observations, verified firewall changes, and bounded native operations.
mod activity;
mod command;
mod process;
mod resources;

pub mod application;
pub mod backend;
pub mod geoip;
pub mod network;
pub mod permissions;
pub mod profile;

/// Whether this process already has administrator privileges.
pub fn is_root() -> bool {
    command::is_root()
}
