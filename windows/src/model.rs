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
    pub detail: String,
    pub alerts: Alerts,
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
        let Some(time) = self.updated else {
            return "수신 전".into();
        };
        let seconds = time.elapsed().unwrap_or_default().as_secs();
        if seconds < 60 {
            format!("{seconds}초 전")
        } else if seconds < 3600 {
            format!("{}분 전", seconds / 60)
        } else {
            format!("{}시간 전", seconds / 3600)
        }
    }

    /// Battery notifications may be quiet while the value is unchanged. Age is
    /// a data-freshness hint, never evidence that the BLE connection has failed.
    pub fn stale(&self) -> bool {
        self.updated
            .is_some_and(|t| t.elapsed().unwrap_or_default().as_secs() >= 900)
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
        if level <= settings.low_threshold && charging == Some(false) && !self.low {
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
}
