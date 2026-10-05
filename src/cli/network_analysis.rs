//! Read-only rule diagnostics from bounded JSON input or explicitly available saved rules.
use super::{args::InputRules, config::input_rules, network::network_status};
use crate::json::print_json;
use anyhow::{Result, ensure};
use serde_json::json;
use xield::{
    app::clean,
    model::NetworkRule,
    network::{self, RuleQuery},
};

const SCOPE: &str = "Prediction within Xield's anchor only. Other anchors and existing PF states can affect traffic; this is not a live verdict.";

fn rules(input: &InputRules) -> Result<Vec<NetworkRule>> {
    if input.path.is_some() || input.stdin {
        return input_rules(input, false);
    }
    let status = network_status()?;
    ensure!(
        status.rules_available,
        "saved rules are unavailable; use sudo -v or supply a JSON rules file/--stdin"
    );
    Ok(status.rules)
}

pub(super) fn check(input: &InputRules) -> Result<()> {
    let rules = rules(input)?;
    print_json(
        &json!({"scope": SCOPE, "rule_count": rules.len(), "warnings": network::shadow_warnings(&rules)?}),
    )
}

pub(super) fn explain(input: &InputRules, query: RuleQuery) -> Result<()> {
    let rules = rules(input)?;
    let explanation = network::explain_rules(&rules, &query)?;
    print_json(&json!({"scope": SCOPE, "query": query, "explanation": explanation}))
}

pub(super) fn warn(rules: &[NetworkRule]) -> Result<()> {
    for warning in network::shadow_warnings(rules)? {
        eprintln!(
            "warning: #{} {} is fully shadowed by #{} {} within Xield's anchor",
            warning.shadowed_position,
            clean(&warning.shadowed_id),
            warning.covering_position,
            clean(&warning.covering_id)
        );
    }
    Ok(())
}
