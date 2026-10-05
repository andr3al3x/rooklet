//! Command-line syntax; execution lives in sibling modules.
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use xield::{
    model::{Action, Direction, Protocol, Setting},
    ui::Theme,
};

#[derive(Parser)]
#[command(
    version,
    about = "A standalone macOS firewall manager and traffic monitor"
)]
pub(super) struct Cli {
    /// Simulate activity and permissions; never changes system settings.
    #[arg(long, global = true)]
    pub(super) demo: bool,
    #[arg(long, value_enum, default_value = "dark", global = true)]
    pub(super) theme: Theme,
    #[command(subcommand)]
    pub(super) command: Option<CliCommand>,
}
#[derive(Subcommand)]
pub(super) enum CliCommand {
    /// Read actual firewall state and observed traffic as JSON.
    Status,
    /// Check tools and report optional setup or monitoring limitations.
    Doctor,
    /// Install or refresh the local country database. Lookups stay offline.
    Geoip {
        #[command(subcommand)]
        command: GeoipCommand,
    },
    /// Manage incoming application permissions.
    Apps {
        #[command(subcommand)]
        command: AppsCommand,
    },
    /// Manage macOS's incoming application firewall settings.
    Firewall {
        #[command(subcommand)]
        command: FirewallCommand,
    },
    /// Manage machine-wide PF network rules. Existing connections may continue.
    Network {
        #[command(subcommand)]
        command: NetworkCommand,
    },
    /// Export, validate, and explicitly apply a complete configuration profile.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
}
#[derive(Subcommand)]
pub(super) enum GeoipCommand {
    /// Download DB-IP Country Lite and atomically replace the managed database.
    Update,
}
#[derive(Subcommand)]
pub(super) enum AppsCommand {
    List,
    Add { path: String },
    Allow { path: String },
    Block { path: String },
    Remove { path: String },
}
#[derive(Subcommand)]
pub(super) enum FirewallCommand {
    Status,
    Set {
        #[arg(value_enum)]
        setting: Setting,
        #[arg(value_enum)]
        state: Switch,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Switch {
    On,
    Off,
}
#[derive(Subcommand)]
pub(super) enum NetworkCommand {
    Status,
    /// Validate saved or supplied rules and report complete single-rule shadowing. Read-only.
    Check(InputRules),
    /// Predict matching within Xield's anchor for a hypothetical connection. No live verdict.
    Explain {
        #[command(flatten)]
        input: InputRules,
        #[arg(long)]
        remote: std::net::IpAddr,
        /// Hypothetical connection protocol: tcp or udp.
        #[arg(long, default_value = "tcp", value_parser = query_protocol)]
        protocol: Protocol,
        /// Hypothetical connection direction: in or out.
        #[arg(long, default_value = "out", value_parser = query_direction)]
        direction: Direction,
        /// Destination service port; omission means unknown (local for in, remote for out).
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        port: Option<u16>,
        /// Interface name; omission means unknown, not any interface.
        #[arg(long)]
        interface: Option<String>,
    },
    /// Install a dedicated PF anchor in a supported configuration; preserves existing rules.
    Setup(InputRules),
    /// Validate and replace only Xield's network rules.
    Apply(InputRules),
    /// Render validated PF syntax without changing anything.
    Preview(InputRules),
    Add(NetworkArgs),
    Delete {
        id: String,
    },
    Toggle {
        id: String,
    },
    Move {
        id: String,
        #[arg(allow_hyphen_values=true,value_parser=clap::value_parser!(i64).range(-1..=1))]
        offset: i64,
    },
    /// Clear Xield's loaded anchor and release its own PF enable reference.
    Disable,
    /// Remove Xield's anchor setup and owned state.
    Remove,
}
#[derive(Args)]
pub(super) struct InputRules {
    /// JSON rule array. Setup may omit it; check/explain use saved rules when omitted.
    pub(super) path: Option<PathBuf>,
    /// Read the JSON rule array from stdin.
    #[arg(long, conflicts_with = "path")]
    pub(super) stdin: bool,
}
fn query_protocol(value: &str) -> Result<Protocol, String> {
    match value {
        "tcp" => Ok(Protocol::Tcp),
        "udp" => Ok(Protocol::Udp),
        _ => Err("use tcp or udp for a hypothetical connection".into()),
    }
}
fn query_direction(value: &str) -> Result<Direction, String> {
    match value {
        "in" => Ok(Direction::Inbound),
        "out" => Ok(Direction::Outbound),
        _ => Err("use in or out for a hypothetical connection".into()),
    }
}
#[derive(Args)]
pub(super) struct NetworkArgs {
    /// Remote peer IP or CIDR, or "any".
    pub(super) destination: String,
    #[arg(long)]
    pub(super) id: Option<String>,
    #[arg(long, default_value = "Network rule")]
    pub(super) name: String,
    #[arg(long, value_enum, default_value = "block")]
    pub(super) action: Action,
    /// Destination port: remote for outgoing, local service port for incoming.
    #[arg(long,value_parser=clap::value_parser!(u16).range(1..))]
    pub(super) port: Option<u16>,
    #[arg(long, value_enum, default_value = "any")]
    pub(super) protocol: Protocol,
    #[arg(long, value_enum, default_value = "out")]
    pub(super) direction: Direction,
    #[arg(long)]
    pub(super) interface: Option<String>,
}
#[derive(Subcommand)]
pub(super) enum ProfileCommand {
    /// Export observed settings, incoming app entries, and saved network rules.
    Export { path: Option<PathBuf> },
    /// Validate a new-format profile and display the planned configuration.
    Check { path: PathBuf },
    /// Apply all scopes. ALF operations are sequential; failures trigger best-effort restoration.
    Apply {
        path: PathBuf,
        #[arg(long)]
        yes: bool,
    },
}
