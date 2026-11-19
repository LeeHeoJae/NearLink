use crate::i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedPeer {
    pub address: u64,
    pub name: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub random_address: bool,
    #[serde(default)]
    pub device_id: String,
}

fn enabled_default() -> bool {
    true
}

impl SavedPeer {
    pub fn new(address: u64, name: String, random_address: bool) -> Self {
        Self {
            address,
            name,
            alias: String::new(),
            enabled: true,
            random_address,
            device_id: String::new(),
        }
    }
    pub fn label(&self) -> &str {
        if self.alias.is_empty() {
            &self.name
        } else {
            &self.alias
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u8,
    pub peers: Vec<SavedPeer>,
    pub notifications: bool,
    pub low_threshold: u8,
    pub full_notifications: bool,
    /// None keeps the neutral app icon; Some shows a chosen peer's fresh level.
    pub tray_peer: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            peers: vec![],
            notifications: true,
            low_threshold: 20,
            full_notifications: true,
            tray_peer: None,
        }
    }
}

pub fn directory() -> Result<PathBuf, String> {
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--data-dir") {
        return args
            .get(index + 1)
            .map(PathBuf::from)
            .ok_or(t("--data-dir needs a path.").into());
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("NearLink"))
        .ok_or(t("Couldn't find the user settings folder.").into())
}

pub fn load(directory: &Path) -> Result<Settings, String> {
    let text = match fs::read_to_string(directory.join("settings.json")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(tf("Couldn't read settings: {0}", &[&error])),
    };
    let settings: Settings =
        serde_json::from_str(&text).map_err(|e| tf("Check the settings file: {0}", &[&e]))?;
    if settings.version != 1
        || settings.low_threshold == 0
        || settings.low_threshold > 90
        || settings.peers.len() > 16
    {
        return Err(t("Unsupported settings format. Check settings.json.").into());
    }
    let mut addresses = std::collections::HashSet::new();
    for peer in &settings.peers {
        if peer.address == 0 || peer.address >= (1 << 48) || !addresses.insert(peer.address) {
            return Err(t("The settings contain an invalid or duplicate device address.").into());
        }
    }
    Ok(settings)
}

pub fn save(directory: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let temporary = directory.join("settings.tmp");
    let target = directory.join("settings.json");
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    use std::os::windows::ffi::OsStrExt;
    let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|e| tf("Couldn't save settings: {0}", &[&e]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_roundtrip_replaces_existing_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!("nearlink-test-{}", std::process::id()));
        let settings = Settings::default();
        save(&root, &settings).unwrap();
        let mut changed = load(&root).unwrap();
        assert_eq!(changed.tray_peer, None);
        changed.tray_peer = Some(7);
        changed.peers.push(SavedPeer {
            address: 7,
            name: "휴대폰".into(),
            alias: "업무용".into(),
            enabled: false,
            random_address: false,
            device_id: String::new(),
        });
        save(&root, &changed).unwrap();
        assert_eq!(load(&root).unwrap().peers[0].label(), "업무용");
        assert!(!load(&root).unwrap().peers[0].enabled);
        assert_eq!(load(&root).unwrap().tray_peer, Some(7));
        changed.peers.push(changed.peers[0].clone());
        save(&root, &changed).unwrap();
        assert!(load(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
