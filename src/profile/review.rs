//! Sanitized complete-scope review of the captured proposal.
use super::preparation::Prepared;
use crate::{
    model::{NetworkRule, Setting},
    network,
    presentation::clean,
};
use std::fmt::Write;

pub(super) fn render(prepared: &Prepared) -> String {
    let before = &prepared.baseline;
    let profile = &prepared.profile;
    let mut out = String::from(
        "Replace every scope: firewall settings, incoming apps, machine-wide PF rules.\n",
    );
    let settings_changed = profile.firewall.as_ref().map_or(0, |settings| {
        [
            Setting::Firewall,
            Setting::Stealth,
            Setting::BlockAll,
            Setting::AllowSigned,
            Setting::AllowSignedApp,
        ]
        .iter()
        .filter(|setting| before.firewall.get(**setting) != settings.get(**setting))
        .count()
    });
    let apps_changed = profile
        .applications
        .iter()
        .filter(|app| {
            !before
                .applications
                .iter()
                .any(|old| old.path == app.path && old.blocked == app.blocked)
        })
        .count()
        + before
            .applications
            .iter()
            .filter(|old| !profile.applications.iter().any(|app| app.path == old.path))
            .count();
    let _ = writeln!(
        out,
        "Changes: {settings_changed} setting{}, {apps_changed} app{}; PF rules {} -> {}{}.",
        if settings_changed == 1 { "" } else { "s" },
        if apps_changed == 1 { "" } else { "s" },
        before.rules.len(),
        profile.network_rules.len(),
        if before.rules == profile.network_rules {
            " (same order/content)"
        } else {
            " (new order/content)"
        }
    );
    out.push_str("Firewall setting changes:\n");
    let mut changed = false;
    if let Some(settings) = &profile.firewall {
        for setting in [
            Setting::Firewall,
            Setting::Stealth,
            Setting::BlockAll,
            Setting::AllowSigned,
            Setting::AllowSignedApp,
        ] {
            let old = before.firewall.get(setting);
            let new = settings.get(setting);
            if old != new {
                let _ = writeln!(out, "  {setting}: {} -> {}", switch(old), switch(new));
                changed = true;
            }
        }
    }
    if !changed {
        out.push_str("  unchanged\n");
    }
    out.push_str("Incoming application changes:\n");
    changed = false;
    for app in &profile.applications {
        match before.applications.iter().find(|old| old.path == app.path) {
            None => {
                let _ = writeln!(
                    out,
                    "  add {}: {}",
                    clean(&app.path),
                    permission(app.blocked)
                );
                changed = true;
            }
            Some(old) if old.blocked != app.blocked => {
                let _ = writeln!(
                    out,
                    "  {}: {} -> {}",
                    clean(&app.path),
                    permission(old.blocked),
                    permission(app.blocked)
                );
                changed = true;
            }
            Some(_) => (),
        }
    }
    for app in &before.applications {
        if !profile.applications.iter().any(|new| new.path == app.path) {
            let _ = writeln!(
                out,
                "  remove {} (was {})",
                clean(&app.path),
                permission(app.blocked)
            );
            changed = true;
        }
    }
    if !changed {
        out.push_str("  unchanged\n");
    }
    let _ = writeln!(
        out,
        "Machine-wide ordered rules: {} before -> {} after; first enabled match wins.",
        before.rules.len(),
        profile.network_rules.len()
    );
    out.push_str("Before:\n");
    append_rules(&mut out, &before.rules);
    out.push_str("After:\n");
    append_rules(&mut out, &profile.network_rules);
    if let Ok(warnings) = network::shadow_warnings(&profile.network_rules) {
        for warning in warnings {
            let _ = writeln!(
                out,
                "Warning: #{} {} is fully shadowed by #{} {} within Xield's anchor.",
                warning.shadowed_position,
                clean(&warning.shadowed_id),
                warning.covering_position,
                clean(&warning.covering_id)
            );
        }
    }
    out.push_str("Incoming changes are sequential, not atomic. Failure triggers best-effort restoration with readback; restoration can fail.\n");
    out.push_str("PF affects the whole machine. Other anchors and existing states still affect traffic; a loaded rule is not an enforcement verdict.\n");
    if before.configured {
        out.push_str("Applying loads the complete Xield anchor and acquires Xield's PF enable reference when needed.\n");
        if !before.enabled || !before.applied {
            out.push_str("PF activation: the prior PF/Xield anchor is inactive or unapplied; this apply activates Xield's anchor.\n");
        }
        out.push_str("Rollback restores Xield rule content where possible; prior PF activation may remain changed. Other PF references are never disabled.\n");
    } else {
        out.push_str("PF is not configured; this profile has no network rules and does not set up or activate PF.\n");
    }
    out
}
fn switch(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
fn permission(blocked: bool) -> &'static str {
    if blocked {
        "block incoming"
    } else {
        "allow incoming"
    }
}
fn append_rules(out: &mut String, rules: &[NetworkRule]) {
    if rules.is_empty() {
        out.push_str("  (none)\n");
    }
    for (index, rule) in rules.iter().enumerate() {
        let port = rule
            .port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "any".into());
        let _ = writeln!(
            out,
            "  #{} {} [{}]: {} remote={} destination-port={} protocol={} direction={} interface={} enabled={}",
            index + 1,
            clean(&rule.id),
            clean(&rule.name),
            rule.action,
            clean(&rule.destination),
            port,
            rule.protocol,
            rule.direction,
            clean(rule.interface.as_deref().unwrap_or("any")),
            switch(rule.enabled)
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Action, Application, Direction, NetworkStatus, Profile, Protocol, Snapshot,
    };
    #[test]
    fn review_sanitizes_labels_and_explains_order_scope_and_activation() {
        let snapshot = Snapshot {
            firewall: Some(Default::default()),
            applications_available: true,
            network: NetworkStatus {
                rules_available: true,
                configured: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut profile = Profile::from_snapshot(&snapshot);
        profile.applications.push(Application {
            path: "/App\u{202e}".into(),
            name: "App".into(),
            blocked: true,
        });
        let rule = NetworkRule {
            id: "all".into(),
            name: "name\u{202e}".into(),
            action: Action::Block,
            destination: "any".into(),
            port: None,
            protocol: Protocol::Any,
            direction: Direction::Both,
            interface: None,
            enabled: true,
        };
        profile.network_rules = vec![
            rule.clone(),
            NetworkRule {
                id: "later".into(),
                ..rule
            },
        ];
        let prepared = Prepared {
            profile,
            baseline: super::super::preparation::Baseline::capture(&snapshot).unwrap(),
        };
        let review = prepared.review();
        assert!(!review.contains('\u{202e}'));
        assert!(review.contains("PF activation"));
        assert!(review.contains("fully shadowed"));
        assert!(review.contains("Before:"));
        assert!(review.contains("After:"));
        assert!(review.contains("not atomic"));
        assert!(review.contains("destination-port=any"));
        assert!(review.starts_with("Replace every scope:"));
        assert!(review.lines().nth(1).unwrap().starts_with("Changes:"));
        assert!(
            review.find("Incoming application changes:").unwrap()
                < review.find("not atomic").unwrap()
        );
        assert!(review.find("After:").unwrap() < review.find("PF activation:").unwrap());
    }
}
