//! Application state and the public interaction API.
mod activity_query;
mod dialog;
mod editor;
mod input;
mod mouse;
mod permissions;
mod rules;
mod selection;
mod termination;

pub use crate::presentation::{bytes, clean, countries};
pub use activity_query::ActivitySort;
pub use dialog::{ConfirmedAction, NetworkDraft, Popup};
pub use mouse::MouseAction;
pub use rules::RuleProbe;
pub use selection::{ActivityRow, process_key};

use crate::model::{Mutation, ProcessActivity, Setting, Snapshot};
use std::{
    collections::{HashSet, VecDeque},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Activity,
    Applications,
    Network,
    Settings,
}
impl View {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Activity => "Activity",
            Self::Applications => "Applications",
            Self::Network => "Network",
            Self::Settings => "Settings",
        }
    }
    pub const ALL: [Self; 4] = [
        Self::Activity,
        Self::Applications,
        Self::Network,
        Self::Settings,
    ];
    pub fn index(self) -> usize {
        match self {
            Self::Activity => 0,
            Self::Applications => 1,
            Self::Network => 2,
            Self::Settings => 3,
        }
    }
}
pub const SETTINGS: [(Setting, &str); 5] = [
    (Setting::Firewall, "Application firewall"),
    (Setting::Stealth, "Stealth mode"),
    (Setting::BlockAll, "Block all incoming"),
    (Setting::AllowSigned, "Automatically allow built-in apps"),
    (
        Setting::AllowSignedApp,
        "Automatically allow downloaded signed apps",
    ),
];
#[derive(Default)]
pub struct Effect {
    pub quit: bool,
    pub mutation: Option<Mutation>,
    pub authenticate: bool,
    pub update_geoip: bool,
    pub terminate: Option<crate::process::TerminationRequest>,
}
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}
pub struct App {
    pub snapshot: Snapshot,
    pub view: View,
    pub popup: Option<Popup>,
    pub busy: bool,
    pub paused: bool,
    pub searching: bool,
    pub filters: [String; 4],
    pub activity_sort: ActivitySort,
    pub selection: [Option<String>; 4],
    pub expanded: HashSet<String>,
    pub chart: VecDeque<(u64, u64)>,
    pub notice: Option<Notice>,
    live_activity: Vec<ProcessActivity>,
    updated_at: Instant,
    last_mouse_click: Option<(View, String, Instant)>,
}
impl App {
    pub fn new(snapshot: Snapshot) -> Self {
        let mut app = Self {
            live_activity: snapshot.activity.clone(),
            snapshot,
            view: View::Activity,
            popup: None,
            busy: false,
            paused: false,
            searching: false,
            filters: Default::default(),
            activity_sort: Default::default(),
            selection: Default::default(),
            expanded: HashSet::new(),
            chart: VecDeque::new(),
            notice: None,
            updated_at: Instant::now(),
            last_mouse_click: None,
        };
        app.reconcile();
        app
    }
    pub fn update(&mut self, mut snapshot: Snapshot, mutation: bool) {
        self.live_activity = snapshot.activity.clone();
        if self.paused {
            snapshot.activity = self.snapshot.activity.clone();
        } else {
            self.chart
                .push_back(snapshot.activity.iter().fold((0u64, 0u64), |(a, b), p| {
                    (a.saturating_add(p.rate_in), b.saturating_add(p.rate_out))
                }));
            if self.chart.len() > 48 {
                self.chart.pop_front();
            }
        }
        self.snapshot = snapshot;
        self.updated_at = Instant::now();
        if mutation {
            self.busy = false;
            self.notify(
                if self.snapshot.demo {
                    "Demo state updated"
                } else {
                    "Change verified against macOS"
                }
                .into(),
                false,
            );
        }
        self.reconcile();
    }
    pub fn failed(&mut self, text: String, mutation: bool) {
        if mutation {
            self.busy = false;
        } else {
            self.snapshot.notices = vec![clean(&text)];
            self.snapshot.firewall = None;
        }
        self.notify(text, true);
    }
    pub fn geoip_updated(&mut self, snapshot: Snapshot) {
        self.update(snapshot, false);
        self.busy = false;
        self.notify(
            if self.snapshot.demo {
                "Demo country update simulated; no download or files changed"
            } else {
                "Country database updated; lookups remain offline"
            }
            .into(),
            false,
        );
    }
    pub fn notify(&mut self, text: String, error: bool) {
        self.notice = Some(Notice {
            text: clean(&text),
            error,
            at: Instant::now(),
        });
    }
    pub fn stale(&self) -> bool {
        self.updated_at.elapsed().as_secs() > 5
    }
    pub fn incoming(&self, process: &ProcessActivity) -> crate::permissions::Resolution {
        crate::permissions::Index::new(&self.snapshot).activity(process)
    }
    pub fn filter(&self) -> &str {
        &self.filters[self.view.index()]
    }
}
