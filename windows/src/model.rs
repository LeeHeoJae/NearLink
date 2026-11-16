use crate::config::{SavedPeer, Settings};
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
            Self::Connecting => "연결 중",
            Self::Reading => "배터리 확인 중",
            Self::Connected => "연결됨",
            Self::Retrying => "재연결 대기",
            Self::Stopped => "중지됨",
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

/// "N초/분/시간 전", then "어제" and "N일 전" so old values never read as hours.
pub fn age_label(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}초 전"),
        60..3600 => format!("{}분 전", seconds / 60),
        3600..86_400 => format!("{}시간 전", seconds / 3600),
        86_400..172_800 => "어제".into(),
        _ => format!("{}일 전", seconds / 86_400),
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
            .unwrap_or_else(|| "확인 전".into())
    }
    pub fn charge_text(&self) -> &'static str {
        match (self.charging, self.plugged) {
            (Some(true), _) => "충전 중",
            (Some(false), Some(true)) => "전원 연결됨",
            (Some(false), _) => "충전 중 아님",
            _ => "확인 전",
        }
    }
    pub fn age_text(&self) -> String {
        age_seconds(self.updated).map_or_else(|| "수신 전".into(), age_label)
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
            State::Retrying => "연결 끊김 · 재연결 대기",
            State::Stopped => "수신 중지됨",
            State::Connected => "연결됨 · 배터리 수신 중",
            _ => self.state.text(),
        };
        if self.stale() {
            format!("{state} · 오래된 값")
        } else if self.updated.is_some() && self.state != State::Connected {
            format!("{state} · 마지막 값")
        } else {
            state.into()
        }
    }
    pub fn accessible_text(&self) -> String {
        format!(
            "{} · {} · {} · {} · 마지막 수신 {}",
            self.saved.label(),
            self.battery_text(),
            self.charge_text(),
            self.status_text(),
            self.age_text()
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
                return Some("배터리가 부족합니다.");
            }
        }
        if level == 100 && !self.full {
            self.full = true;
            if settings.full_notifications {
                return Some("배터리가 완전히 충전되었습니다.");
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
                .push_back((peer.saved.label().into(), format!("{text} 현재 {level}%")));
        }
    }
}

/// Joins whole lines within `limit` UTF-16 units (Win32 buffers exclude the NUL).
/// Lines that do not fit are summarized as "외 N{unit}" rather than cut mid-line.
pub fn fit_lines(header: &str, lines: &[String], unit: &str, limit: usize) -> String {
    let len = |s: &str| s.encode_utf16().count();
    let separator = |text: &str| if text.is_empty() { "" } else { "\n" };
    let summary = |text: &str, count: usize| format!("{}외 {count}{unit}", separator(text));
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
                format!("NearLink · 배터리 알림 {}건", notices.len()),
                fit_lines("", &lines, "건", 255),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freshness_is_distinct_from_connection_and_always_exposed_as_text() {
        let mut peer = Peer::new(SavedPeer::new(1, "태블릿".into(), false));
        assert!(!peer.stale());
        assert!(peer.accessible_text().contains("수신 전"));
        peer.level = Some(73);
        peer.state = State::Connected;
        peer.updated = Some(SystemTime::now() - std::time::Duration::from_secs(901));
        assert!(peer.status_text().contains("오래된 값"));
        assert!(peer.status_text().contains("연결됨"));
        peer.state = State::Retrying;
        assert!(peer.status_text().contains("연결 끊김"));
        peer.updated = Some(SystemTime::now());
        assert!(peer.status_text().contains("마지막 값"));
        peer.state = State::Stopped;
        assert!(peer.accessible_text().contains("수신 중지됨"));
    }
    #[test]
    fn recent_session_check_keeps_an_unchanged_reading_current() {
        let mut peer = Peer::new(SavedPeer::new(1, "태블릿".into(), false));
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
        settings.peers.push(SavedPeer::new(7, "휴대폰".into(), false));
        let mut model = Model::new(settings);
        model.verified(7);
        assert!(model.peers[&7].verified.is_none());
        model.peers.get_mut(&7).unwrap().state = State::Connected;
        model.verified(7);
        assert!(model.peers[&7].verified.is_some());
    }
    #[test]
    fn ages_switch_to_days_after_a_day() {
        assert_eq!(age_label(59), "59초 전");
        assert_eq!(age_label(3599), "59분 전");
        assert_eq!(age_label(86_399), "23시간 전");
        assert_eq!(age_label(86_400), "어제");
        assert_eq!(age_label(3 * 86_400), "3일 전");
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
            .push(SavedPeer::new(7, "휴대폰".into(), false));
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
        let lines: Vec<String> = (0..4).map(|i| format!("기기{i} 50% · 연결됨")).collect();
        let text = fit_lines("NearLink", &lines, "대", 41);
        assert_eq!(text.encode_utf16().count(), 41);
        assert_eq!(text, "NearLink\n기기0 50% · 연결됨\n기기1 50% · 연결됨\n외 2대");
        assert_eq!(
            fit_lines("NearLink", &lines[..1], "대", 40),
            "NearLink\n기기0 50% · 연결됨"
        );
    }

    #[test]
    fn simultaneous_notices_share_one_balloon() {
        let notices = vec![
            ("휴대폰".to_string(), "배터리가 부족합니다. 현재 15%".to_string()),
            ("태블릿".to_string(), "배터리가 완전히 충전되었습니다. 현재 100%".to_string()),
        ];
        let (title, message) = notice_balloon(&notices).unwrap();
        assert_eq!(title, "NearLink · 배터리 알림 2건");
        assert!(message.contains("휴대폰: ") && message.contains("태블릿: "));
        assert_eq!(notice_balloon(&notices[..1]).unwrap().0, "NearLink · 휴대폰");
        assert!(notice_balloon(&[]).is_none());
    }
}
