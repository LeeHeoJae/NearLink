use crate::{
    config::{SavedPeer, Settings},
    i18n::{t, tf, tn},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub type Shared = Arc<Mutex<Model>>;

#[derive(Clone, Debug)]
pub struct Discovered {
    pub address: u64,
    pub name: String,
    pub random_address: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Connecting,
    Reading,
    Connected,
    Retrying,
    Stopped,
}

impl State {
    pub fn text(self) -> &'static str {
        match self {
            Self::Connecting => t("Connecting"),
            Self::Reading => t("Reading battery"),
            Self::Connected => t("Connected"),
            Self::Retrying => t("Waiting to reconnect"),
            Self::Stopped => t("Stopped"),
        }
    }
}

pub struct Peer {
    pub saved: SavedPeer,
    pub state: State,
    pub level: Option<u8>,
    pub charging: Option<bool>,
    pub plugged: Option<bool>,
    pub updated: Option<SystemTime>,
    /// Last successful periodic session check while connected. Android notifies only on
    /// change, so a recent check means an unchanged reading is still current.
    pub verified: Option<SystemTime>,
    pub detail: String,
    pub alerts: Alerts,
}

const STALE_SECONDS: u64 = 900;

fn age_seconds(time: Option<SystemTime>) -> Option<u64> {
    time.map(|t| t.elapsed().unwrap_or_default().as_secs())
}

/// "N sec/min/hr ago", then "Yesterday" and "N days ago" so old values never read as hours.
pub fn age_label(seconds: u64) -> String {
    match seconds {
        0..60 => tf("{0} sec ago", &[&seconds]),
        60..3600 => tf("{0} min ago", &[&(seconds / 60)]),
        3600..86_400 => tf("{0} hr ago", &[&(seconds / 3600)]),
        86_400..172_800 => t("Yesterday").into(),
        _ => tf("{0} days ago", &[&(seconds / 86_400)]),
    }
}

impl Peer {
    pub fn new(saved: SavedPeer) -> Self {
        let state = if saved.enabled {
            State::Connecting
        } else {
            State::Stopped
        };
        Self {
            saved,
            state,
            level: None,
            charging: None,
            plugged: None,
            updated: None,
            verified: None,
            detail: String::new(),
            alerts: Alerts::default(),
        }
    }
    pub fn battery_text(&self) -> String {
        self.level
            .map(|v| format!("{v}%"))
            .unwrap_or_else(|| t("Unknown").into())
    }
    pub fn charge_text(&self) -> &'static str {
        match (self.charging, self.plugged) {
            (Some(true), _) => t("Charging"),
            (Some(false), Some(true)) => t("Plugged in"),
            (Some(false), _) => t("Not charging"),
            _ => t("Unknown"),
        }
    }
    pub fn age_text(&self) -> String {
        age_seconds(self.updated).map_or_else(|| t("Not received yet").into(), age_label)
    }

    /// Battery notifications are quiet while the value is unchanged. Age alone is a
    /// data-freshness hint, never evidence that the BLE connection has failed, and a
    /// recent session check on a live connection keeps an unchanged value current.
    pub fn stale(&self) -> bool {
        let Some(data) = age_seconds(self.updated) else {
            return false;
        };
        data >= STALE_SECONDS
            && !(self.state == State::Connected
                && age_seconds(self.verified).is_some_and(|age| age < STALE_SECONDS))
    }

    /// Whether an old reception time is explained by a confirmed, unchanged connection.
    pub fn confirmed_unchanged(&self) -> bool {
        age_seconds(self.updated).is_some_and(|age| age >= STALE_SECONDS) && !self.stale()
    }
    pub fn status_text(&self) -> String {
        let state = match self.state {
            State::Retrying => t("Disconnected · Waiting to reconnect"),
            State::Stopped => t("Receiving stopped"),
            State::Connected => t("Connected · Receiving battery"),
            _ => self.state.text(),
        };
        if self.stale() {
            tf("{0} · Outdated value", &[&state])
        } else if self.updated.is_some() && self.state != State::Connected {
            tf("{0} · Last value", &[&state])
        } else {
            state.into()
        }
    }
    /// Compact second line of a device row: connection, then how current the value is.
    pub fn row_status(&self) -> String {
        let state = match self.state {
            State::Retrying => t("Disconnected · Waiting to reconnect"),
            State::Stopped => t("Receiving stopped"),
            _ => self.state.text(),
        };
        let age = self.age_text();
        if self.updated.is_none() {
            format!("{state} · {age}")
        } else if self.stale() {
            tf("{0} · Outdated value {1}", &[&state, &age])
        } else if self.state != State::Connected {
            tf("{0} · Last value {1}", &[&state, &age])
        } else {
            format!("{state} · {age}")
        }
    }

    /// The value may not reflect the device now; rows mute the reading and say why in text.
    pub fn muted(&self) -> bool {
        self.updated.is_none() || self.stale() || self.state != State::Connected
    }

    /// Lost connection or old data, highlighted next to its text.
    pub fn needs_attention(&self) -> bool {
        self.stale() || self.state == State::Retrying
    }

    /// Segoe Fluent Icons / MDL2 Assets mobile battery glyph in 10% steps.
    pub fn battery_glyph(&self) -> char {
        let charging = self.charging == Some(true);
        let code = match self.level {
            None if charging => 0xEB17,
            None => 0xE996,
            Some(level) => {
                let step = (u32::from(level.min(100)) + 5) / 10;
                (if charging { 0xEBAB } else { 0xEBA0 }) + step
            }
        };
        char::from_u32(code).unwrap_or('?')
    }

    /// Short visible charging label; "not charging" is left to the accessible text.
    pub fn charge_badge(&self) -> &'static str {
        match (self.charging, self.plugged) {
            (Some(true), _) => t("Charging"),
            (Some(false), Some(true)) => t("Plugged in"),
            _ => "",
        }
    }

    pub fn accessible_text(&self) -> String {
        tf(
            "{0} · {1} · {2} · {3} · Last received {4}",
            &[
                &self.saved.label(),
                &self.battery_text(),
                &self.charge_text(),
                &self.status_text(),
                &self.age_text(),
            ],
        )
    }
}

#[derive(Default)]
pub struct Alerts {
    low: bool,
    full: bool,
}

impl Alerts {
    pub fn update(
        &mut self,
        level: u8,
        charging: Option<bool>,
        settings: &Settings,
    ) -> Option<&'static str> {
        if level > settings.low_threshold.saturating_add(5) {
            self.low = false;
        }
        if level < 95 {
            self.full = false;
        }
        // Unknown charging state still warns once; only a confirmed charge suppresses it.
        if level <= settings.low_threshold && charging != Some(true) && !self.low {
            self.low = true;
            if settings.notifications {
                return Some(t("Battery is low."));
            }
        }
        if level == 100 && !self.full {
            self.full = true;
            if settings.full_notifications {
                return Some(t("Battery is fully charged."));
            }
        }
        None
    }
}

pub struct Model {
    pub settings: Settings,
    pub peers: BTreeMap<u64, Peer>,
    pub discovered: Vec<Discovered>,
    pub scanning: bool,
    pub message: String,
    pub notices: VecDeque<(String, String)>,
    pub dirty: bool,
}

impl Model {
    pub fn new(settings: Settings) -> Self {
        let peers = settings
            .peers
            .iter()
            .cloned()
            .map(|p| (p.address, Peer::new(p)))
            .collect();
        Self {
            settings,
            peers,
            discovered: vec![],
            scanning: false,
            message: String::new(),
            notices: VecDeque::new(),
            dirty: false,
        }
    }

    /// Records a successful periodic session check for a connected device.
    pub fn verified(&mut self, address: u64) {
        if let Some(peer) = self.peers.get_mut(&address)
            && peer.saved.enabled
            && peer.state == State::Connected
        {
            peer.verified = Some(SystemTime::now());
        }
    }

    pub fn reading(
        &mut self,
        address: u64,
        level: Option<u8>,
        charging: Option<bool>,
        plugged: Option<bool>,
        ready: bool,
    ) {
        let Some(peer) = self.peers.get_mut(&address) else {
            return;
        };
        if !peer.saved.enabled {
            return;
        }
        if let Some(level) = level {
            peer.level = Some(level);
        }
        peer.charging = charging;
        peer.plugged = plugged;
        if level.is_some() {
            peer.updated = Some(SystemTime::now());
        }
        if ready {
            peer.state = State::Connected;
            peer.detail.clear();
        }
        if ready
            && let Some(level) = peer.level
            && let Some(text) = peer.alerts.update(level, charging, &self.settings)
        {
            self.notices
                .push_back((peer.saved.label().into(), tf("{0} Currently {1}%", &[&text, &level])));
        }
    }
}

/// Joins whole lines within `limit` UTF-16 units (Win32 buffers exclude the NUL).
/// Lines that do not fit are summarized by `more` (for example "and 2 more") rather than cut mid-line.
pub fn fit_lines(header: &str, lines: &[String], more: &dyn Fn(usize) -> String, limit: usize) -> String {
    let len = |s: &str| s.encode_utf16().count();
    let separator = |text: &str| if text.is_empty() { "" } else { "\n" };
    let summary = |text: &str, count: usize| format!("{}{}", separator(text), more(count));
    let mut text = header.to_string();
    for (index, line) in lines.iter().enumerate() {
        let remaining = lines.len() - index - 1;
        let reserve = if remaining > 0 { len(&summary("x", remaining)) } else { 0 };
        if len(&text) + len(separator(&text)) + len(line) + reserve <= limit {
            text = format!("{text}{}{line}", separator(&text));
        } else {
            let more = summary(&text, lines.len() - index);
            if len(&text) + len(&more) <= limit {
                text.push_str(&more);
            }
            break;
        }
    }
    text
}

/// Several alerts raised together become one balloon so none is silently replaced.
pub fn notice_balloon(notices: &[(String, String)]) -> Option<(String, String)> {
    match notices {
        [] => None,
        [(title, message)] => Some((format!("NearLink · {title}"), message.clone())),
        _ => {
            let lines: Vec<_> = notices
                .iter()
                .map(|(title, message)| format!("{title}: {message}"))
                .collect();
            Some((
                tn(
                    "NearLink · {0} battery alert",
                    "NearLink · {0} battery alerts",
                    notices.len(),
                ),
                fit_lines(
                    "",
                    &lines,
                    &|n| tn("and {0} more alert", "and {0} more alerts", n),
                    255,
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, with_lang};
    #[test]
    fn freshness_is_distinct_from_connection_and_always_exposed_as_text() {
        let mut peer = Peer::new(SavedPeer::new(1, "Tablet".into(), false));
        assert!(!peer.stale());
        assert!(peer.accessible_text().contains(t("Not received yet")));
        peer.level = Some(73);
        peer.state = State::Connected;
        peer.updated = Some(SystemTime::now() - std::time::Duration::from_secs(901));
        assert!(peer.status_text().contains("Outdated value"));
        assert!(peer.status_text().contains(t("Connected")));
        peer.state = State::Retrying;
        assert!(peer.status_text().contains("Disconnected"));
        peer.updated = Some(SystemTime::now());
        assert!(peer.status_text().contains("Last value"));
        peer.state = State::Stopped;
        assert!(peer.accessible_text().contains(t("Receiving stopped")));
    }
    #[test]
    fn row_status_is_compact_but_keeps_freshness_explicit() {
        let mut peer = Peer::new(SavedPeer::new(1, "Tablet".into(), false));
        assert_eq!(peer.row_status(), "Connecting · Not received yet");
        assert!(peer.muted());
        peer.level = Some(73);
        peer.state = State::Connected;
        peer.updated = Some(SystemTime::now());
        assert!(peer.row_status().starts_with("Connected · "));
        assert!(!peer.muted() && !peer.needs_attention());
        peer.updated = Some(SystemTime::now() - std::time::Duration::from_secs(901));
        assert!(peer.row_status().contains("Outdated value"));
        assert!(peer.needs_attention());
        peer.state = State::Retrying;
        peer.updated = Some(SystemTime::now());
        assert!(peer.row_status().contains("Disconnected"));
        assert!(peer.row_status().contains("Last value"));
        assert!(peer.muted() && peer.needs_attention());
    }
    #[test]
    fn battery_glyph_follows_level_and_charging() {
        let mut peer = Peer::new(SavedPeer::new(1, "Tablet".into(), false));
        assert_eq!(peer.battery_glyph(), '\u{E996}');
        peer.level = Some(0);
        assert_eq!(peer.battery_glyph(), '\u{EBA0}');
        peer.level = Some(73);
        assert_eq!(peer.battery_glyph(), '\u{EBA7}');
        peer.level = Some(100);
        assert_eq!(peer.battery_glyph(), '\u{EBAA}');
        peer.charging = Some(true);
        assert_eq!(peer.battery_glyph(), '\u{EBB5}');
        assert_eq!(peer.charge_badge(), t("Charging"));
        peer.charging = Some(false);
        assert_eq!(peer.charge_badge(), "");
    }
    #[test]
    fn recent_session_check_keeps_an_unchanged_reading_current() {
        let mut peer = Peer::new(SavedPeer::new(1, "Tablet".into(), false));
        peer.level = Some(73);
        peer.state = State::Connected;
        peer.updated = Some(SystemTime::now() - std::time::Duration::from_secs(3600));
        assert!(peer.stale());
        peer.verified = Some(SystemTime::now());
        assert!(!peer.stale());
        assert!(peer.confirmed_unchanged());
        // A check from a connection that has since dropped proves nothing.
        peer.state = State::Retrying;
        assert!(peer.stale());
        assert!(!peer.confirmed_unchanged());
    }
    #[test]
    fn verification_only_applies_to_connected_devices() {
        let mut settings = Settings::default();
        settings.peers.push(SavedPeer::new(7, "Phone".into(), false));
        let mut model = Model::new(settings);
        model.verified(7);
        assert!(model.peers[&7].verified.is_none());
        model.peers.get_mut(&7).unwrap().state = State::Connected;
        model.verified(7);
        assert!(model.peers[&7].verified.is_some());
    }
    #[test]
    fn ages_switch_to_days_after_a_day() {
        assert_eq!(age_label(59), "59 sec ago");
        assert_eq!(age_label(3599), "59 min ago");
        assert_eq!(age_label(86_399), "23 hr ago");
        assert_eq!(age_label(86_400), t("Yesterday"));
        assert_eq!(age_label(3 * 86_400), "3 days ago");
        with_lang(Lang::Ko, || {
            assert_eq!(age_label(59), "59초 전");
            assert_eq!(age_label(86_400), "어제");
            assert_eq!(age_label(3 * 86_400), "3일 전");
        });
    }
    #[test]
    fn alerts_do_not_repeat_and_rearm_with_hysteresis() {
        let mut alerts = Alerts::default();
        let settings = Settings::default();
        assert!(alerts.update(19, Some(true), &settings).is_none());
        assert!(alerts.update(19, Some(false), &settings).is_some());
        assert!(alerts.update(18, Some(false), &settings).is_none());
        assert!(alerts.update(24, Some(false), &settings).is_none());
        alerts.update(26, Some(false), &settings);
        assert!(alerts.update(20, Some(false), &settings).is_some());
        assert!(alerts.update(100, Some(false), &settings).is_some());
        assert!(alerts.update(100, Some(false), &settings).is_none());
        alerts.update(94, Some(false), &settings);
        assert!(alerts.update(100, Some(false), &settings).is_some());
    }

    #[test]
    fn initial_reads_do_not_claim_connection_or_raise_alerts_and_stop_ignores_late_data() {
        let mut settings = Settings::default();
        settings
            .peers
            .push(SavedPeer::new(7, "Phone".into(), false));
        let mut model = Model::new(settings);
        model.reading(7, Some(15), Some(false), Some(false), false);
        assert_eq!(model.peers[&7].state, State::Connecting);
        assert!(model.notices.is_empty());
        model.reading(7, Some(14), Some(false), Some(false), true);
        assert_eq!(model.peers[&7].state, State::Connected);
        assert_eq!(model.notices.len(), 1);
        model.peers.get_mut(&7).unwrap().saved.enabled = false;
        model.peers.get_mut(&7).unwrap().state = State::Stopped;
        model.reading(7, Some(13), Some(false), Some(false), true);
        assert_eq!(model.peers[&7].state, State::Stopped);
        assert_eq!(model.peers[&7].level, Some(14));
    }

    #[test]
    fn full_notifications_are_independent_of_low_notifications() {
        let settings = Settings {
            notifications: false,
            ..Settings::default()
        };
        let mut alerts = Alerts::default();
        assert!(alerts.update(10, Some(false), &settings).is_none());
        assert!(alerts.update(100, Some(false), &settings).is_some());
    }

    #[test]
    fn low_alert_fires_when_charging_state_is_unknown() {
        let settings = Settings::default();
        let mut alerts = Alerts::default();
        assert!(alerts.update(10, None, &settings).is_some());
        assert!(alerts.update(9, None, &settings).is_none());
    }

    #[test]
    fn fit_lines_keeps_whole_lines_and_summarizes_the_rest() {
        let lines: Vec<String> = (0..4).map(|i| format!("Dev{i} 50% · Connected")).collect();
        let more = |n| tn("and {0} more alert", "and {0} more alerts", n);
        let text = fit_lines("NearLink", &lines, &more, 68);
        assert_eq!(text.encode_utf16().count(), 68);
        assert_eq!(
            text,
            "NearLink\nDev0 50% · Connected\nDev1 50% · Connected\nand 2 more alerts"
        );
        assert_eq!(
            fit_lines("NearLink", &lines[..1], &more, 40),
            "NearLink\nDev0 50% · Connected"
        );
        with_lang(Lang::Ko, || {
            let lines: Vec<String> = (0..4).map(|i| format!("기기{i} 50% · 연결됨")).collect();
            let text = fit_lines("NearLink", &lines, &more, 41);
            assert_eq!(text, "NearLink\n기기0 50% · 연결됨\n기기1 50% · 연결됨\n외 2건");
        });
    }

    #[test]
    fn simultaneous_notices_share_one_balloon() {
        let notices = vec![
            ("Phone".to_string(), "Battery is low. Currently 15%".to_string()),
            ("Tablet".to_string(), "Battery is fully charged. Currently 100%".to_string()),
        ];
        let (title, message) = notice_balloon(&notices).unwrap();
        assert_eq!(title, "NearLink · 2 battery alerts");
        assert!(message.contains("Phone: ") && message.contains("Tablet: "));
        assert_eq!(notice_balloon(&notices[..1]).unwrap().0, "NearLink · Phone");
        with_lang(Lang::Ko, || {
            assert_eq!(notice_balloon(&notices).unwrap().0, "NearLink · 배터리 알림 2건");
        });
        assert!(notice_balloon(&[]).is_none());
    }
}
