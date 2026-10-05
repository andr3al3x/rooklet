//! macOS PF rules in a dedicated, explicitly referenced anchor.
//!
//! Compilation is pure; live operations serialize mutations and retain PF
//! ownership references so rollback never disables another PF user.
mod analysis;
mod compiler;
mod config;
mod lifecycle;
mod persistence;
mod pf;

pub use analysis::{
    QueryField, RuleExplanation, RuleQuery, ShadowWarning, explain_rules, shadow_warnings,
};
pub use compiler::{compile_rules, render_rules, validate_rules};
pub use lifecycle::{apply, disable, remove, setup, status};

const CONFIG: &str = "/etc/pf.conf";
const ANCHOR_FILE: &str = "/etc/pf.anchors/xield";
const MAX_FILE: u64 = 1_048_576;
