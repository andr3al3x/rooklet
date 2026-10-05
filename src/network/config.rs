//! Managed parent configuration editing and live ruleset drift detection.
use super::MAX_FILE;
use anyhow::{Result, ensure};

const BEGIN: &str = "# BEGIN ROOKLET MANAGED ANCHOR";
const END: &str = "# END ROOKLET MANAGED ANCHOR";
const BLOCK: &str = "# BEGIN ROOKLET MANAGED ANCHOR\nanchor \"rooklet\"\nload anchor \"rooklet\" from \"/etc/pf.anchors/rooklet\"\n# END ROOKLET MANAGED ANCHOR\n";

pub(super) fn patch_config(source: &str, install: bool) -> Result<String> {
    ensure!(
        !source.contains('\r') && source.len() as u64 <= MAX_FILE,
        "Unsupported PF configuration encoding or size"
    );
    let begins = source.matches(BEGIN).count();
    let ends = source.matches(END).count();
    ensure!(
        begins == ends && begins <= 1,
        "Rooklet PF configuration markers were changed; refusing to overwrite them"
    );
    let mut clean = source.to_string();
    if begins == 1 {
        let start = source.find(BEGIN).unwrap();
        ensure!(
            start == 0 || source.as_bytes()[start - 1] == b'\n',
            "Rooklet marker is not on its own line"
        );
        ensure!(
            source[start..].starts_with(BLOCK),
            "Managed Rooklet PF block was changed; restore it before continuing"
        );
        clean.replace_range(start..start + BLOCK.len(), "");
    }
    for line in clean.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        ensure!(
            matches!(
                line,
                "scrub-anchor \"com.apple/*\""
                    | "nat-anchor \"com.apple/*\""
                    | "rdr-anchor \"com.apple/*\""
                    | "dummynet-anchor \"com.apple/*\""
                    | "anchor \"com.apple/*\""
                    | "load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\""
            ),
            "Custom PF configuration is present ({line}); Rooklet supports only Apple's stock anchor layout and refuses to reload this root ruleset"
        );
    }
    ensure!(
        clean
            .lines()
            .any(|line| line.trim() == "anchor \"com.apple/*\""),
        "Expected Apple's main filter anchor is missing"
    );
    if install {
        if begins == 1 {
            return Ok(source.into());
        }
        if !clean.ends_with('\n') {
            clean.push('\n');
        }
        clean.push_str(BLOCK);
    }
    Ok(clean)
}
pub(super) fn configured(source: &str) -> bool {
    patch_config(source, true).is_ok() && source.contains(BLOCK)
}
pub(super) fn has_parent_anchor(output: &str) -> bool {
    output
        .lines()
        .any(|line| matches!(line.trim(), "anchor \"rooklet\" all" | "anchor \"rooklet\""))
}
pub(super) fn parent_matches_managed_config(output: &str) -> bool {
    normalized_rules(output) == "anchor \"com.apple/*\" all\nanchor \"rooklet\" all"
}
pub(super) fn setup_needs_reload(
    is_configured: bool,
    parent_referenced: bool,
    live_anchor: &str,
    owned_token_live: bool,
) -> Result<bool> {
    if is_configured && parent_referenced {
        return Ok(false);
    }
    ensure!(
        !owned_token_live,
        "Rooklet owns a live PF reference while its parent configuration is missing; disable Rooklet before setting up again"
    );
    ensure!(
        live_anchor.is_empty(),
        "An existing live Rooklet anchor contains rules without a managed parent; disable Rooklet or inspect those rules before setup"
    );
    Ok(true)
}
fn diagnostic_line(line: &str) -> bool {
    line.starts_with("No ALTQ support")
        || line.starts_with("ALTQ related functions disabled")
        || line.starts_with("pfctl: Use of -f option")
        || line.starts_with("pfctl: Warning:")
}
pub(super) fn normalized_rules(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !diagnostic_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}
pub(super) fn validate_parent_rules(filters: &str, translations: &str) -> Result<()> {
    for line in filters
        .lines()
        .chain(translations.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        ensure!(
            matches!(
                line,
                "anchor \"com.apple/*\" all"
                    | "anchor \"rooklet\" all"
                    | "nat-anchor \"com.apple/*\" all"
                    | "rdr-anchor \"com.apple/*\" all"
            ) || diagnostic_line(line),
            "Custom live PF parent rules are present ({line}); refusing to reload them"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const APPLE: &str = "# original comment\nscrub-anchor \"com.apple/*\"\nnat-anchor \"com.apple/*\"\nrdr-anchor \"com.apple/*\"\ndummynet-anchor \"com.apple/*\"\nanchor \"com.apple/*\"\nload anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"\n";
    #[test]
    fn configuration_round_trip_preserves_apple() {
        let installed = patch_config(APPLE, true).unwrap();
        assert!(installed.starts_with(APPLE));
        assert_eq!(patch_config(&installed, true).unwrap(), installed);
        assert_eq!(patch_config(&installed, false).unwrap(), APPLE);
    }
    #[test]
    fn changed_block_and_custom_root_rules_are_rejected() {
        assert!(patch_config(&format!("{APPLE}pass all\n"), true).is_err());
        assert!(
            patch_config(
                &format!("{APPLE}{BLOCK}").replace("anchor \"rooklet\"\n", "anchor \"other\"\n"),
                false
            )
            .is_err()
        );
        assert!(patch_config(&format!("{APPLE}{BLOCK}{BLOCK}"), true).is_err());
        assert!(patch_config(&format!("{APPLE}# BEGIN ROOKLET MANAGED ANCHOR\n"), false).is_err());
    }
    #[test]
    fn live_rules_normalization_and_reference_are_exact() {
        assert!(has_parent_anchor("anchor \"rooklet\" all\n"));
        assert!(!has_parent_anchor("anchor \"rooklet/*\" all\n"));
        assert!(!has_parent_anchor("anchor \"rooklet-other\" all\n"));
        assert_eq!(
            normalized_rules(
                "No ALTQ support in kernel\nALTQ related functions disabled\npass in quick inet all\n"
            ),
            "pass in quick inet all"
        );
    }
    #[test]
    fn managed_anchor_requires_its_exact_block_and_destination() {
        assert!(patch_config(&format!("{APPLE}anchor \"rooklet\"\n"), true).is_err());
        let installed = patch_config(APPLE, true).unwrap();
        let replaced_destination =
            installed.replace("/etc/pf.anchors/rooklet", "/etc/pf.anchors/rooklet-other");
        assert!(!configured(&replaced_destination));
        assert!(patch_config(&replaced_destination, false).is_err());
        assert!(patch_config(&replaced_destination, true).is_err());
        assert!(!parent_matches_managed_config(
            "anchor \"com.apple/*\" all\nanchor \"rooklet-other\" all"
        ));
    }
    #[test]
    fn explicit_setup_recovers_an_unloaded_parent_without_adopting_orphan_rules() {
        assert!(setup_needs_reload(true, false, "", false).unwrap());
        assert!(setup_needs_reload(false, false, "", false).unwrap());
        assert!(!setup_needs_reload(true, true, "pass all", true).unwrap());
        assert!(setup_needs_reload(true, false, "pass all", false).is_err());
        assert!(setup_needs_reload(true, false, "", true).is_err());
        assert!(setup_needs_reload(false, true, "", true).is_err());
    }
    #[test]
    fn parent_reload_rejects_custom_filter_and_translation_rules() {
        let filters = "anchor \"com.apple/*\" all\nanchor \"rooklet\" all\n";
        let translations = "nat-anchor \"com.apple/*\" all\nrdr-anchor \"com.apple/*\" all\n";
        assert!(validate_parent_rules(filters, translations).is_ok());
        assert!(validate_parent_rules(&format!("{filters}pass all\n"), translations).is_err());
        assert!(
            validate_parent_rules(
                filters,
                &format!("{translations}nat on en0 from any to any -> 192.0.2.1\n")
            )
            .is_err()
        );
        assert!(validate_parent_rules("anchor \"other\" all", translations).is_err());
    }
}
