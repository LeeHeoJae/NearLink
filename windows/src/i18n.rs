//! Display-language support. English is the source language: code passes the English text and
//! the Korean table below supplies the translation. `--lang en|ko` or `NEARLINK_LANG` overrides
//! the Windows display language, which is otherwise used to choose the language.
use std::{
    cell::Cell,
    collections::HashMap,
    fmt::Display,
    sync::{Mutex, OnceLock},
};
use windows::{Win32::Globalization::GetUserDefaultUILanguage, core::PCWSTR};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lang {
    En,
    Ko,
}

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

fn parse(value: &str) -> Option<Lang> {
    let value = value.trim().to_ascii_lowercase();
    match value.split(['-', '_']).next()? {
        "ko" => Some(Lang::Ko),
        "en" => Some(Lang::En),
        _ => None,
    }
}

fn detect() -> Lang {
    // Unit tests expect the source language unless they ask for another one.
    if cfg!(test) {
        return Lang::En;
    }
    let args: Vec<String> = std::env::args().collect();
    let requested = args
        .iter()
        .position(|arg| arg == "--lang")
        .and_then(|index| args.get(index + 1).cloned())
        .or_else(|| std::env::var("NEARLINK_LANG").ok());
    if let Some(lang) = requested.as_deref().and_then(parse) {
        return lang;
    }
    // The primary language identifier of Korean (LANG_KOREAN) is 0x12.
    let id = unsafe { GetUserDefaultUILanguage() };
    if id & 0x3ff == 0x12 { Lang::Ko } else { Lang::En }
}

pub fn lang() -> Lang {
    if let Some(lang) = OVERRIDE.with(Cell::get) {
        return lang;
    }
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(detect)
}

/// Runs `f` with another language on this thread only, so parallel tests do not interfere.
#[cfg(test)]
pub fn with_lang<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
    let old = OVERRIDE.with(|cell| cell.replace(Some(lang)));
    let result = f();
    OVERRIDE.with(|cell| cell.set(old));
    result
}

fn korean(key: &str) -> Option<&'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| KO.iter().copied().collect()).get(key).copied()
}

/// Translates a message. Unknown keys fall back to the English source text.
pub fn t(key: &'static str) -> &'static str {
    match lang() {
        Lang::En => key,
        Lang::Ko => korean(key).unwrap_or(key),
    }
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut text = template.to_string();
    for (index, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), &arg.to_string());
    }
    text
}

/// Translates a message and fills `{0}`, `{1}`, … so word order may differ between languages.
pub fn tf(key: &'static str, args: &[&dyn Display]) -> String {
    fill(t(key), args)
}

/// Picks the singular form for one item in English; Korean has a single form keyed by `other`.
pub fn tn(one: &'static str, other: &'static str, count: usize) -> String {
    let key = if count == 1 && lang() == Lang::En { one } else { other };
    fill(t(key), &[&count])
}

/// A NUL-terminated UTF-16 copy of the translated message. Each distinct message is kept for the
/// life of the process, so the pointer stays valid for menus and dialogs.
pub fn pw(key: &'static str) -> PCWSTR {
    type Cache = Mutex<HashMap<(Lang, &'static str), &'static [u16]>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let lang = lang();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    let wide = *cache.entry((lang, key)).or_insert_with(|| {
        let units: Vec<u16> = t(key).encode_utf16().chain(Some(0)).collect();
        Box::leak(units.into_boxed_slice())
    });
    PCWSTR(wide.as_ptr())
}

pub const KO: &[(&str, &str)] = &[
    ("Pairing was canceled.", "페어링을 취소했어요."),
    ("Pairing confirmation timed out. Check the request on both devices.", "페어링 확인 시간이 지났어요. 양쪽 기기의 요청을 확인해 주세요."),
    ("The pairing request was rejected.", "페어링 요청이 거절됐어요."),
    ("Windows didn't allow pairing. Check the Bluetooth permissions.", "Windows에서 페어링을 허용하지 않았어요. 블루투스 권한을 확인해 주세요."),
    ("The device isn't ready to pair. On Android, tap Prepare to connect.", "기기가 페어링할 준비가 되지 않았어요. Android에서 연결 준비를 눌러 주세요."),
    ("Pairing authentication failed. Check the existing pairing records on both devices.", "페어링 인증에 실패했어요. 양쪽 기기의 기존 페어링 기록을 확인해 주세요."),
    ("Couldn't set up encrypted pairing.", "암호화된 페어링을 설정하지 못했어요."),
    ("Another pairing request is in progress. Finish it, then try again.", "다른 페어링 요청이 진행 중이에요. 요청을 완료한 뒤 다시 시도해 주세요."),
    ("Windows couldn't open the pairing confirmation window.", "Windows에서 페어링 확인 창을 시작하지 못했어요."),
    ("Couldn't complete pairing.", "페어링을 완료하지 못했어요."),
    ("Device not responding (Unreachable)", "기기 응답 없음 (Unreachable)"),
    ("Access denied (AccessDenied)", "접근 거부 (AccessDenied)"),
    ("GATT protocol error (ProtocolError)", "GATT 프로토콜 오류 (ProtocolError)"),
    ("Unknown GATT error", "알 수 없는 GATT 오류"),
    ("Timed out. Check that the device is prepared to connect and that Bluetooth is on.", "응답 시간이 지났어요. 기기의 연결 준비와 블루투스를 확인해 주세요."),
    ("Unsupported battery data.", "지원하지 않는 배터리 데이터예요."),
    ("The device rejected the request. Update Android NearLink and tap Prepare to connect.", "기기가 요청을 거절했어요. Android NearLink를 업데이트하고 연결 준비를 눌러 주세요."),
    ("Couldn't read battery info.", "배터리 정보를 읽지 못했어요."),
    ("Couldn't find the latest NearLink battery service. Update the Android app.", "최신 NearLink 배터리 서비스를 찾지 못했어요. Android 앱을 업데이트해 주세요."),
    ("Couldn't enable battery change notifications.", "배터리 변경 알림을 설정하지 못했어요."),
    ("The device was removed.", "기기가 제거됐어요."),
    ("Pairing is required. Select “Reconnect” to start a pairing request.", "페어링이 필요해요. ‘다시 연결’을 누르면 페어링 요청을 시작해요."),
    ("Check the pairing request on both devices.", "양쪽 기기의 페어링 요청을 확인해 주세요."),
    ("Battery service discovery failed", "배터리 서비스 검색 실패"),
    ("The device has no NearLink battery service. Check that Android NearLink is running.", "기기에 NearLink 배터리 서비스가 없어요. Android NearLink의 실행 상태를 확인해 주세요."),
    ("Couldn't verify the device's connection session.", "기기의 연결 세션을 확인하지 못했어요."),
    ("Invalid battery level.", "잘못된 배터리 잔량이에요."),
    ("Invalid battery status.", "잘못된 배터리 상태예요."),
    ("The connection changed. Reconnecting.", "연결이 변경되어 다시 연결할게요."),
    ("The battery notification didn't arrive. Reconnecting.", "배터리 알림이 도착하지 않아 다시 연결할게요."),
    ("The device's battery service restarted.", "기기의 배터리 서비스가 다시 시작됐어요."),
    ("The device's connection or service changed. Reconnecting.", "기기의 연결이나 서비스가 변경되어 다시 연결할게요."),
    ("The connection was lost.", "연결이 끊어졌어요."),
    ("Stopped receiving on this PC.", "이 PC의 수신을 중지했어요."),
    ("No devices found. On Android, tap Prepare to connect, then search again.", "기기를 찾지 못했어요. Android에서 연결 준비를 누르고 다시 검색해 주세요."),
    ("The search was interrupted. Check this PC's Bluetooth adapter and permissions.", "검색이 중단됐어요. PC의 블루투스 어댑터와 권한을 확인해 주세요."),
    ("--data-dir needs a path.", "--data-dir 경로가 필요해요."),
    ("Couldn't find the user settings folder.", "사용자 설정 폴더를 찾지 못했어요."),
    ("Unsupported settings format. Check settings.json.", "지원하지 않는 설정 형식이에요. settings.json을 확인해 주세요."),
    ("The settings contain an invalid or duplicate device address.", "설정에 잘못되거나 중복된 기기 주소가 있어요."),
    ("Connecting", "연결 중"),
    ("Reading battery", "배터리 확인 중"),
    ("Connected", "연결됨"),
    ("Waiting to reconnect", "재연결 대기"),
    ("Stopped", "중지됨"),
    ("Yesterday", "어제"),
    ("Charging", "충전 중"),
    ("Plugged in", "전원 연결됨"),
    ("Disconnected · Waiting to reconnect", "연결 끊김 · 재연결 대기"),
    ("Receiving stopped", "수신 중지됨"),
    ("Connected · Receiving battery", "연결됨 · 배터리 수신 중"),
    ("Not received yet", "수신 전"),
    ("Battery is low.", "배터리가 부족해요."),
    ("Battery is fully charged.", "배터리가 완전히 충전됐어요."),
    ("The device sent an invalid pairing confirmation code.", "기기에서 잘못된 페어링 확인 코드를 받았어요."),
    ("This device's pairing confirmation method isn't supported.", "이 기기의 페어링 확인 방식을 지원하지 않아요."),
    ("Couldn't complete the pairing confirmation.", "페어링 확인을 완료하지 못했어요."),
    ("The device didn't send a pairing confirmation request.", "기기에서 페어링 확인 요청을 받지 못했어요."),
    ("NearLink · Pairing confirmation", "NearLink · 페어링 확인"),
    ("Do the confirmation codes on both devices match?", "두 기기의 확인 코드가 같은가요?"),
    ("Preview needs a separate --data-dir folder.", "미리 보기에는 별도 --data-dir 폴더를 지정해 주세요."),
    ("No connected devices.\nOn Android NearLink, tap Prepare to connect, then add the device.", "연결한 기기가 없어요.\nAndroid NearLink에서 연결 준비를 누른 뒤 기기를 추가해 주세요."),
    ("No connected devices.\r\nOn Android NearLink, tap Prepare to connect, then add the device.", "연결한 기기가 없어요.\r\nAndroid NearLink에서 연결 준비를 누른 뒤 기기를 추가해 주세요."),
    ("Device Battery", "기기 배터리"),
    ("NearLink · Preview", "NearLink · 미리 보기"),
    ("&Add device", "기기 추가(&A)"),
    ("&Open details", "상세 설정(&O)"),
    ("&Reconnect", "다시 연결(&R)"),
    ("&Stop receiving", "수신 중지(&S)"),
    ("&Nickname", "별명(&N)"),
    ("Re&move from list", "목록에서 제거(&D)"),
    ("S&ettings", "설정(&O)"),
    ("Device a&ctions", "기기 동작(&M)"),
    ("Check the battery of nearby phones and tablets.", "가까이 있는 휴대폰과 태블릿의 배터리를 확인해 주세요."),
    ("Preview · Sample devices, no BLE reception", "미리 보기 · 예시 기기, BLE 수신 안 함"),
    ("NearLink keeps receiving from the tray when you close this window.", "창을 닫아도 트레이에서 수신해요."),
    ("This value hasn't been updated for over 15 minutes. If the battery doesn't change, the received time can get old.", "15분 이상 갱신되지 않은 값이에요. 배터리가 변하지 않으면 수신 시각이 오래될 수 있어요."),
    ("The battery hasn't changed, so the received time is old, but a recent connection check succeeded and this is the current value.", "배터리 변화가 없어 수신 시각은 오래됐지만, 최근 연결 확인에 성공해 현재 값이에요."),
    ("This is the last level received and may differ from the current value.", "마지막으로 받은 잔량이에요. 현재 값과 다를 수 있어요."),
    ("The value updates automatically when the battery changes. The last received time is based on battery data reception.", "배터리 값이 바뀌면 자동으로 갱신해요. 마지막 수신 시각은 배터리 데이터 수신 기준이에요."),
    ("Select a device to see its connection status.", "기기를 선택하면 연결 상태를 확인할 수 있어요."),
    ("Device nickname", "기기 별명"),
    ("Settings", "설정"),
    ("Add device", "기기 추가"),
    ("On Android NearLink, tap Prepare to connect.\r\nSelect the discovered device and connect to start pairing.", "Android NearLink에서 연결 준비를 눌러 주세요.\r\n검색된 기기를 선택하고 연결하면 페어링을 진행할게요."),
    ("&Rescan", "다시 검색(&R)"),
    ("&Bluetooth settings", "블루투스 설정(&B)"),
    ("&Connect selected device", "선택한 기기 연결(&C)"),
    ("Searching…", "검색 중…"),
    ("&Nickname\r\nLeave it empty to use the Bluetooth device name.", "기기 별명(&N)\r\n비워 두면 블루투스 기기 이름을 사용해요."),
    ("&Save", "저장(&S)"),
    ("Cancel", "취소"),
    ("Notifications and startup", "알림과 자동 시작"),
    ("&Low battery alert", "배터리 부족 알림(&L)"),
    ("% or below · Not while charging", "% 이하 · 충전 중에는 알리지 않음"),
    ("Notify when &fully charged (100%)", "100% 충전 완료 알림(&F)"),
    ("Start in the tray when I sign in to &Windows", "Windows 로그인 시 트레이에서 시작(&W)"),
    ("You can add up to 16 devices. Remove an unused device first.", "최대 16대까지 등록할 수 있어요. 사용하지 않는 기기를 먼저 제거해 주세요."),
    ("Enter a number from 1 to 90 for the low battery threshold.", "부족 알림 기준은 1~90 사이의 숫자를 입력해 주세요."),
    ("Remove device", "기기 제거"),
    ("Battery outline (default)", "배터리 윤곽선 (기본)"),
    ("&Tray icon", "트레이 표시(&T)"),
    ("No connected devices", "연결한 기기가 없어요"),
    ("&Open Device Battery", "기기 배터리 열기(&O)"),
    ("&Settings", "설정(&S)"),
    ("E&xit", "종료(&X)"),
    ("Work phone", "업무용 휴대폰"),
    ("Device not received yet", "아직 수신하지 않은 기기"),
    ("{0} Select “Reconnect” to try pairing again from the start. (Status {1})", "{0} ‘다시 연결’을 누르면 페어링부터 다시 시도할게요. (상태 {1})"),
    ("{0}: {1} (Status {2}).", "{0}: {1} (상태 {2})."),
    ("Pairing request failed. Select “Reconnect” to try again. {0}", "페어링 요청에 실패했어요. ‘다시 연결’을 눌러 다시 시도해 주세요. {0}"),
    ("{0} Retrying in {1} s.", "{0} {1}초 후 재시도할게요."),
    ("Couldn't search for devices. Check Bluetooth. {0}", "기기를 검색하지 못했어요. 블루투스를 확인해 주세요. {0}"),
    ("NearLink device · {0}", "NearLink 기기 · {0}"),
    ("Couldn't read settings: {0}", "설정을 읽지 못했어요: {0}"),
    ("Check the settings file: {0}", "설정 파일을 확인해 주세요: {0}"),
    ("Couldn't save settings: {0}", "설정을 저장하지 못했어요: {0}"),
    ("Couldn't start NearLink.\n{0}", "NearLink를 시작하지 못했어요.\n{0}"),
    ("{0} sec ago", "{0}초 전"),
    ("{0} min ago", "{0}분 전"),
    ("{0} hr ago", "{0}시간 전"),
    ("{0} days ago", "{0}일 전"),
    ("Unknown", "확인 전"),
    ("Not charging", "충전 중 아님"),
    ("{0} · Outdated value", "{0} · 오래된 값"),
    ("{0} · Last value", "{0} · 마지막 값"),
    ("{0} · Outdated value {1}", "{0} · 오래된 값 {1}"),
    ("{0} · Last value {1}", "{0} · 마지막 값 {1}"),
    ("{0} · {1} · {2} · {3} · Last received {4}", "{0} · {1} · {2} · {3} · 마지막 수신 {4}"),
    ("{0} Currently {1}%", "{0} 현재 {1}%"),
    ("NearLink · {0} battery alerts", "NearLink · 배터리 알림 {0}건"),
    ("and {0} more alerts", "외 {0}건"),
    ("{0}\n\nConfirmation code: {1}\n\nIf Android shows the same code, select “Yes”.\nAlso confirm the pairing request on Android.", "{0}\n\n확인 코드: {1}\n\nAndroid에 같은 코드가 표시되면 ‘예’를 눌러 주세요.\nAndroid에서도 페어링 요청을 확인해 주세요."),
    ("{0} · Already added", "{0} · 이미 등록됨"),
    ("{0} · Added, receiving stopped", "{0} · 등록됨, 수신 중지"),
    ("Looking for devices… {0} found · The search ends in 15 seconds.", "기기를 찾고 있어요… {0}대 발견 · 검색은 15초 후 끝나요."),
    ("Found {0} devices. Select a device to connect.", "{0}대를 찾았어요. 연결할 기기를 선택해 주세요."),
    ("Remove {0} from this PC's list?\nWindows pairing is kept. To also stop Android from sending, stop the connection in the Android app.", "{0} 기기를 이 PC의 목록에서 제거할까요?\nWindows 페어링은 유지돼요. Android의 송신도 끝내려면 상대 앱에서 연결을 중지해 주세요."),
    ("Show {0} level as number", "{0} 잔량 숫자"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SOURCES: &[&str] = &[
        include_str!("ble.rs"),
        include_str!("config.rs"),
        include_str!("main.rs"),
        include_str!("model.rs"),
        include_str!("pairing.rs"),
        include_str!("theme.rs"),
        include_str!("tray.rs"),
        include_str!("ui.rs"),
    ];

    fn unescape(raw: &str) -> String {
        let mut out = String::new();
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    /// Message keys found in the sources: every string literal passed to t, tf, tn or pw.
    fn used_keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for source in SOURCES {
            for name in ["t", "tf", "tn", "pw"] {
                let pattern = format!("{name}(");
                let mut from = 0;
                while let Some(found) = source[from..].find(&pattern) {
                    let start = from + found;
                    from = start + pattern.len();
                    let previous = source[..start].chars().next_back();
                    if previous.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '!') {
                        continue;
                    }
                    let rest = source[from..].trim_start();
                    if !rest.starts_with('"') {
                        continue;
                    }
                    // The first literal of tn is the singular form, which needs no table entry.
                    let mut literals = vec![];
                    let mut cursor = rest;
                    for _ in 0..if name == "tn" { 2 } else { 1 } {
                        let Some(body) = cursor.trim_start().strip_prefix('"') else { break };
                        let mut end = 0;
                        let bytes = body.as_bytes();
                        while end < bytes.len() && bytes[end] != b'"' {
                            end += if bytes[end] == b'\\' { 2 } else { 1 };
                        }
                        literals.push(unescape(&body[..end]));
                        cursor = body[end + 1..].trim_start().trim_start_matches(',');
                    }
                    if let Some(key) = literals.last() {
                        keys.insert(key.clone());
                    }
                }
            }
        }
        keys
    }

    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for index in 0..10 {
            if text.contains(&format!("{{{index}}}")) {
                found.insert(index.to_string());
            }
        }
        found
    }

    #[test]
    fn every_message_has_a_korean_translation() {
        let missing: Vec<_> = used_keys().into_iter().filter(|key| korean(key).is_none()).collect();
        assert!(missing.is_empty(), "missing Korean translations: {missing:#?}");
    }

    #[test]
    fn korean_table_has_no_unused_entries() {
        let used = used_keys();
        let unused: Vec<_> = KO.iter().map(|(key, _)| *key).filter(|key| !used.contains(*key)).collect();
        assert!(unused.is_empty(), "unused Korean entries: {unused:#?}");
    }

    #[test]
    fn translations_keep_the_same_placeholders() {
        for (english, korean) in KO {
            assert_eq!(placeholders(english), placeholders(korean), "{english}");
        }
    }

    #[test]
    fn language_codes_are_parsed() {
        assert_eq!(parse("ko-KR"), Some(Lang::Ko));
        assert_eq!(parse("EN_us"), Some(Lang::En));
        assert_eq!(parse("ja"), None);
    }

    #[test]
    fn english_is_the_default_and_korean_is_available() {
        assert_eq!(t("Cancel"), "Cancel");
        with_lang(Lang::Ko, || assert_eq!(t("Cancel"), "취소"));
        assert_eq!(tf("{0} min ago", &[&5]), "5 min ago");
        with_lang(Lang::Ko, || assert_eq!(tf("{0} min ago", &[&5]), "5분 전"));
    }

    #[test]
    fn plural_forms_follow_the_language() {
        assert_eq!(tn("Found {0} device.", "Found {0} devices.", 1), "Found 1 device.");
        assert_eq!(tn("Found {0} device.", "Found {0} devices.", 3), "Found 3 devices.");
    }

    #[test]
    fn wide_strings_are_nul_terminated_and_cached() {
        let first = pw("Cancel");
        assert_eq!(first, pw("Cancel"));
        let text = unsafe { first.to_string().unwrap() };
        assert_eq!(text, "Cancel");
    }
}
