use crate::{
    i18n::{t, tf},
    model::{Discovered, Shared, State},
    protocol,
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    Devices::{
        Bluetooth::{
            Advertisement::*, BluetoothAddressType, BluetoothCacheMode, BluetoothConnectionStatus,
            BluetoothLEDevice, GenericAttributeProfile::*,
        },
        Enumeration::DevicePairingResultStatus,
    },
    Foundation::TypedEventHandler,
    Storage::Streams::{DataReader, DataWriter, IBuffer},
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    core::{Error, GUID, HRESULT, IInspectable, Result, RuntimeType},
};
use windows_future::{AsyncStatus, IAsyncOperation};
#[path = "pairing.rs"]
mod pairing;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

// Pairing needs another explicit user action after cancellation or rejection.
// Do not bury its cause under automatic reconnect attempts.
const PAIRING_REQUIRED: HRESULT = HRESULT(0x8004A001u32 as i32);

fn pairing_failure(text: &str) -> Error {
    Error::new(PAIRING_REQUIRED, text)
}

fn check_pairing_result(status: DevicePairingResultStatus) -> Result<()> {
    if status == DevicePairingResultStatus::Paired
        || status == DevicePairingResultStatus::AlreadyPaired
    {
        return Ok(());
    }
    let reason = match status {
        DevicePairingResultStatus::PairingCanceled => t("Pairing was canceled."),
        DevicePairingResultStatus::AuthenticationTimeout => {
            t("Pairing confirmation timed out. Check the request on both devices.")
        }
        DevicePairingResultStatus::ConnectionRejected
        | DevicePairingResultStatus::RejectedByHandler => t("The pairing request was rejected."),
        DevicePairingResultStatus::AccessDenied
        | DevicePairingResultStatus::AuthenticationNotAllowed => {
            t("Windows didn't allow pairing. Check the Bluetooth permissions.")
        }
        DevicePairingResultStatus::NotReadyToPair => {
            t("The device isn't ready to pair. On Android, tap Prepare to connect.")
        }
        DevicePairingResultStatus::AuthenticationFailure => {
            t("Pairing authentication failed. Check the existing pairing records on both devices.")
        }
        DevicePairingResultStatus::ProtectionLevelCouldNotBeMet => {
            t("Couldn't set up encrypted pairing.")
        }
        DevicePairingResultStatus::OperationAlreadyInProgress => {
            t("Another pairing request is in progress. Finish it, then try again.")
        }
        DevicePairingResultStatus::RequiredHandlerNotRegistered => {
            t("Windows couldn't open the pairing confirmation window.")
        }
        _ => t("Couldn't complete pairing."),
    };
    Err(pairing_failure(&tf(
        "{0} Select “Reconnect” to try pairing again from the start. (Status {1})",
        &[&reason, &status.0],
    )))
}

fn failure(text: &str) -> Error {
    Error::new(HRESULT(0x80004005u32 as i32), text)
}

fn gatt_failure(stage: &str, status: GattCommunicationStatus) -> Error {
    let reason = match status {
        GattCommunicationStatus::Unreachable => t("Device not responding (Unreachable)"),
        GattCommunicationStatus::AccessDenied => t("Access denied (AccessDenied)"),
        GattCommunicationStatus::ProtocolError => t("GATT protocol error (ProtocolError)"),
        _ => t("Unknown GATT error"),
    };
    failure(&tf("{0}: {1} (Status {2}).", &[&stage, &reason, &status.0]))
}

struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn wait<T: RuntimeType>(
    operation: IAsyncOperation<T>,
    stop: &AtomicBool,
    seconds: u64,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while operation.Status()? == AsyncStatus::Started {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            let _ = operation.Cancel();
            return Err(failure(
                t("Timed out. Check that the device is prepared to connect and that Bluetooth is on."),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    operation.GetResults()
}

fn bytes(buffer: &IBuffer) -> Result<Vec<u8>> {
    let length = buffer.Length()? as usize;
    if length > 512 {
        return Err(failure(t("Unsupported battery data.")));
    }
    let mut value = vec![0; length];
    DataReader::FromBuffer(buffer)?.ReadBytes(&mut value)?;
    Ok(value)
}

fn write(characteristic: &GattCharacteristic, value: &[u8], stop: &AtomicBool) -> Result<()> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(value)?;
    let result = wait(
        characteristic.WriteValueWithResultAndOptionAsync(
            &writer.DetachBuffer()?,
            GattWriteOption::WriteWithResponse,
        )?,
        stop,
        20,
    )?;
    if result.Status()? != GattCommunicationStatus::Success {
        return Err(failure(
            t("The device rejected the request. Update Android NearLink and tap Prepare to connect."),
        ));
    }
    Ok(())
}

fn read(characteristic: &GattCharacteristic, stop: &AtomicBool) -> Result<Vec<u8>> {
    let result = wait(
        characteristic.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
        stop,
        20,
    )?;
    if result.Status()? != GattCommunicationStatus::Success {
        return Err(failure(t("Couldn't read battery info.")));
    }
    bytes(&result.Value()?)
}

fn characteristic(
    service: &GattDeviceService,
    uuid: GUID,
    stop: &AtomicBool,
) -> Result<GattCharacteristic> {
    let result = wait(
        service.GetCharacteristicsForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)?,
        stop,
        20,
    )?;
    if result.Status()? != GattCommunicationStatus::Success
        || result.Characteristics()?.Size()? == 0
    {
        return Err(failure(
            t("Couldn't find the latest NearLink battery service. Update the Android app."),
        ));
    }
    let characteristic = result.Characteristics()?.GetAt(0)?;
    characteristic.SetProtectionLevel(GattProtectionLevel::EncryptionRequired)?;
    Ok(characteristic)
}

#[derive(Debug)]
enum Event {
    Level(Vec<u8>),
    Status(Vec<u8>),
    Invalidated,
    Stop,
}

/// Owns every registration; releasing a failed attempt removes callbacks before reconnect.
struct Link {
    device: BluetoothLEDevice,
    session: Option<GattSession>,
    service: Option<GattDeviceService>,
    tokens: Vec<(GattCharacteristic, i64)>,
    changed: Option<i64>,
    disconnected: Option<i64>,
}
impl Drop for Link {
    fn drop(&mut self) {
        for (characteristic, token) in &self.tokens {
            let _ = characteristic.RemoveValueChanged(*token);
        }
        if let Some(token) = self.changed {
            let _ = self.device.RemoveGattServicesChanged(token);
        }
        if let Some(token) = self.disconnected {
            let _ = self.device.RemoveConnectionStatusChanged(token);
        }
        if let Some(service) = &self.service {
            let _ = service.Close();
        }
        if let Some(session) = &self.session {
            let _ = session.SetMaintainConnection(false);
            let _ = session.Close();
        }
        let _ = self.device.Close();
    }
}

fn subscribe(
    link: &mut Link,
    characteristic: &GattCharacteristic,
    level: bool,
    tx: &mpsc::Sender<Event>,
    stop: &AtomicBool,
) -> Result<()> {
    let sender = tx.clone();
    let token = characteristic.ValueChanged(&TypedEventHandler::<
        GattCharacteristic,
        GattValueChangedEventArgs,
    >::new(move |_, args| {
        if let Some(args) = args.as_ref()
            && let Ok(value) = args.CharacteristicValue().and_then(|value| bytes(&value))
        {
            let _ = sender.send(if level {
                Event::Level(value)
            } else {
                Event::Status(value)
            });
        }
        Ok(())
    }))?;
    link.tokens.push((characteristic.clone(), token));
    let result = wait(
        characteristic.WriteClientCharacteristicConfigurationDescriptorAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )?,
        stop,
        20,
    )?;
    if result != GattCommunicationStatus::Success {
        return Err(failure(t("Couldn't enable battery change notifications.")));
    }
    Ok(())
}

fn set_state(shared: &Shared, address: u64, state: State, detail: &str, wake: &Wake) {
    if let Some(peer) = shared.lock().unwrap().peers.get_mut(&address) {
        peer.state = state;
        peer.detail = detail.into();
    }
    wake();
}

struct ConnectionContext<'a> {
    shared: &'a Shared,
    wake: &'a Wake,
    owner: isize,
}

fn connection(
    context: ConnectionContext<'_>,
    address: u64,
    pair: &mut bool,
    stop: &Arc<AtomicBool>,
    rx: &mpsc::Receiver<Event>,
    tx: &mpsc::Sender<Event>,
) -> Result<()> {
    let ConnectionContext {
        shared,
        wake,
        owner,
    } = context;
    set_state(shared, address, State::Connecting, "", wake);
    let saved = shared
        .lock()
        .unwrap()
        .peers
        .get(&address)
        .ok_or_else(|| failure(t("The device was removed.")))?
        .saved
        .clone();
    let operation = if saved.device_id.is_empty() {
        BluetoothLEDevice::FromBluetoothAddressWithBluetoothAddressTypeAsync(
            address,
            if saved.random_address {
                BluetoothAddressType::Random
            } else {
                BluetoothAddressType::Public
            },
        )?
    } else {
        BluetoothLEDevice::FromIdAsync(&saved.device_id.into())?
    };
    let device = wait(operation, stop, 20)?;
    let mut link = Link {
        device,
        session: None,
        service: None,
        tokens: vec![],
        changed: None,
        disconnected: None,
    };
    let pairing = link.device.DeviceInformation()?.Pairing()?;
    if !pairing.IsPaired()? {
        if !*pair {
            return Err(pairing_failure(
                t("Pairing is required. Select “Reconnect” to start a pairing request."),
            ));
        }
        set_state(
            shared,
            address,
            State::Connecting,
            t("Check the pairing request on both devices."),
            wake,
        );
        let result = pairing::request(&pairing, stop, owner);
        result.map_err(|error| {
            if error.code() == PAIRING_REQUIRED {
                error
            } else {
                pairing_failure(&tf(
                    "Pairing request failed. Select “Reconnect” to try again. {0}",
                    &[&error],
                ))
            }
        })?;
    }
    // Only consume the user's pairing intent after pairing has actually succeeded.
    // A transient failure while opening the device must not disable the first request.
    *pair = false;
    // Own a connection request for the whole attempt. A device object alone does
    // not keep the LE link alive across app restarts or service discovery gaps.
    let session = wait(
        GattSession::FromDeviceIdAsync(&link.device.BluetoothDeviceId()?)?,
        stop,
        20,
    )?;
    link.session = Some(session.clone());
    session.SetMaintainConnection(true)?;
    // Keep Windows' paired identity for reconnects after LE address rotation.
    let device_id = link.device.DeviceId()?.to_string();
    let name = link.device.Name()?.to_string();
    {
        let mut model = shared.lock().unwrap();
        if let Some(peer) = model.peers.get_mut(&address) {
            let changed =
                peer.saved.device_id != device_id || (!name.is_empty() && peer.saved.name != name);
            if changed {
                peer.saved.device_id = device_id;
                if !name.is_empty() {
                    peer.saved.name = name;
                }
                let learned = peer.saved.clone();
                if let Some(saved) = model
                    .settings
                    .peers
                    .iter_mut()
                    .find(|p| p.address == address)
                {
                    *saved = learned;
                }
                model.dirty = true;
            }
        }
    }
    wake();
    let invalidated = tx.clone();
    link.changed = Some(link.device.GattServicesChanged(&TypedEventHandler::<
        BluetoothLEDevice,
        IInspectable,
    >::new(move |_, _| {
        let _ = invalidated.send(Event::Invalidated);
        Ok(())
    }))?);
    let disconnected = tx.clone();
    link.disconnected =
        Some(link.device.ConnectionStatusChanged(&TypedEventHandler::<
            BluetoothLEDevice,
            IInspectable,
        >::new(move |device, _| {
            if let Some(device) = device.as_ref()
                && device.ConnectionStatus()? == BluetoothConnectionStatus::Disconnected
            {
                let _ = disconnected.send(Event::Invalidated);
            }
            Ok(())
        }))?);
    let result = wait(
        link.device.GetGattServicesForUuidWithCacheModeAsync(
            protocol::BAS,
            BluetoothCacheMode::Uncached,
        )?,
        stop,
        20,
    )?;
    let status = result.Status()?;
    if status != GattCommunicationStatus::Success {
        return Err(gatt_failure(t("Battery service discovery failed"), status));
    }
    if result.Services()?.Size()? == 0 {
        return Err(failure(
            t("The device has no NearLink battery service. Check that Android NearLink is running."),
        ));
    }
    let service = result.Services()?.GetAt(0)?;
    link.service = Some(service.clone());
    let control = characteristic(&service, protocol::CONTROL, stop)?;
    let session = characteristic(&service, protocol::SESSION, stop)?;
    let level_char = characteristic(&service, protocol::LEVEL, stop)?;
    let status_char = characteristic(&service, protocol::STATUS, stop)?;
    set_state(shared, address, State::Reading, "", wake);
    write(&control, &[protocol::JOIN_RECEIVER], stop)?;
    let request = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        | 1;
    write(&session, &protocol::hello(request), stop)?;
    let epoch = protocol::ready(&read(&session, stop)?, request, None)
        .ok_or_else(|| failure(t("Couldn't verify the device's connection session.")))?;
    let mut level = read(&level_char, stop)?
        .first()
        .copied()
        .filter(|v| *v <= 100);
    if level.is_none() {
        return Err(failure(t("Invalid battery level.")));
    }
    let initial_status = protocol::battery_status(&read(&status_char, stop)?)
        .ok_or_else(|| failure(t("Invalid battery status.")))?;
    let mut charging = initial_status.charging;
    let mut plugged = initial_status.plugged;
    shared
        .lock()
        .unwrap()
        .reading(address, level, charging, plugged, false);
    wake();
    subscribe(&mut link, &level_char, true, tx, stop)?;
    subscribe(&mut link, &status_char, false, tx, stop)?;
    // Discard notifications that arrived before ACK. ACK always causes a fresh level notification.
    while let Ok(event) = rx.try_recv() {
        if matches!(event, Event::Stop | Event::Invalidated) {
            return Err(failure(t("The connection changed. Reconnecting.")));
        }
    }
    write(&session, &protocol::ack(request, epoch), stop)?;
    let mut verified = false;
    let mut deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let event = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !verified {
                    return Err(failure(t("The battery notification didn't arrive. Reconnecting.")));
                }
                write(&control, &[protocol::JOIN_RECEIVER], stop)?;
                if protocol::ready(&read(&session, stop)?, request, Some(epoch)).is_none() {
                    return Err(failure(t("The device's battery service restarted.")));
                }
                shared.lock().unwrap().verified(address);
                wake();
                deadline = Instant::now() + Duration::from_secs(30);
                continue;
            }
        };
        match event {
            Event::Stop => return Ok(()),
            Event::Invalidated => {
                return Err(failure(
                    t("The device's connection or service changed. Reconnecting."),
                ));
            }
            Event::Level(value) => {
                if value.len() != 1 || value[0] > 100 {
                    continue;
                }
                level = Some(value[0]);
                if !verified {
                    verified = true;
                    deadline = Instant::now() + Duration::from_secs(30);
                }
            }
            Event::Status(value) => {
                let Some(status) = protocol::battery_status(&value) else {
                    continue;
                };
                // Status does not replace the mandatory post-ACK Battery Level notification.
                charging = status.charging;
                plugged = status.plugged;
                level = status.level.or(level);
            }
        }
        shared
            .lock()
            .unwrap()
            .reading(address, level, charging, plugged, verified);
        wake();
    }
}

struct Worker {
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.tx.send(Event::Stop);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct Engine {
    shared: Shared,
    wake: Wake,
    workers: HashMap<u64, Worker>,
    scan: Option<Arc<AtomicBool>>,
    owner: isize,
}

impl Engine {
    pub fn new(shared: Shared, wake: Wake, owner: isize) -> Self {
        Self {
            shared,
            wake,
            workers: HashMap::new(),
            scan: None,
            owner,
        }
    }
    pub fn start(&mut self, address: u64, pair: bool) {
        self.workers.remove(&address);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_stop = stop.clone();
        let worker_tx = tx.clone();
        let shared = self.shared.clone();
        let wake = self.wake.clone();
        let owner = self.owner;
        let handle = thread::spawn(move || {
            let _apartment = match Apartment::new() {
                Ok(value) => value,
                Err(error) => {
                    set_state(&shared, address, State::Stopped, &error.to_string(), &wake);
                    return;
                }
            };
            let mut delay = 1u64;
            let mut may_pair = pair;
            while !stop.load(Ordering::Acquire) {
                let started = Instant::now();
                let result = connection(
                    ConnectionContext {
                        shared: &shared,
                        wake: &wake,
                        owner,
                    },
                    address,
                    &mut may_pair,
                    &stop,
                    &rx,
                    &tx,
                );
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if let Err(error) = &result
                    && error.code() == PAIRING_REQUIRED
                {
                    set_state(&shared, address, State::Stopped, &error.message(), &wake);
                    break;
                }
                if started.elapsed() > Duration::from_secs(90) {
                    delay = 1;
                }
                let detail = result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| t("The connection was lost.").into());
                set_state(
                    &shared,
                    address,
                    State::Retrying,
                    &tf("{0} Retrying in {1} s.", &[&detail, &delay]),
                    &wake,
                );
                // Drop old callback messages before waiting; Stop still has an atomic flag.
                while rx.try_recv().is_ok() {}
                let retry = Instant::now() + Duration::from_secs(delay);
                while !stop.load(Ordering::Acquire) && Instant::now() < retry {
                    let _ = rx.recv_timeout(retry.saturating_duration_since(Instant::now()));
                }
                delay = (delay * 2).min(60);
            }
        });
        self.workers.insert(
            address,
            Worker {
                stop: worker_stop,
                tx: worker_tx,
                handle: Some(handle),
            },
        );
    }
    pub fn stop(&mut self, address: u64) {
        self.workers.remove(&address);
        set_state(
            &self.shared,
            address,
            State::Stopped,
            t("Stopped receiving on this PC."),
            &self.wake,
        );
    }
    pub fn cancel_scan(&mut self) {
        if let Some(stop) = self.scan.take() {
            stop.store(true, Ordering::Release);
        }
    }
    pub fn shutdown(&mut self) {
        self.cancel_scan();
        // Cancel every peer before joining any worker; each Link releases its
        // callbacks, service and maintained session before the instance lock opens.
        for worker in self.workers.values() {
            worker.cancel();
        }
        self.workers.clear();
    }
    pub fn scan(&mut self) {
        self.cancel_scan();
        let stop = Arc::new(AtomicBool::new(false));
        self.scan = Some(stop.clone());
        let shared = self.shared.clone();
        let wake = self.wake.clone();
        {
            let mut model = shared.lock().unwrap();
            model.discovered.clear();
            model.scanning = true;
            model.message.clear();
        }
        wake();
        thread::spawn(move || {
            let result = scan(&shared, &wake, &stop);
            if !stop.load(Ordering::Acquire) {
                let mut model = shared.lock().unwrap();
                model.scanning = false;
                if let Err(error) = result {
                    model.message =
                        tf("Couldn't search for devices. Check Bluetooth. {0}", &[&error]);
                } else if model.discovered.is_empty() {
                    model.message =
                        t("No devices found. On Android, tap Prepare to connect, then search again.")
                            .into();
                }
                drop(model);
                wake();
            }
        });
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn scan(shared: &Shared, wake: &Wake, stop: &Arc<AtomicBool>) -> Result<()> {
    let _apartment = Apartment::new()?;
    let watcher = BluetoothLEAdvertisementWatcher::new()?;
    watcher.SetScanningMode(BluetoothLEScanningMode::Active)?;
    // Names arrive separately in scan responses without the service UUID.
    let names = std::sync::Mutex::new(HashMap::<u64, String>::new());
    let model = shared.clone();
    let updated = wake.clone();
    let cancelled = stop.clone();
    let received = watcher.Received(&TypedEventHandler::<
        BluetoothLEAdvertisementWatcher,
        BluetoothLEAdvertisementReceivedEventArgs,
    >::new(move |_, args| {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        let address = args.BluetoothAddress()?;
        let advertisement = args.Advertisement()?;
        let mut name = advertisement.LocalName()?.to_string();
        let services = advertisement.ServiceUuids()?;
        let nearlink = (0..services.Size()?)
            .any(|i| services.GetAt(i).is_ok_and(|uuid| uuid == protocol::BRIDGE));
        let mut names = names.lock().unwrap();
        if !name.is_empty() && names.len() < 128 {
            names.insert(address, name.clone());
        }
        if name.is_empty() {
            name = names.get(&address).cloned().unwrap_or_default();
        }
        drop(names);
        let mut model = model.lock().unwrap();
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(peer) = model
            .discovered
            .iter_mut()
            .find(|peer| peer.address == address)
        {
            if !name.is_empty() && peer.name != name {
                peer.name = name;
                drop(model);
                updated();
            }
        } else if nearlink && model.discovered.len() < 32 {
            model.discovered.push(Discovered {
                address,
                random_address: args.BluetoothAddressType()? == BluetoothAddressType::Random,
                name: if name.is_empty() {
                    tf("NearLink device · {0}", &[&format!("{:04X}", address & 0xffff)])
                } else {
                    name
                },
            });
            drop(model);
            updated();
        }
        Ok(())
    }))?;
    let result = (|| -> Result<()> {
        watcher.Start()?;
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until && !stop.load(Ordering::Acquire) {
            if watcher.Status()? == BluetoothLEAdvertisementWatcherStatus::Aborted {
                return Err(failure(
                    t("The search was interrupted. Check this PC's Bluetooth adapter and permissions."),
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    let _ = watcher.Stop();
    let _ = watcher.RemoveReceived(received);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gatt_discovery_failures_preserve_windows_status() {
        for (status, expected) in [
            (GattCommunicationStatus::Unreachable, "Unreachable"),
            (GattCommunicationStatus::AccessDenied, "AccessDenied"),
            (GattCommunicationStatus::ProtocolError, "ProtocolError"),
        ] {
            let error = gatt_failure(t("Battery service discovery failed"), status);
            assert!(error.message().contains(expected));
            assert!(error.message().contains(t("Battery service discovery failed")));
            assert_ne!(error.code(), PAIRING_REQUIRED);
        }
    }

    #[test]
    fn shutdown_cancels_all_peers_before_joining_and_can_repeat() {
        use crate::{config::Settings, model::Model};
        use std::sync::Mutex;
        let shared = Arc::new(Mutex::new(Model::new(Settings::default())));
        let mut engine = Engine::new(shared, Arc::new(|| {}), 0);
        let flags: Vec<_> = (0..2).map(|_| Arc::new(AtomicBool::new(false))).collect();
        let (results, completed) = mpsc::channel();
        for (address, stop) in flags.iter().enumerate() {
            let (tx, rx) = mpsc::channel();
            let all_flags = flags.clone();
            let results = results.clone();
            let handle = thread::spawn(move || {
                assert!(matches!(
                    rx.recv_timeout(Duration::from_secs(2)),
                    Ok(Event::Stop)
                ));
                let deadline = Instant::now() + Duration::from_millis(500);
                while !all_flags.iter().all(|flag| flag.load(Ordering::Acquire))
                    && Instant::now() < deadline
                {
                    thread::yield_now();
                }
                results
                    .send(all_flags.iter().all(|flag| flag.load(Ordering::Acquire)))
                    .unwrap();
            });
            engine.workers.insert(
                address as u64,
                Worker {
                    stop: stop.clone(),
                    tx,
                    handle: Some(handle),
                },
            );
        }
        let scanning = Arc::new(AtomicBool::new(false));
        engine.scan = Some(scanning.clone());
        engine.shutdown();
        assert!(scanning.load(Ordering::Acquire));
        assert!(engine.workers.is_empty());
        for _ in 0..2 {
            assert!(completed.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        engine.shutdown();
    }

    #[test]
    fn only_confirmed_pairing_allows_gatt_setup() {
        assert!(check_pairing_result(DevicePairingResultStatus::Paired).is_ok());
        assert!(check_pairing_result(DevicePairingResultStatus::AlreadyPaired).is_ok());
        for status in [
            DevicePairingResultStatus::PairingCanceled,
            DevicePairingResultStatus::AuthenticationTimeout,
            DevicePairingResultStatus::NotReadyToPair,
            DevicePairingResultStatus::AccessDenied,
            DevicePairingResultStatus::AuthenticationFailure,
            DevicePairingResultStatus::RequiredHandlerNotRegistered,
            DevicePairingResultStatus::Failed,
        ] {
            let error = check_pairing_result(status).unwrap_err();
            assert_eq!(error.code(), PAIRING_REQUIRED);
            assert!(error.message().contains("Reconnect"));
            assert!(error.message().contains(&format!("Status {}", status.0)));
        }
    }

    #[test]
    fn pairing_failure_preserves_cause_instead_of_claiming_a_disconnect() {
        let canceled =
            check_pairing_result(DevicePairingResultStatus::PairingCanceled).unwrap_err();
        assert!(canceled.message().contains("canceled"));
        crate::i18n::with_lang(crate::i18n::Lang::Ko, || {
            let canceled =
                check_pairing_result(DevicePairingResultStatus::PairingCanceled).unwrap_err();
            assert!(canceled.message().contains("취소"));
        });
        let timeout =
            check_pairing_result(DevicePairingResultStatus::AuthenticationTimeout).unwrap_err();
        assert!(timeout.message().contains("timed out"));
        assert_ne!(failure("GATT failure").code(), PAIRING_REQUIRED);
    }
}
