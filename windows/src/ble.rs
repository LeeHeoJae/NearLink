use crate::{
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
        Enumeration::{DevicePairingProtectionLevel, DevicePairingResultStatus},
    },
    Foundation::TypedEventHandler,
    Storage::Streams::{DataReader, DataWriter, IBuffer},
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    core::{Error, GUID, HRESULT, IInspectable, Result, RuntimeType},
};
use windows_future::{AsyncStatus, IAsyncOperation};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

fn failure(text: &str) -> Error {
    Error::new(HRESULT(0x80004005u32 as i32), text)
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
                "응답 시간이 지났습니다. 기기의 연결 준비와 블루투스를 확인해 주세요.",
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    operation.GetResults()
}

fn bytes(buffer: &IBuffer) -> Result<Vec<u8>> {
    let length = buffer.Length()? as usize;
    if length > 512 {
        return Err(failure("지원하지 않는 배터리 데이터입니다."));
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
            "기기가 요청을 거절했습니다. Android NearLink를 업데이트하고 연결 준비를 눌러 주세요.",
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
        return Err(failure("배터리 정보를 읽지 못했습니다."));
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
            "최신 NearLink 배터리 서비스를 찾지 못했습니다. Android 앱을 업데이트해 주세요.",
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
        return Err(failure("배터리 변경 알림을 설정하지 못했습니다."));
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

fn connection(
    shared: &Shared,
    address: u64,
    pair: bool,
    stop: &AtomicBool,
    rx: &mpsc::Receiver<Event>,
    tx: &mpsc::Sender<Event>,
    wake: &Wake,
) -> Result<()> {
    set_state(shared, address, State::Connecting, "", wake);
    let saved = shared
        .lock()
        .unwrap()
        .peers
        .get(&address)
        .ok_or_else(|| failure("기기가 제거되었습니다."))?
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
        service: None,
        tokens: vec![],
        changed: None,
        disconnected: None,
    };
    let pairing = link.device.DeviceInformation()?.Pairing()?;
    if !pairing.IsPaired()? {
        if !pair {
            return Err(failure(
                "페어링이 필요합니다. 기기를 다시 추가하거나 Windows 블루투스 설정에서 페어링해 주세요.",
            ));
        }
        set_state(
            shared,
            address,
            State::Connecting,
            "양쪽 기기의 페어링 요청을 확인해 주세요.",
            wake,
        );
        let result = wait(
            pairing.PairWithProtectionLevelAsync(DevicePairingProtectionLevel::Encryption)?,
            stop,
            60,
        )?;
        if result.Status()? != DevicePairingResultStatus::Paired
            && result.Status()? != DevicePairingResultStatus::AlreadyPaired
        {
            return Err(failure(
                "페어링을 완료하지 못했습니다. Windows 블루투스 설정에서 먼저 페어링한 뒤 다시 연결해 주세요.",
            ));
        }
    }
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
    if result.Status()? != GattCommunicationStatus::Success || result.Services()?.Size()? == 0 {
        return Err(failure(
            "기기의 NearLink에서 연결 준비를 누르고 가까이 두세요. 블루투스가 꺼져 있으면 켜 주세요.",
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
        .ok_or_else(|| failure("기기의 연결 세션을 확인하지 못했습니다."))?;
    let mut level = read(&level_char, stop)?
        .first()
        .copied()
        .filter(|v| *v <= 100);
    if level.is_none() {
        return Err(failure("잘못된 배터리 잔량입니다."));
    }
    let initial_status = protocol::battery_status(&read(&status_char, stop)?)
        .ok_or_else(|| failure("잘못된 배터리 상태입니다."))?;
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
            return Err(failure("연결이 변경되어 다시 연결합니다."));
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
                    return Err(failure("배터리 알림이 도착하지 않아 다시 연결합니다."));
                }
                write(&control, &[protocol::JOIN_RECEIVER], stop)?;
                if protocol::ready(&read(&session, stop)?, request, Some(epoch)).is_none() {
                    return Err(failure("기기의 배터리 서비스가 다시 시작되었습니다."));
                }
                deadline = Instant::now() + Duration::from_secs(30);
                continue;
            }
        };
        match event {
            Event::Stop => return Ok(()),
            Event::Invalidated => {
                return Err(failure(
                    "기기의 연결이나 서비스가 변경되어 다시 연결합니다.",
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
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.tx.send(Event::Stop);
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
}

impl Engine {
    pub fn new(shared: Shared, wake: Wake) -> Self {
        Self {
            shared,
            wake,
            workers: HashMap::new(),
            scan: None,
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
        let handle = thread::spawn(move || {
            let _apartment = match Apartment::new() {
                Ok(value) => value,
                Err(error) => {
                    set_state(&shared, address, State::Stopped, &error.to_string(), &wake);
                    return;
                }
            };
            let mut delay = 1u64;
            let mut first = pair;
            while !stop.load(Ordering::Acquire) {
                let started = Instant::now();
                let result = connection(&shared, address, first, &stop, &rx, &tx, &wake);
                first = false;
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if started.elapsed() > Duration::from_secs(90) {
                    delay = 1;
                }
                let detail = result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "연결이 끊어졌습니다.".into());
                set_state(
                    &shared,
                    address,
                    State::Retrying,
                    &format!("{detail} {delay}초 후 재시도합니다."),
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
            "이 PC의 수신을 중지했습니다.",
            &self.wake,
        );
    }
    pub fn cancel_scan(&mut self) {
        if let Some(stop) = self.scan.take() {
            stop.store(true, Ordering::Release);
        }
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
                        format!("기기를 검색하지 못했습니다. 블루투스를 확인해 주세요. {error}");
                } else if model.discovered.is_empty() {
                    model.message =
                        "기기를 찾지 못했습니다. Android에서 연결 준비를 누르고 다시 검색하세요."
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
        self.cancel_scan();
        self.workers.clear();
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
                    format!("NearLink 기기 · {:04X}", address & 0xffff)
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
                    "검색이 중단되었습니다. PC의 블루투스 어댑터와 권한을 확인하세요.",
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
