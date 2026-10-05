use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! choice {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
        pub enum $name { $(#[serde(rename = $label)] #[value(name = $label)] $variant),+ }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self { $(Self::$variant => $label),+ })
            }
        }
    };
}
choice!(Action { Allow => "allow", Block => "block" });
choice!(Protocol { Any => "any", Tcp => "tcp", Udp => "udp" });
choice!(Direction { Both => "both", Inbound => "in", Outbound => "out" });
choice!(Setting { Firewall => "firewall", Stealth => "stealth", BlockAll => "block-all", AllowSigned => "allow-signed", AllowSignedApp => "allow-signed-app" });

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirewallSettings {
    pub enabled: bool,
    pub stealth: bool,
    pub block_all: bool,
    pub allow_signed: bool,
    pub allow_signed_app: bool,
}
impl FirewallSettings {
    pub fn get(&self, setting: Setting) -> bool {
        match setting {
            Setting::Firewall => self.enabled,
            Setting::Stealth => self.stealth,
            Setting::BlockAll => self.block_all,
            Setting::AllowSigned => self.allow_signed,
            Setting::AllowSignedApp => self.allow_signed_app,
        }
    }
    pub fn set(&mut self, setting: Setting, value: bool) {
        match setting {
            Setting::Firewall => self.enabled = value,
            Setting::Stealth => self.stealth = value,
            Setting::BlockAll => self.block_all = value,
            Setting::AllowSigned => self.allow_signed = value,
            Setting::AllowSignedApp => self.allow_signed_app = value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub path: String,
    pub name: String,
    pub blocked: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Country {
    pub code: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub remote_ip: String,
    pub remote_port: Option<u16>,
    pub protocol: Protocol,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub country: Option<Country>,
    pub local: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessActivity {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
    pub identities: Vec<crate::process::ProcessIdentity>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub rate_in: u64,
    pub rate_out: u64,
    pub connections: Vec<Connection>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRule {
    pub id: String,
    pub name: String,
    pub action: Action,
    pub destination: String,
    pub port: Option<u16>,
    pub protocol: Protocol,
    pub direction: Direction,
    pub interface: Option<String>,
    pub enabled: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub rules_available: bool,
    pub configured: bool,
    pub enabled: bool,
    pub applied: bool,
    pub rules: Vec<NetworkRule>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub firewall: Option<FirewallSettings>,
    pub applications_available: bool,
    pub applications: Vec<Application>,
    pub activity: Vec<ProcessActivity>,
    pub network: NetworkStatus,
    pub geoip: Option<String>,
    pub notices: Vec<String>,
    /// Local filesystem evidence; excluded from configuration and JSON output.
    #[serde(skip)]
    pub permission_paths: crate::permissions::Paths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub format: String,
    pub version: u32,
    pub firewall: Option<FirewallSettings>,
    pub applications: Vec<Application>,
    pub network_rules: Vec<NetworkRule>,
}
impl Profile {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        Self {
            format: "rooklet-profile".into(),
            version: 1,
            firewall: snapshot.firewall.clone(),
            applications: snapshot.applications.clone(),
            network_rules: snapshot.network.rules.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Mutation {
    Setting(Setting, bool),
    Applications { paths: Vec<String>, action: Action },
    AddApplication(String),
    RemoveApplication(String),
    NetworkRules(Vec<NetworkRule>),
}
