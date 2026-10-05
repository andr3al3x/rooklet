//! Filtered rows and stable selection identities.
use super::{ActivitySort, App, SETTINGS, View, activity_query::ActivityQuery};
use rooklet_core::model::{Application, Connection, NetworkRule, ProcessActivity};

pub enum ActivityRow<'a> {
    Process(&'a ProcessActivity),
    Connection(&'a ProcessActivity, &'a Connection),
}
impl ActivityRow<'_> {
    pub fn key(&self) -> String {
        match self {
            Self::Process(process) => process_key(process),
            Self::Connection(process, flow) => format!(
                "flow:{}:{}:{}:{}",
                process_key(process),
                flow.protocol,
                flow.remote_ip,
                flow.remote_port.unwrap_or(0)
            ),
        }
    }
    pub fn process(&self) -> &ProcessActivity {
        match self {
            Self::Process(p) | Self::Connection(p, _) => p,
        }
    }
}
impl App {
    pub fn activity_rows(&self) -> Vec<ActivityRow<'_>> {
        let Ok(query) = ActivityQuery::parse(&self.filters[0]) else {
            return Vec::new();
        };
        let permissions = query
            .needs_incoming()
            .then(|| rooklet_core::permissions::Index::new(&self.snapshot));
        let mut processes: Vec<_> = self.snapshot.activity.iter().collect();
        if self.activity_sort != ActivitySort::Snapshot {
            processes.sort_by(|a, b| self.activity_sort.compare(a, b, &self.snapshot.resources));
        }
        let needs_peer = query.needs_peer();
        let mut result = Vec::new();
        for process in processes {
            let incoming = permissions
                .as_ref()
                .map(|index| index.activity(process).state)
                .unwrap_or(rooklet_core::permissions::IncomingState::Unknown);
            if !query.process_matches(process, incoming) {
                continue;
            }
            let matches_process = !needs_peer && query.plain_process_matches(process);
            let mut matching = process
                .connections
                .iter()
                .filter(|flow| query.flow_matches(process, flow))
                .peekable();
            if matches_process || matching.peek().is_some() {
                result.push(ActivityRow::Process(process));
                if self.expanded.contains(&process_key(process)) || needs_peer || !matches_process {
                    for flow in matching {
                        result.push(ActivityRow::Connection(process, flow));
                    }
                }
            }
        }
        result
    }
    /// Invalid typed filters are explicit and never silently fall back to free text.
    pub fn activity_filter_error(&self) -> Option<String> {
        ActivityQuery::parse(&self.filters[0]).err()
    }
    pub fn applications(&self) -> Vec<&Application> {
        let filter = self.filters[1].to_lowercase();
        self.snapshot
            .applications
            .iter()
            .filter(|app| {
                format!("{} {}", app.name, app.path)
                    .to_lowercase()
                    .contains(&filter)
            })
            .collect()
    }
    pub fn rule_rows(&self) -> Vec<(usize, &NetworkRule)> {
        let filter = self.filters[2].to_lowercase();
        self.snapshot
            .network
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                format!("{} {} {}", rule.name, rule.destination, rule.action)
                    .to_lowercase()
                    .contains(&filter)
            })
            .collect()
    }
    pub fn keys(&self) -> Vec<String> {
        match self.view {
            View::Activity => self.activity_rows().iter().map(ActivityRow::key).collect(),
            View::Applications => self.applications().iter().map(|a| a.path.clone()).collect(),
            View::Network => self.rule_rows().iter().map(|(_, r)| r.id.clone()).collect(),
            View::Settings => SETTINGS.iter().map(|(s, _)| s.to_string()).collect(),
        }
    }
    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selection[self.view.index()].as_ref()?;
        self.keys().iter().position(|key| key == selected)
    }
    pub fn selected_key(&self) -> Option<&str> {
        self.selection[self.view.index()].as_deref()
    }
    pub(super) fn reconcile(&mut self) {
        let keys = self.keys();
        if !self.selection[self.view.index()]
            .as_ref()
            .is_some_and(|key| keys.contains(key))
        {
            self.selection[self.view.index()] = keys.first().cloned();
        }
    }
    pub(super) fn prune_expanded(&mut self) {
        let present: std::collections::HashSet<_> = self
            .snapshot
            .activity
            .iter()
            .filter(|process| {
                self.paused
                    || app_path(process).is_some()
                    || process
                        .identities
                        .iter()
                        .any(|identity| identity.pid == process.pid)
            })
            .map(process_key)
            .collect();
        self.expanded.retain(|key| present.contains(key));
    }
    pub(super) fn navigate(&mut self, delta: isize) {
        let keys = self.keys();
        if keys.is_empty() {
            return;
        }
        let index = self
            .selected_key()
            .and_then(|selected| keys.iter().position(|key| key == selected))
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(keys.len() - 1);
        self.selection[self.view.index()] = Some(keys[index].clone());
    }
}
pub fn process_key(process: &ProcessActivity) -> String {
    if let Some(path) = app_path(process) {
        format!("app:{path}")
    } else {
        match process
            .identities
            .iter()
            .find(|identity| identity.pid == process.pid)
        {
            Some(identity) => format!(
                "process:{}:{}:{}:{}",
                process.pid, identity.pid_version, identity.start_sec, identity.start_usec
            ),
            None => format!("process:{}", process.pid),
        }
    }
}
fn app_path(process: &ProcessActivity) -> Option<&str> {
    process.path.as_deref().filter(|path| {
        std::path::Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "app")
    })
}
