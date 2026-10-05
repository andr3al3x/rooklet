//! Pure validation, PF compilation, and hypothetical rule analysis.
mod analysis;
mod compiler;
pub use analysis::{
    QueryField, RuleExplanation, RuleQuery, ShadowWarning, explain_rules, shadow_warnings,
};
pub use compiler::{compile_rules, render_rules, validate_rules};
