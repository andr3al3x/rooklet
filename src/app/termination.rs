//! Identity-bound process control proposals; dispatch happens only after confirmation.
use super::{App, ConfirmedAction, Popup, clean};
use rooklet_core::process::{TerminationMode, TerminationRequest};

impl App {
    pub fn termination_finished(
        &mut self,
        snapshot: rooklet_core::model::Snapshot,
        report: &rooklet_core::process::TerminationReport,
    ) {
        self.update(snapshot, false);
        self.termination_report(report);
    }
    pub fn termination_report(&mut self, report: &rooklet_core::process::TerminationReport) {
        self.busy = false;
        let count = report.delivered.len();
        let noun = if count == 1 { "process" } else { "processes" };
        let target_noun = if report.attempted == 1 {
            "process"
        } else {
            "processes"
        };
        let text = if report.failures.is_empty() {
            format!("Signal delivered to {count} {noun}; activity will refresh")
        } else {
            format!(
                "Signal delivered to {count}/{} {target_noun}; {} failed: {}",
                report.attempted,
                report.failures.len(),
                report.failures[0].reason
            )
        };
        self.notify(text, !report.failures.is_empty());
    }

    pub(super) fn termination_action(&mut self, mode: TerminationMode) {
        let rows = self.activity_rows();
        let Some(row) = self.selected_index().and_then(|index| rows.get(index)) else {
            return;
        };
        let process = row.process();
        if process.identities.is_empty() {
            self.notify(
                "No verified process identities available; refresh activity before terminating"
                    .into(),
                true,
            );
            return;
        }
        let targets = process.identities.clone();
        let full_name = clean(&process.name);
        let name = if full_name.chars().count() > 24 {
            format!("{}…", full_name.chars().take(24).collect::<String>())
        } else {
            full_name
        };
        let entries = targets
            .iter()
            .map(|identity| format!("PID {} · {}", identity.pid, clean(&identity.path)))
            .collect::<Vec<_>>()
            .join("\n");
        let (title, signal) = match mode {
            TerminationMode::Terminate => ("Terminate application", "SIGTERM"),
            TerminationMode::ForceKill => ("Force kill application", "SIGKILL"),
        };
        self.popup = Some(Popup::Confirm {
            title: title.into(),
            scroll: Default::default(),
            body: format!(
                "Send {signal} to {name}?\nUnsaved work may be lost. Processes can restart.\n\nIncludes this app's {} captured processes and helpers:\n{entries}",
                targets.len()
            ),
            action: ConfirmedAction::Terminate(TerminationRequest { targets, mode }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reporting_without_a_new_observation_preserves_snapshot_age() {
        let mut app = App::new(rooklet_core::model::Snapshot::default());
        app.updated_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(6));
        app.busy = true;
        app.termination_report(&rooklet_core::process::TerminationReport {
            attempted: 1,
            delivered: vec![201],
            failures: Vec::new(),
        });
        assert!(app.stale());
        assert!(!app.busy);
    }
}
