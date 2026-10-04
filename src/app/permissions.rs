//! Confirm captured incoming entries for either a grouped app or one registration.
use super::{App, View, clean};
use crate::model::{Action, Mutation};
impl App {
    pub(super) fn application_action(&mut self, action: Action) {
        if !self.snapshot.applications_available {
            self.notify(
                "Incoming application entries unavailable; refresh before changing permissions"
                    .into(),
                true,
            );
            return;
        }
        let paths = match self.view {
            View::Applications => self
                .selected_key()
                .map(|path| vec![path.to_owned()])
                .unwrap_or_default(),
            View::Activity => self
                .activity_rows()
                .get(self.selected_index().unwrap_or(0))
                .map(|row| self.incoming(row.process()).paths)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if paths.is_empty() {
            self.notify("No registered incoming entries for this app; add its bundle or executable in Applications (n)".into(), true);
            return;
        }
        let noun = if paths.len() == 1 { "entry" } else { "entries" };
        let entries = paths
            .iter()
            .map(|path| clean(path))
            .collect::<Vec<_>>()
            .join("\n");
        let body = format!(
            "{action} incoming connections for {} registered {noun}:\n\n{entries}\n\nThis does not control outgoing traffic. Changes are verified per entry; partial failures are reported.",
            paths.len()
        );
        self.confirm(
            "Incoming application permissions",
            body,
            Mutation::Applications { paths, action },
        );
    }
}
