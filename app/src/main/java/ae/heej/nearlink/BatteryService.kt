package ae.heej.nearlink

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Handler
import android.os.Looper
import android.os.Build
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import java.util.UUID

class BatteryService : Service() {
    private var server: BluetoothGattServer? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var level: BluetoothGattCharacteristic? = null
    private var statusCharacteristic: BluetoothGattCharacteristic? = null
    private val subscribed = mutableSetOf<Pair<BluetoothDevice, UUID>>()
    private val handler = Handler(Looper.getMainLooper())
    private val notifications = linkedMapOf<Pair<BluetoothDevice, UUID>, ByteArray>()
    private var notifying: BluetoothDevice? = null
    private var notificationDetails: String? = null
    // Session-local identifiers allow correlation without logging Bluetooth addresses.
    private val diagnosticPeers = mutableMapOf<BluetoothDevice, Int>()
    private fun diagnosticPeer(device: BluetoothDevice): Int = synchronized(diagnosticPeers) {
        diagnosticPeers.getOrPut(device) { diagnosticPeers.size + 1 }
    }
    private fun ByteArray?.diagnosticValue(): String = this?.joinToString(":") { "%02x".format(it.toInt() and 0xff) } ?: "null"
    private val notificationRecovery = NotificationRecovery()
    private var notificationRetryPending = false
    private val retryNotifications = Runnable { notificationRetryPending = false; notifySubscribers() }
    @Volatile private var sessionProtocol = SessionHandshake.Server(newSessionId(), android.os.SystemClock::elapsedRealtime)
    @Volatile private var statusValue = BatteryStatusCodec.encode(0, 0, false, false)
    private val notificationTimeout: Runnable = Runnable {
        // A stalled stack must not leave the queue blocked forever.
        val grace = notificationRecovery.timeoutDelay()
        Log.w(TAG, "notification timeout generation=$generation $notificationDetails grace=$grace")
        if (grace != null) handler.postDelayed(notificationTimeout, grace)
        else {
            Log.w(TAG, "repeated notification timeouts; restarting GATT server")
            closeBle()
            startBle()
        }
    }
    @Volatile private var battery = 0
    @Volatile private var generation = 0
    private var waitingForBluetooth = false
    private var readyUntil = 0L
    // Separate from BLE callbacks: restarting Bluetooth must not cancel the safety deadline.
    private val leaseHandler = Handler(Looper.getMainLooper())
    private val leaseExpired = Runnable {
        if (BridgeSession.savedPeer(this) != null) return@Runnable
        Log.i(TAG, "Peer heartbeat expired; stopping bridge")
        BridgeSession.stop(this)
        BridgeStatus.send(SendState.STOPPED, "상대 기기와 연결을 확인하지 못해 중지했어요. 다시 연결해 주세요.")
        BridgeStatus.receive(ReceiveState.STOPPED)
    }

    private fun renewLease() {
        leaseHandler.removeCallbacks(leaseExpired)
        leaseHandler.postDelayed(leaseExpired, 120_000)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_PREPARE) {
            readyUntil = android.os.SystemClock.elapsedRealtime() + 120_000
            renewLease()
        }
        return if (BridgeSession.savedPeer(this) != null) START_STICKY else START_NOT_STICKY
    }

    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_TURNING_OFF, BluetoothAdapter.STATE_OFF -> {
                    waitingForBluetooth = true
                    BridgeStatus.send(SendState.WAITING_FOR_BLUETOOTH)
                    closeBle()
                }
                BluetoothAdapter.STATE_ON -> if (waitingForBluetooth) {
                    waitingForBluetooth = false
                    startBle()
                }
            }
        }
    }

    private var advertiseCallback: AdvertiseCallback? = null

    private fun advertiseCallback(callbackGeneration: Int) = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            if (callbackGeneration != generation) return
            Log.i(TAG, "advertising started")
            if (!waitingForBluetooth) BridgeStatus.send(SendState.ADVERTISING)
        }

        override fun onStartFailure(errorCode: Int) {
            if (callbackGeneration != generation) return
            Log.e(TAG, "advertising failed: $errorCode")
            BridgeStatus.send(SendState.ERROR, "배터리 보내기를 시작하지 못했습니다. 블루투스를 확인하고 다시 시도해 주세요.")
            stopSelf()
        }
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val value = intent.getIntExtra("level", -1)
            val scale = intent.getIntExtra("scale", 100)
            if (value >= 0 && scale > 0) {
                val previous = battery
                battery = (value * 100 / scale).coerceIn(0, 100)
                val chargeState = when (intent.getIntExtra(BatteryManager.EXTRA_STATUS, BatteryManager.BATTERY_STATUS_UNKNOWN)) {
                    BatteryManager.BATTERY_STATUS_CHARGING -> 1
                    BatteryManager.BATTERY_STATUS_DISCHARGING -> 2
                    BatteryManager.BATTERY_STATUS_FULL, BatteryManager.BATTERY_STATUS_NOT_CHARGING -> 3
                    else -> 0
                }
                val plugged = intent.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0)
                statusValue = BatteryStatusCodec.encode(battery, chargeState,
                    plugged and (BatteryManager.BATTERY_PLUGGED_AC or BatteryManager.BATTERY_PLUGGED_USB or 8) != 0,
                    plugged and BatteryManager.BATTERY_PLUGGED_WIRELESS != 0)
                Log.i(TAG, "battery changed previous=$previous level=$battery raw=$value scale=$scale chargeState=$chargeState plugged=$plugged status=${statusValue.diagnosticValue()}")
                notifySubscribers()
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        BridgeStatus.initialize(this)
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel("probe", "BLE battery test", NotificationManager.IMPORTANCE_LOW)
        )
        val notification = Notification.Builder(this, "probe")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle("배터리 블루투스 시험 중")
            .setContentText("시험 앱에서 중지할 수 있습니다.")
            .setOngoing(true)
            .build()
        startForeground(1, notification)
        registerReceiver(receiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED))?.let {
            receiver.onReceive(this, it)
        }
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(bluetoothStateReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED), Context.RECEIVER_EXPORTED)
        } else {
            registerReceiver(bluetoothStateReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))
        }
        startBle()
    }

    private fun startBle() {
        if (server != null) return
        sessionProtocol = SessionHandshake.Server(newSessionId(), android.os.SystemClock::elapsedRealtime)
        val currentGeneration = ++generation
        Log.i(TAG, "GATT server starting generation=$currentGeneration")
        BridgeStatus.send(SendState.STARTING)
        try {
            val manager = getSystemService(BluetoothManager::class.java)
            val adapter: BluetoothAdapter? = manager.adapter
            if (adapter == null || !adapter.isEnabled) {
                Log.e(TAG, "Bluetooth disabled")
                waitingForBluetooth = true
                BridgeStatus.send(SendState.WAITING_FOR_BLUETOOTH)
                return
            }
            advertiser = adapter.bluetoothLeAdvertiser
            if (advertiser == null) {
                Log.e(TAG, "BLE advertising unavailable")
                BridgeStatus.send(SendState.ERROR, "배터리 보내기를 사용할 수 없습니다. 블루투스 상태와 기기의 BLE 지원 여부를 확인해 주세요.")
                stopSelf()
                return
            }
            val openedServer = manager.openGattServer(this, serverCallback(currentGeneration))
            server = openedServer
            if (openedServer == null) {
                Log.e(TAG, "GATT server unavailable")
                BridgeStatus.send(SendState.ERROR, "배터리 서비스를 열지 못했습니다. 블루투스를 확인하고 다시 시도해 주세요.")
                stopSelf()
                return
            }
            val service = BluetoothGattService(BAS, BluetoothGattService.SERVICE_TYPE_PRIMARY)
            val characteristic = BluetoothGattCharacteristic(
                LEVEL,
                BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED
            )
            characteristic.addDescriptor(
                BluetoothGattDescriptor(CCC, BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED or BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED)
            )
            level = characteristic
            service.addCharacteristic(characteristic)
            val status = BluetoothGattCharacteristic(STATUS,
                BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED)
            status.addDescriptor(BluetoothGattDescriptor(CCC,
                BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED or BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED))
            statusCharacteristic = status
            service.addCharacteristic(status)
            service.addCharacteristic(BluetoothGattCharacteristic(
                HANDSHAKE, BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_WRITE_ENCRYPTED))
            service.addCharacteristic(BluetoothGattCharacteristic(SESSION,
                BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED or BluetoothGattCharacteristic.PERMISSION_WRITE_ENCRYPTED))
            if (!openedServer.addService(service)) {
                Log.e(TAG, "failed to register BAS")
                BridgeStatus.send(SendState.ERROR, "배터리 서비스를 등록하지 못했습니다. 다시 시도해 주세요.")
                stopSelf()
            }
        } catch (e: SecurityException) {
            Log.e(TAG, "Bluetooth permission denied", e)
            BridgeStatus.send(SendState.ERROR, "주변 기기 권한을 허용한 뒤 다시 시도해 주세요.")
            stopSelf()
        }
    }

    private fun serverCallback(callbackGeneration: Int) = object : BluetoothGattServerCallback() {
        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            if (callbackGeneration != generation) return
            if (status != BluetoothGatt.GATT_SUCCESS) {
                Log.e(TAG, "service registration failed: $status")
                BridgeStatus.send(SendState.ERROR, "배터리 서비스를 등록하지 못했습니다. 다시 시도해 주세요.")
                stopSelf()
                return
            }
            try {
                val settings = AdvertiseSettings.Builder()
                    .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_POWER)
                    .setConnectable(true)
                    .build()
                val data = AdvertiseData.Builder().addServiceUuid(ParcelUuid(BAS)).addServiceUuid(ParcelUuid(PairingModel.BRIDGE_UUID)).build()
                val response = AdvertiseData.Builder().setIncludeDeviceName(true).build()
                val activeCallback = advertiseCallback(callbackGeneration)
                advertiseCallback = activeCallback
                advertiser?.startAdvertising(settings, data, response, activeCallback)
            } catch (e: SecurityException) {
                Log.e(TAG, "advertise permission denied", e)
                BridgeStatus.send(SendState.ERROR, "주변 기기 권한을 허용한 뒤 다시 시도해 주세요.")
                stopSelf()
            }
        }

        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            if (callbackGeneration != generation) return
            Log.i(TAG, "connection generation=$callbackGeneration peer=${diagnosticPeer(device)} state=$newState status=$status")
            if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                sessionProtocol.forget(device.address)
                synchronized(subscribed) { subscribed.removeAll { it.first == device } }
                handler.post {
                    if (callbackGeneration != generation) return@post
                    notifications.keys.removeAll { it.first == device }
                    if (notifying == device) {
                        notifying = null
                        notificationDetails = null
                        handler.removeCallbacks(notificationTimeout)
                        drainNotifications()
                    }
                }
            }
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            handler.post {
                if (callbackGeneration != generation || notifying != device) {
                    Log.w(TAG, "notification callback ignored generation=$callbackGeneration peer=${diagnosticPeer(device)} status=$status currentGeneration=$generation")
                    return@post
                }
                Log.i(TAG, "notification callback generation=$callbackGeneration $notificationDetails status=$status")
                handler.removeCallbacks(notificationTimeout)
                notifying = null
                notificationDetails = null
                if (status != BluetoothGatt.GATT_SUCCESS) scheduleNotificationRetry()
                else {
                    notificationRecovery.success()
                    handler.removeCallbacks(retryNotifications)
                    notificationRetryPending = false
                    drainNotifications()
                }
            }
        }

        override fun onCharacteristicWriteRequest(device: BluetoothDevice, requestId: Int,
            characteristic: BluetoothGattCharacteristic, preparedWrite: Boolean, responseNeeded: Boolean,
            offset: Int, value: ByteArray) {
            if (callbackGeneration != generation) return
            handler.post {
                if (callbackGeneration != generation) return@post
                try {
                    if (characteristic.uuid == SESSION) {
                        val packet = SessionHandshake.decode(value)
                        val authenticated = isAuthorizedPeer(device) && !preparedWrite && responseNeeded && offset == 0
                        val subscribedToBattery = synchronized(subscribed) {
                            (device to LEVEL) in subscribed && (device to STATUS) in subscribed
                        }
                        val accepted = if (!authenticated || packet == null) false else when (packet.kind) {
                            SessionHandshake.HELLO -> sessionProtocol.hello(device.address, packet)
                            SessionHandshake.ACK -> sessionProtocol.acknowledge(device.address, packet, subscribedToBattery)
                            else -> false
                        }
                        if (responseNeeded) server?.sendResponse(device, requestId,
                            if (accepted) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE, 0, null)
                        Log.i(TAG, "session write kind=${packet?.kind} accepted=$accepted generation=$generation")
                        if (accepted && packet?.kind == SessionHandshake.ACK) notifySubscribers()
                        return@post
                    }
                    val valid = characteristic.uuid == HANDSHAKE && !preparedWrite && responseNeeded &&
                        offset == 0 && device.bondState == BluetoothDevice.BOND_BONDED &&
                        BridgeSession.hasPermissions(this@BatteryService)
                    val current = BridgeSession.savedPeer(this@BatteryService) == device.address
                    val command = BridgeControl.accept(value, valid, current,
                        android.os.SystemClock.elapsedRealtime() < readyUntil)
                    if (command == BridgeControl.Command.JOIN) {
                        try {
                            if (!current) BridgeSession.start(this@BatteryService, device.address)
                            readyUntil = 0L
                            renewLease()
                            server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                        } catch (e: Exception) {
                            Log.e(TAG, "Could not start reciprocal connection", e)
                            server?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, 0, null)
                        }
                    } else if (command == BridgeControl.Command.STOP) {
                        BridgeSession.disable(this@BatteryService)
                        server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                        leaseHandler.postDelayed({
                            BridgeSession.stop(this@BatteryService)
                            BridgeStatus.send(SendState.STOPPED, "상대 기기에서 연결을 중지했어요.")
                            BridgeStatus.receive(ReceiveState.STOPPED)
                        }, 300)
                    } else if (responseNeeded) server?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, 0, null)
                } catch (e: SecurityException) { Log.e(TAG, "Handshake failed", e) }
            }
        }

        override fun onCharacteristicReadRequest(device: BluetoothDevice, requestId: Int, offset: Int, characteristic: BluetoothGattCharacteristic) {
            if (callbackGeneration != generation) return
            val value = when (characteristic.uuid) {
                LEVEL -> byteArrayOf(battery.toByte())
                STATUS -> statusValue.copyOf()
                SESSION -> sessionProtocol.response(device.address)
                else -> null
            }
            val result = when {
                value == null -> BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
                !isAuthorizedPeer(device) -> BluetoothGatt.GATT_FAILURE
                offset !in 0..value.size -> BluetoothGatt.GATT_INVALID_OFFSET
                else -> BluetoothGatt.GATT_SUCCESS
            }
            try {
                val response = if (result == BluetoothGatt.GATT_SUCCESS) value?.copyOfRange(offset, value.size) else null
                val sent = server?.sendResponse(device, requestId, result, offset, response)
                Log.i(TAG, "read generation=$callbackGeneration peer=${diagnosticPeer(device)} request=$requestId characteristic=${characteristic.uuid} offset=$offset result=$result value=${response.diagnosticValue()} responseSent=$sent")
            } catch (e: SecurityException) { Log.e(TAG, "read response failed", e) }
        }

        override fun onDescriptorReadRequest(device: BluetoothDevice, requestId: Int, offset: Int, descriptor: BluetoothGattDescriptor) {
            if (callbackGeneration != generation) return
            val value = synchronized(subscribed) {
                if ((device to descriptor.characteristic.uuid) in subscribed) BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE else BluetoothGattDescriptor.DISABLE_NOTIFICATION_VALUE
            }
            try {
                val result = when {
                    descriptor.uuid != CCC || descriptor.characteristic.uuid !in listOf(LEVEL, STATUS) -> BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
                    !isAuthorizedPeer(device) -> BluetoothGatt.GATT_FAILURE
                    offset != 0 -> BluetoothGatt.GATT_INVALID_OFFSET
                    else -> BluetoothGatt.GATT_SUCCESS
                }
                val sent = server?.sendResponse(device, requestId, result, offset, if (result == BluetoothGatt.GATT_SUCCESS) value else null)
                Log.i(TAG, "descriptor read generation=$callbackGeneration peer=${diagnosticPeer(device)} request=$requestId characteristic=${descriptor.characteristic.uuid} descriptor=${descriptor.uuid} offset=$offset result=$result value=${if (result == BluetoothGatt.GATT_SUCCESS) value.diagnosticValue() else "null"} responseSent=$sent")
            } catch (e: SecurityException) { Log.e(TAG, "descriptor read failed", e) }
        }

        override fun onDescriptorWriteRequest(device: BluetoothDevice, requestId: Int, descriptor: BluetoothGattDescriptor, preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray) {
            if (callbackGeneration != generation) return
            val enabled = value.contentEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)
            val disabled = value.contentEquals(BluetoothGattDescriptor.DISABLE_NOTIFICATION_VALUE)
            val valid = descriptor.uuid == CCC && descriptor.characteristic.uuid in listOf(LEVEL, STATUS) &&
                isAuthorizedPeer(device) && !preparedWrite && offset == 0 && (enabled || disabled)
            if (valid) synchronized(subscribed) {
                val key = device to descriptor.characteristic.uuid
                if (enabled) subscribed.add(key) else subscribed.remove(key)
            }
            val subscriptionCount = synchronized(subscribed) { subscribed.size }
            try {
                val sent = if (responseNeeded) server?.sendResponse(device, requestId, if (valid) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE, offset, null) else null
                Log.i(TAG, "descriptor write generation=$callbackGeneration peer=${diagnosticPeer(device)} request=$requestId characteristic=${descriptor.characteristic.uuid} descriptor=${descriptor.uuid} value=${value.diagnosticValue()} prepared=$preparedWrite offset=$offset authorized=${isAuthorizedPeer(device)} valid=$valid responseNeeded=$responseNeeded responseSent=$sent subscriptions=$subscriptionCount")
            } catch (e: SecurityException) { Log.e(TAG, "descriptor write failed", e) }
            if (valid && enabled) notifySubscribers()
        }
    }

    private fun notifySubscribers() {
        handler.post {
            synchronized(subscribed) {
                subscribed.removeAll { !isAuthorizedPeer(it.first) }
                subscribed.forEach { key ->
                    notifications[key] = if (key.second == LEVEL) byteArrayOf(battery.toByte()) else statusValue.copyOf()
                }
                Log.i(TAG, "notification enqueue generation=$generation subscriptions=${subscribed.size} queued=${notifications.size}")
            }
            drainNotifications()
        }
    }

    private fun scheduleNotificationRetry() {
        notificationRetryPending = true
        handler.removeCallbacks(retryNotifications)
        handler.postDelayed(retryNotifications, notificationRecovery.failureDelay())
    }

    @Suppress("DEPRECATION")
    private fun drainNotifications() {
        if (notifying != null || notificationRetryPending) return
        val activeServer = server ?: return
        while (notifications.isNotEmpty()) {
            val entry = notifications.entries.first()
            val key = entry.key
            val value = entry.value
            notifications.remove(key)
            if (!synchronized(subscribed) { key in subscribed } || !isAuthorizedPeer(key.first)) continue
            val characteristic = (if (key.second == LEVEL) level else statusCharacteristic) ?: continue
            try {
                notifying = key.first
                notificationDetails = "peer=${diagnosticPeer(key.first)} characteristic=${key.second} value=${value.diagnosticValue()}"
                val (accepted, requestResult) = if (Build.VERSION.SDK_INT >= 33) {
                    val result = activeServer.notifyCharacteristicChanged(key.first, characteristic, false, value)
                    (result == BluetoothStatusCodes.SUCCESS) to result.toString()
                } else {
                    characteristic.value = value
                    val result = activeServer.notifyCharacteristicChanged(key.first, characteristic, false)
                    result to result.toString()
                }
                Log.i(TAG, "notification request generation=$generation $notificationDetails accepted=$accepted result=$requestResult queued=${notifications.size}")
                if (accepted) {
                    handler.postDelayed(notificationTimeout, 15_000)
                    return
                }
                notifying = null
                notificationDetails = null
                // Re-enqueue latest values after a busy/error response, rather than losing a change.
                scheduleNotificationRetry()
                return
            } catch (e: SecurityException) {
                notifying = null
                notificationDetails = null
                Log.e(TAG, "notify denied", e)
            }
        }
    }

    private fun isAuthorizedPeer(device: BluetoothDevice): Boolean = try {
        device.bondState == BluetoothDevice.BOND_BONDED &&
            device.address.equals(BridgeSession.savedPeer(this), ignoreCase = true)
    } catch (_: SecurityException) {
        false
    }

    private fun closeBle() {
        Log.i(TAG, "GATT server closing generation=$generation")
        generation++
        handler.removeCallbacksAndMessages(null)
        notifications.clear()
        notificationRecovery.clear()
        notificationRetryPending = false
        notifying = null
        notificationDetails = null
        try {
            advertiseCallback?.let { advertiser?.stopAdvertising(it) }
        } catch (e: SecurityException) { Log.e(TAG, "cleanup failed", e) }
        try { server?.close() }
        catch (e: SecurityException) { Log.e(TAG, "server cleanup failed", e) }
        advertiser = null
        advertiseCallback = null
        server = null
        level = null
        statusCharacteristic = null
        synchronized(subscribed) { subscribed.clear() }
    }

    override fun onDestroy() {
        leaseHandler.removeCallbacksAndMessages(null)
        unregisterReceiver(bluetoothStateReceiver)
        closeBle()
        unregisterReceiver(receiver)
        BridgeStatus.sendStopped()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val ACTION_PREPARE = "ae.heej.nearlink.PREPARE"
        private fun newSessionId(): Long = UUID.randomUUID().mostSignificantBits.let { if (it == 0L) 1L else it }
        val SESSION: UUID = UUID.fromString("734b7302-53b1-4cf0-8d2e-d4b5c742b65b")
        val HANDSHAKE: UUID = UUID.fromString("734b7301-53b1-4cf0-8d2e-d4b5c742b65b")
        private const val TAG = "BatteryBleProbe"
        private val BAS = UUID.fromString("0000180f-0000-1000-8000-00805f9b34fb")
        private val STATUS = UUID.fromString("00002bed-0000-1000-8000-00805f9b34fb")
        private val LEVEL = UUID.fromString("00002a19-0000-1000-8000-00805f9b34fb")
        private val CCC = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }
}
