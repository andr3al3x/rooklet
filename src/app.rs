//! Application state and the public interaction API.
mod activity_query;
mod dialog;
mod editor;
mod input;
mod mouse;
mod permissions;
mod profiles;
mod rules;
mod selection;
mod termination;

pub use activity_query::ActivitySort;
pub use dialog::{ConfirmedAction, NetworkDraft, Popup};
pub use mouse::MouseAction;
pub use profiles::{ProfileOperation, ProfileOutcome};
use rooklet_core::text::clean;
pub use rules::RuleProbe;
pub use selection::{ActivityRow, process_key};

use rooklet_core::model::{Mutation, NetworkStatus, ProcessActivity, Setting, Snapshot};
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
    pub terminate: Option<rooklet_core::process::TerminationRequest>,
    pub profile: Option<ProfileOperation>,
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
    pub resources_visible: bool,
    pub selection: [Option<String>; 4],
    pub expanded: HashSet<String>,
    pub chart: VecDeque<(u64, u64)>,
    pub notice: Option<Notice>,
    live_activity: Vec<ProcessActivity>,
    live_resources: rooklet_core::resources::Resources,
    live_paths: rooklet_core::permissions::Paths,
    updated_at: Option<Instant>,
    last_mouse_click: Option<(View, String, Instant)>,
    pending_profile: Option<profiles::Pending>,
}
impl App {
    pub fn new(snapshot: Snapshot) -> Self {
        let mut app = Self {
            live_activity: snapshot.activity.clone(),
            live_resources: snapshot.resources.clone(),
            live_paths: snapshot.permission_paths.clone(),
            snapshot,
            view: View::Activity,
            popup: None,
            busy: false,
            paused: false,
            searching: false,
            filters: Default::default(),
            activity_sort: Default::default(),
            resources_visible: true,
            selection: Default::default(),
            expanded: HashSet::new(),
            chart: VecDeque::new(),
            notice: None,
            updated_at: Some(Instant::now()),
            last_mouse_click: None,
            pending_profile: None,
        };
        app.reconcile();
        app
    }
    pub fn update(&mut self, mut snapshot: Snapshot, mutation: bool) {
        self.live_activity = snapshot.activity.clone();
        self.live_resources = snapshot.resources.clone();
        self.live_paths = snapshot.permission_paths.clone();
        if self.paused {
            snapshot
                .permission_paths
                .preserve_activity(&self.snapshot.permission_paths);
            snapshot.activity = self.snapshot.activity.clone();
            snapshot.resources = self.snapshot.resources.clone();
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
        self.prune_expanded();
        self.updated_at = Some(Instant::now());
        if mutation {
            self.busy = false;
            self.notify("Change verified against macOS".into(), false);
        }
        self.reconcile();
    }
    pub fn operation_failed(&mut self, text: String) {
        self.busy = false;
        self.pending_profile = None;
        if let Some(Popup::Profiles { loading, .. }) = &mut self.popup {
            *loading = false;
        }
        self.notify(text, true);
    }
    pub fn observation_failed(&mut self, text: String) {
        self.invalidate_observation(&text);
        self.notify(text, true);
    }
    pub fn geoip_updated(&mut self, snapshot: Snapshot) {
        self.update(snapshot, false);
        self.busy = false;
        self.notify(
            "Country database updated; lookups remain offline".into(),
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
        self.updated_at.is_none_or(|at| at.elapsed().as_secs() > 5)
    }
    pub fn invalidate_observation(&mut self, reason: &str) {
        self.updated_at = None;
        self.snapshot.notices = vec![clean(reason)];
        self.snapshot.firewall = None;
        self.snapshot.control_age_ms = None;
        self.snapshot.applications_available = false;
        self.snapshot.applications.clear();
        self.snapshot.permission_paths = Default::default();
        self.snapshot.network = NetworkStatus {
            message: Some(clean(reason)),
            ..Default::default()
        };
        self.live_activity.clear();
        self.live_resources = Default::default();
        self.snapshot.resources = Default::default();
        self.live_paths = Default::default();
        for process in &mut self.snapshot.activity {
            process.rate_in = 0;
            process.rate_out = 0;
            process.identities.clear();
        }
    }
    pub fn incoming(&self, process: &ProcessActivity) -> rooklet_core::permissions::Resolution {
        rooklet_core::permissions::Index::new(&self.snapshot).activity(process)
    }
    pub fn filter(&self) -> &str {
        &self.filters[self.view.index()]
    }
}
