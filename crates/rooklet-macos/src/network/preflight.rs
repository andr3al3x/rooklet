//! Read-only host validation shared by profile preparation and PF application.
use super::{
    ANCHOR_FILE, CONFIG, MAX_FILE,
    config::{configured, parent_matches_managed_config},
    persistence::{
        State, bounded_read, read_state, trusted, trusted_parents, validate_state_destination,
    },
    pf::{root, run, validate_pf, verify_interfaces},
};
use anyhow::{Result, ensure};
use rooklet_core::model::NetworkRule;
use rooklet_core::network::compile_rules;
use std::path::Path;

pub(super) struct Validated {
    pub source: String,
    pub expected: String,
    pub previous: String,
}

/// Checks interfaces, trusted configuration, PF syntax, and persisted state size.
/// No rules, files, enable references, or connection states are changed.
pub(super) fn preflight_apply(rules: &[NetworkRule]) -> Result<()> {
    root()?;
    validate(rules)?;
    validate_state_destination()?;
    read_state()?;
    Ok(())
}

pub(super) fn validate(rules: &[NetworkRule]) -> Result<Validated> {
    validate_with(rules, &System)
}

// Only read-only operations belong here; apply runs this under its mutation lock
// before writing state, acquiring an enable reference, or loading an anchor.
trait Host {
    fn verify_interfaces(&self, rules: &[NetworkRule]) -> Result<()>;
    fn config(&self) -> Result<String>;
    fn parent(&self) -> Result<String>;
    fn anchor(&self) -> Result<String>;
    fn preview(&self, source: &str) -> Result<String>;
}

struct System;
impl Host for System {
    fn verify_interfaces(&self, rules: &[NetworkRule]) -> Result<()> {
        verify_interfaces(rules)
    }
    fn config(&self) -> Result<String> {
        trusted_parents(Path::new(CONFIG))?;
        trusted(Path::new(CONFIG), false)?;
        bounded_read(Path::new(CONFIG))
    }
    fn parent(&self) -> Result<String> {
        run(&["-sr"], None)
    }
    fn anchor(&self) -> Result<String> {
        trusted_parents(Path::new(ANCHOR_FILE))?;
        trusted(Path::new(ANCHOR_FILE), false)?;
        bounded_read(Path::new(ANCHOR_FILE))
    }
    fn preview(&self, source: &str) -> Result<String> {
        validate_pf(source)
    }
}

fn validate_with(rules: &[NetworkRule], host: &impl Host) -> Result<Validated> {
    let compiled = compile_rules(rules)?;
    host.verify_interfaces(rules)?;
    ensure!(
        configured(&host.config()?),
        "Rooklet's parent anchor is not configured; run network setup first"
    );
    ensure!(
        parent_matches_managed_config(&host.parent()?),
        "Live PF parent configuration differs from Rooklet's managed layout; inspect PF before applying rules"
    );
    let previous = host.anchor()?;
    ensure!(
        previous.starts_with("# Rooklet network rules:"),
        "Rooklet's anchor file was replaced; refusing to overwrite it"
    );
    let expected = host.preview(&compiled)?;
    let candidate = State {
        rules: rules.to_vec(),
        enable_token: Some("0".repeat(32)),
        active: true,
        loaded_rules: Some(expected.clone()),
    };
    ensure!(
        serde_json::to_vec_pretty(&candidate)?.len() as u64 <= MAX_FILE,
        "Compiled network state exceeds the 1 MiB safety limit; use fewer rules"
    );
    Ok(Validated {
        source: compiled,
        expected,
        previous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeHost<'a> {
        parent: &'a str,
        calls: RefCell<Vec<&'static str>>,
    }
    impl Host for FakeHost<'_> {
        fn verify_interfaces(&self, _rules: &[NetworkRule]) -> Result<()> {
            self.calls.borrow_mut().push("interfaces");
            Ok(())
        }
        fn config(&self) -> Result<String> {
            self.calls.borrow_mut().push("config");
            super::super::config::patch_config("anchor \"com.apple/*\"\n", true)
        }
        fn parent(&self) -> Result<String> {
            self.calls.borrow_mut().push("parent");
            Ok(self.parent.into())
        }
        fn anchor(&self) -> Result<String> {
            self.calls.borrow_mut().push("anchor");
            compile_rules(&[])
        }
        fn preview(&self, _source: &str) -> Result<String> {
            self.calls.borrow_mut().push("preview");
            Ok(String::new())
        }
    }

    #[test]
    fn shared_apply_validation_refuses_parent_drift_before_anchor_work() {
        for parent in [
            "anchor \"rooklet\" all",
            "anchor \"rooklet\" all\nanchor \"com.apple/*\" all",
            "pass quick all\nanchor \"com.apple/*\" all\nanchor \"rooklet\" all",
            "anchor \"com.apple/*\" all\nanchor \"rooklet\" all\nanchor \"other\" all",
        ] {
            let host = FakeHost {
                parent,
                calls: RefCell::default(),
            };
            let Err(error) = validate_with(&[], &host) else {
                panic!("parent drift accepted: {parent}");
            };
            assert!(error.to_string().contains("managed layout"));
            assert_eq!(*host.calls.borrow(), ["interfaces", "config", "parent"]);
        }
    }

    #[test]
    fn shared_apply_validation_accepts_only_exact_parent_order() {
        let host = FakeHost {
            parent: "No ALTQ support in kernel\nanchor \"com.apple/*\" all\nanchor \"rooklet\" all\n",
            calls: RefCell::default(),
        };
        let validated = validate_with(&[], &host).unwrap();
        assert_eq!(validated.source, compile_rules(&[]).unwrap());
        assert_eq!(
            *host.calls.borrow(),
            ["interfaces", "config", "parent", "anchor", "preview"]
        );
    }
}
