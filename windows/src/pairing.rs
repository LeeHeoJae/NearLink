//! Explicit desktop consent and numeric comparison for encrypted Bluetooth pairing.
use super::{Apartment, wait};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use windows::{
    Devices::Enumeration::{DeviceInformationPairing, DeviceInformationCustomPairing,
        DevicePairingRequestedEventArgs, DevicePairingKinds, DevicePairingProtectionLevel,
        DevicePairingResultStatus},
    Foundation::TypedEventHandler,
    Win32::{Foundation::{HWND, LPARAM, WPARAM}, UI::{Controls::*, WindowsAndMessaging::{
        SendMessageW, IDCANCEL, IDYES,
    }}},
    core::{Result, HRESULT, PCWSTR, w},
};

struct Registration {
    custom: DeviceInformationCustomPairing,
    token: i64,
    active: Arc<AtomicBool>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let _ = self.custom.RemovePairingRequested(self.token);
    }
}

pub(super) fn request(pairing: &DeviceInformationPairing, stop: &Arc<AtomicBool>, owner: isize)
    -> Result<DevicePairingResultStatus>
{
    let custom = pairing.Custom()?;
    let active = Arc::new(AtomicBool::new(true));
    let seen = Arc::new(AtomicBool::new(false));
    let callback_error = Arc::new(Mutex::new(None::<String>));
    let request_active = active.clone();
    let request_stop = stop.clone();
    let request_seen = seen.clone();
    let errors = callback_error.clone();
    let token = custom.PairingRequested(&TypedEventHandler::<
        DeviceInformationCustomPairing, DevicePairingRequestedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        request_seen.store(true, Ordering::Release);
        if !request_active.load(Ordering::Acquire) || request_stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let result = (|| -> Result<()> {
            match args.PairingKind()? {
                // Windows provides the system consent dialog for ConfirmOnly.
                DevicePairingKinds::ConfirmOnly => args.Accept(),
                DevicePairingKinds::ConfirmPinMatch => {
                    let pin = args.Pin()?.to_string();
                    if !valid_pin(&pin) {
                        return Err(super::failure("기기에서 잘못된 페어링 확인 코드를 받았습니다."));
                    }
                    let name = args.DeviceInformation()?.Name()?.to_string();
                    let deferral = args.GetDeferral()?;
                    let args = args.clone();
                    let active = request_active.clone();
                    let stop = request_stop.clone();
                    let errors = errors.clone();
                    std::thread::spawn(move || {
                        let result = (|| -> Result<()> {
                            let _apartment = Apartment::new()?;
                            if confirm_pin(&name, &pin, &active, &stop, owner)?
                                && active.load(Ordering::Acquire) && !stop.load(Ordering::Acquire)
                            {
                                args.Accept()?;
                            }
                            Ok(())
                        })();
                        if let Err(error) = result {
                            *errors.lock().unwrap() = Some(error.to_string());
                        }
                        let _ = deferral.Complete();
                    });
                    Ok(())
                }
                _ => Err(super::failure("이 기기의 페어링 확인 방식을 지원하지 않습니다.")),
            }
        })();
        if let Err(error) = result {
            *errors.lock().unwrap() = Some(error.to_string());
        }
        Ok(())
    }))?;
    let _registration = Registration { custom: custom.clone(), token, active };
    let result = wait(custom.PairWithProtectionLevelAsync(
        DevicePairingKinds::ConfirmOnly | DevicePairingKinds::ConfirmPinMatch,
        DevicePairingProtectionLevel::Encryption,
    )?, stop, 60)?;
    let status = result.Status()?;
    if status != DevicePairingResultStatus::Paired && status != DevicePairingResultStatus::AlreadyPaired {
        let detail = callback_error.lock().unwrap().clone().unwrap_or_else(|| {
            if seen.load(Ordering::Acquire) { "페어링 확인을 완료하지 못했습니다.".into() }
            else { "기기에서 페어링 확인 요청을 받지 못했습니다.".into() }
        });
        let cause = super::check_pairing_result(status).unwrap_err();
        return Err(super::pairing_failure(&format!("{} {detail}", cause.message())));
    }
    Ok(status)
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.bytes().all(|value| value.is_ascii_digit())
}

struct PromptState<'a> { active: &'a AtomicBool, stop: &'a AtomicBool }

unsafe extern "system" fn prompt_timer(hwnd: HWND, notification: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM, _: LPARAM, data: isize) -> HRESULT
{
    if notification == TDN_TIMER {
        // TaskDialogIndirect is synchronous; this state outlives every callback.
        let state = unsafe { &*(data as *const PromptState<'_>) };
        if !state.active.load(Ordering::Acquire) || state.stop.load(Ordering::Acquire) {
            unsafe { SendMessageW(hwnd, TDM_CLICK_BUTTON.0 as u32,
                Some(WPARAM(IDCANCEL.0 as usize)), Some(LPARAM(0))); }
        }
    }
    HRESULT(0)
}

fn confirm_pin(name: &str, pin: &str, active: &AtomicBool, stop: &AtomicBool, owner: isize) -> Result<bool> {
    if !active.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Ok(false);
    }
    let text: Vec<u16> = format!("{name}\n\n확인 코드: {pin}\n\nAndroid에 같은 코드가 표시되면 ‘예’를 누르세요.\nAndroid에서도 페어링 요청을 확인해 주세요.")
        .encode_utf16().chain(Some(0)).collect();
    let state = PromptState { active, stop };
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND(owner as *mut _),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER | TDF_SIZE_TO_CONTENT,
        dwCommonButtons: TDCBF_YES_BUTTON | TDCBF_NO_BUTTON | TDCBF_CANCEL_BUTTON,
        pszWindowTitle: w!("NearLink · 페어링 확인"),
        pszMainInstruction: w!("두 기기의 확인 코드가 같은가요?"),
        pszContent: PCWSTR(text.as_ptr()),
        nDefaultButton: IDCANCEL.0,
        pfCallback: Some(prompt_timer),
        lpCallbackData: &state as *const PromptState<'_> as isize,
        ..Default::default()
    };
    let mut selected = 0;
    unsafe { TaskDialogIndirect(&config, Some(&mut selected), None, None)?; }
    Ok(selected == IDYES.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_codes_preserve_leading_zeroes_and_reject_invalid_input() {
        assert!(valid_pin("000123"));
        assert!(valid_pin("123456"));
        for invalid in ["", "12345", "1234567", "１２３４５６", "123\n45", "<1234>"] {
            assert!(!valid_pin(invalid));
        }
    }

    #[test]
    #[ignore = "opens a native dialog for manual visual verification; cancels after 20 seconds"]
    fn confirmation_dialog_closes_when_pairing_attempt_ends() {
        let active = Arc::new(AtomicBool::new(true));
        let finish = active.clone();
        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(20));
            finish.store(false, Ordering::Release);
        });
        let result = confirm_pin("화면 검증용 예시 기기 · 실제 페어링 아님", "000123", &active, &AtomicBool::new(false), 0);
        closer.join().unwrap();
        assert!(!result.unwrap());
    }
}
