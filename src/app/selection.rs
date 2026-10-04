//! Filtered rows and stable selection identities.
use super::{App, SETTINGS, View};
use crate::model::{Application, Connection, NetworkRule, ProcessActivity};

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
        let filter = self.filters[0].to_lowercase();
        let mut result = Vec::new();
        for process in &self.snapshot.activity {
            let matches_process = format!(
                "{} {} {}",
                process.name,
                process.pid,
                process.path.as_deref().unwrap_or("")
            )
            .to_lowercase()
            .contains(&filter);
            let matching: Vec<_> = process
                .connections
                .iter()
                .filter(|flow| {
                    format!(
                        "{} {} {}",
                        flow.remote_ip,
                        flow.remote_port.unwrap_or(0),
                        flow.country
                            .as_ref()
                            .map(|c| format!("{} {}", c.code, c.name))
                            .unwrap_or_else(|| if flow.local {
                                "Local network".into()
                            } else {
                                "Unknown".into()
                            })
                    )
                    .to_lowercase()
                    .contains(&filter)
                })
                .collect();
            if matches_process || !matching.is_empty() {
                result.push(ActivityRow::Process(process));
                if self.expanded.contains(&process_key(process))
                    || (!filter.is_empty() && !matches_process)
                {
                    for flow in matching {
                        result.push(ActivityRow::Connection(process, flow));
                    }
                }
            }
        }
        result
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
    pub fn rules(&self) -> Vec<&NetworkRule> {
        let filter = self.filters[2].to_lowercase();
        self.snapshot
            .network
            .rules
            .iter()
            .filter(|rule| {
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
            View::Network => self.rules().iter().map(|r| r.id.clone()).collect(),
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
    pub(super) fn navigate(&mut self, delta: isize) {
        let keys = self.keys();
        if keys.is_empty() {
            return;
        }
        let index = self
            .selected_index()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(keys.len() - 1);
        self.selection[self.view.index()] = Some(keys[index].clone());
    }
}
pub fn process_key(process: &ProcessActivity) -> String {
    if let Some(path) = &process.path
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e == "app")
    {
        format!("app:{path}")
    } else {
        format!("process:{}", process.pid)
    }
}
