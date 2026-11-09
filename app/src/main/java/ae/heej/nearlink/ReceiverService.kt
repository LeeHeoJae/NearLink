package ae.heej.nearlink

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Intent
import android.content.Context
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.ParcelUuid
import android.util.Log
import java.util.UUID

class ReceiverService : Service() {
    private val bluetoothReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR) == BluetoothAdapter.STATE_ON) {
                handler.removeCallbacks(reconnect)
                reconnectAttempts = 0
                if (device == null) {
                    BridgeSession.savedPeer(this@ReceiverService)?.let { onStartCommand(Intent().putExtra("address", it), 0, 0) }
                } else if (gatt == null) scanForPeer()
            }
        }
    }
    @Volatile private var gatt: BluetoothGatt? = null
    private var device: BluetoothDevice? = null
    private var peerName = "선택한 기기"
    private val handler = Handler(Looper.getMainLooper())
    @Volatile private var stopping = false
    private val reconnect = Runnable { scanForPeer() }
    private var reconnectAttempts = 0
    private var scanner: BluetoothLeScanner? = null
    private var scanning = false
    // GATT discovery and reads are serialized on the main handler.
    private var discovering = false
    private var reading = false
    private var rediscoverPending = false
    private data class Operation(val characteristic: BluetoothGattCharacteristic, val subscribe: Boolean, val handshake: Boolean = false, val stop: Boolean = false)
    private val operations = java.util.ArrayDeque<Operation>()
    private var activeOperation: Operation? = null
    private var lastLevel: Int? = null
    private var charging: Boolean? = null
    private var plugged: Boolean? = null
    private var stopRequested = false
    private val stopTimeout = Runnable { finishStop() }
    private val heartbeat = object : Runnable {
        override fun run() {
            if (stopping || stopRequested) return
            val active = gatt
            val characteristic = active?.getService(BAS)?.getCharacteristic(BatteryService.HANDSHAKE)
            if (active != null && characteristic != null && activeOperation?.handshake != true && operations.none { it.handshake }) {
                operations.add(Operation(characteristic, false, handshake = true))
                if (activeOperation == null && !discovering) runNextOperation(active)
            }
            handler.postDelayed(this, 30_000)
        }
    }

    private fun finishStop() {
        handler.removeCallbacks(stopTimeout)
        BridgeSession.stop(this)
        BridgeStatus.send(SendState.STOPPED, BridgeSession.stopNotice(this))
        BridgeStatus.receive(ReceiveState.STOPPED)
    }

    private fun requestPeerStop() {
        stopRequested = true
        handler.removeCallbacks(heartbeat)
        handler.postDelayed(stopTimeout, 3_000)
        val active = gatt
        val characteristic = active?.getService(BAS)?.getCharacteristic(BatteryService.HANDSHAKE)
        if (active == null || characteristic == null || discovering) { finishStop(); return }
        operations.clear()
        operations.addFirst(Operation(characteristic, false, handshake = true, stop = true))
        if (activeOperation == null) runNextOperation(active)
    }

    private val operationTimeout = Runnable { scheduleReconnect("응답이 없어 다시 연결합니다.") }

    private fun armTimeout() {
        handler.removeCallbacks(operationTimeout)
        handler.postDelayed(operationTimeout, 20_000)
    }

    private fun resetOperations() {
        handler.removeCallbacks(operationTimeout)
        operations.clear()
        activeOperation = null
        lastLevel = null
        charging = null
        plugged = null
    }

    private val scanTimeout = Runnable {
        if (scanning) {
            Log.i(TAG, "peer not advertising battery service; retry later")
            stopScan()
            try {
                val manager = getSystemService(BluetoothManager::class.java)
                if (device != null && manager.getConnectedDevices(BluetoothProfile.GATT).contains(device)) {
                    Log.i(TAG, "peer already has a GATT link; trying existing connection")
                    connect()
                } else scheduleReconnect("상대 기기의 배터리 보내기가 켜져 있는지 확인해 주세요.")
            } catch (e: SecurityException) {
                Log.e(TAG, "connection state permission denied", e)
                BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
                stopSelf()
            }
        }
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            handler.post {
                if (!scanning || stopping || device == null || device != result.device) return@post
                Log.i(TAG, "paired peer advertising battery service")
                stopScan()
                connect()
            }
        }

        override fun onScanFailed(errorCode: Int) {
            handler.post {
                if (!scanning || stopping) return@post
                Log.e(TAG, "scan failed: $errorCode")
                stopScan()
                scheduleReconnect("기기 검색에 실패했습니다. 블루투스를 확인해 주세요.")
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(bluetoothReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED), Context.RECEIVER_EXPORTED)
        else registerReceiver(bluetoothReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))
        BridgeStatus.initialize(this)
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel("receiver", "BLE battery test", NotificationManager.IMPORTANCE_LOW)
        )
        startForeground(2, Notification.Builder(this, "receiver")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle("배터리 연결 유지 시험 중")
            .setOngoing(true)
            .build())
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null) {
            val saved = BridgeSession.savedPeer(this) ?: run { stopSelf(); return START_NOT_STICKY }
            return onStartCommand(Intent().putExtra("address", saved), flags, startId)
        }
        if (intent.action == ACTION_STOP_PEER) { requestPeerStop(); return START_NOT_STICKY }
        handler.removeCallbacks(heartbeat)
        handler.postDelayed(heartbeat, 30_000)
        val peer = intent.getStringExtra("peer")
        val address = intent.getStringExtra("address")
        if (peer == null && address == null) {
            Log.e(TAG, "peer missing")
            BridgeStatus.receive(ReceiveState.ERROR, "연결할 기기를 다시 선택해 주세요.")
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            val adapter = getSystemService(BluetoothManager::class.java).adapter
            if (adapter != null && !adapter.isEnabled) {
                BridgeStatus.receive(ReceiveState.WAITING_FOR_BLUETOOTH, "블루투스를 켜면 다시 연결을 시도합니다.")
                return START_STICKY
            }
            val selected = adapter.bondedDevices.firstOrNull { candidate ->
                (address != null && address == candidate.address) ||
                    (peer != null && (peer == candidate.name || (Build.VERSION.SDK_INT >= 30 && peer == candidate.alias)))
            }
            if (selected == null) {
                Log.e(TAG, "paired peer not found")
                BridgeStatus.receive(ReceiveState.ERROR, "페어링된 기기를 찾지 못했습니다. 블루투스 설정을 확인하고 다시 선택해 주세요.")
                stopSelf()
                return START_NOT_STICKY
            }
            if (selected == device && gatt != null) {
                val active = gatt!!
                val handshake = active.getService(BAS)?.getCharacteristic(BatteryService.HANDSHAKE)
                if (handshake != null && activeOperation?.handshake != true && operations.none { it.handshake }) {
                    operations.add(Operation(handshake, false, true))
                    if (activeOperation == null && !discovering) runNextOperation(active)
                }
                return START_STICKY
            }
            if (selected == device && scanning) return START_STICKY
            handler.removeCallbacks(reconnect)
            stopScan()
            resetOperations()
            if (selected != device) {
                val old = gatt
                gatt = null
                discovering = false
                reading = false
                rediscoverPending = false
                old?.disconnect()
                old?.close()
            }
            device = selected
            peerName = (if (Build.VERSION.SDK_INT >= 30) selected.alias else selected.name) ?: "이름 없는 기기"
            BridgeStatus.selectPeer(selected.address, peerName)
            reconnectAttempts = 0
            scanForPeer()
        } catch (e: SecurityException) {
            Log.e(TAG, "permission denied", e)
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
        }
        return START_STICKY
    }

    private fun scanForPeer() {
        val peer = device ?: return
        if (stopping || scanning) return
        try {
            val adapter = getSystemService(BluetoothManager::class.java).adapter
            val activeScanner = if (adapter != null && adapter.isEnabled) adapter.bluetoothLeScanner else null
            scanner = activeScanner
            if (activeScanner == null) {
                Log.e(TAG, "BLE scanner unavailable")
                if (adapter == null || !adapter.isEnabled) {
                    scheduleReconnect("블루투스를 켜면 다시 연결을 시도합니다.", waitingForBluetooth = true)
                } else scheduleReconnect("기기 검색을 사용할 수 없습니다. 블루투스를 확인해 주세요.")
                return
            }
            val filter = ScanFilter.Builder().setDeviceAddress(peer.address).setServiceUuid(ParcelUuid(BAS)).build()
            val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_POWER).build()
            scanning = true
            activeScanner.startScan(listOf(filter), settings, scanCallback)
            BridgeStatus.receive(ReceiveState.SEARCHING)
            handler.postDelayed(scanTimeout, 15_000)
            Log.i(TAG, "waiting for paired peer battery advertisement")
        } catch (e: SecurityException) {
            Log.e(TAG, "scan permission denied", e)
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
        } catch (e: IllegalStateException) {
            Log.e(TAG, "scan unavailable", e)
            stopScan()
            scheduleReconnect()
        }
    }

    private fun stopScan() {
        handler.removeCallbacks(scanTimeout)
        if (!scanning) return
        scanning = false
        try { scanner?.stopScan(scanCallback) }
        catch (e: SecurityException) { Log.e(TAG, "stop scan permission denied", e) }
        scanner = null
    }

    private fun connect() {
        val peer = device ?: return
        if (stopping) return
        try {
            handler.removeCallbacks(reconnect)
            val old = gatt
            gatt = null
            old?.close()
            discovering = false
            reading = false
            rediscoverPending = false
            resetOperations()
            armTimeout()
            gatt = peer.connectGatt(this, false, callback, BluetoothDevice.TRANSPORT_LE)
            if (gatt == null) {
                Log.e(TAG, "connection request failed")
                scheduleReconnect()
            } else {
                BridgeStatus.receive(ReceiveState.CONNECTING)
                Log.i(TAG, "connection requested")
            }
        } catch (e: SecurityException) {
            Log.e(TAG, "connect permission denied", e)
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
        }
    }

    private fun scheduleReconnect(detail: String? = null, waitingForBluetooth: Boolean = false) {
        if (stopping) return
        BridgeStatus.receive(if (waitingForBluetooth) ReceiveState.WAITING_FOR_BLUETOOTH else ReceiveState.RETRYING, detail)
        handler.removeCallbacks(reconnect)
        resetOperations()
        val failed = gatt
        gatt = null
        discovering = false
        reading = false
        rediscoverPending = false
        try { failed?.disconnect(); failed?.close() }
        catch (e: SecurityException) { Log.e(TAG, "failed client cleanup denied", e) }
        val delay = minOf(300_000L, 5_000L shl minOf(reconnectAttempts++, 6))
        handler.postDelayed(reconnect, delay)
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onServiceChanged(gatt: BluetoothGatt) {
            handler.post {
                if (stopping || gatt !== this@ReceiverService.gatt) return@post
                Log.i(TAG, "peer services changed; rediscovery requested")
                if (discovering || reading) rediscoverPending = true else startDiscovery(gatt)
            }
        }

        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            handler.post {
                if (stopping || gatt !== this@ReceiverService.gatt) return@post
                Log.i(TAG, "state=$newState status=$status")
                try {
                    if (newState == BluetoothProfile.STATE_CONNECTED && status == BluetoothGatt.GATT_SUCCESS) {
                        startDiscovery(gatt)
                    } else if (newState == BluetoothProfile.STATE_DISCONNECTED || status != BluetoothGatt.GATT_SUCCESS) {
                        scheduleReconnect()
                    }
                } catch (e: SecurityException) { Log.e(TAG, "connection error", e) }
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            handler.post { handleServicesDiscovered(gatt, status) }
        }

        override fun onCharacteristicRead(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray, status: Int) {
            handler.post { handleRead(gatt, characteristic, value, status) }
        }

        @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
        override fun onCharacteristicRead(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
            val value = characteristic.value?.copyOf()
            handler.post { handleRead(gatt, characteristic, value, status) }
        }

        override fun onCharacteristicWrite(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
            handler.post {
                if (stopping || gatt !== this@ReceiverService.gatt) return@post
                val operation = activeOperation ?: return@post
                if (!operation.handshake || operation.characteristic !== characteristic) return@post
                if (operation.stop) {
                    if (status != BluetoothGatt.GATT_SUCCESS) Log.w(TAG, "Stop request rejected: $status")
                    finishStop()
                    return@post
                }
                if (status != BluetoothGatt.GATT_SUCCESS) Log.w(TAG, "Reciprocal setup was not accepted: $status")
                finishOperation(gatt)
            }
        }

        override fun onDescriptorWrite(gatt: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            handler.post {
                if (stopping || gatt !== this@ReceiverService.gatt) return@post
                val operation = activeOperation ?: return@post
                if (!operation.subscribe || operation.characteristic !== descriptor.characteristic) return@post
                if (status != BluetoothGatt.GATT_SUCCESS) { scheduleReconnect("변경 알림을 설정하지 못해 다시 연결합니다."); return@post }
                finishOperation(gatt)
            }
        }

        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
            val snapshot = value.copyOf()
            handler.post { if (!stopping && gatt === this@ReceiverService.gatt) acceptValue(characteristic.uuid, snapshot) }
        }

        @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic) {
            val snapshot = characteristic.value?.copyOf() ?: return
            handler.post { if (!stopping && gatt === this@ReceiverService.gatt) acceptValue(characteristic.uuid, snapshot) }
        }
    }

    private fun startDiscovery(active: BluetoothGatt) {
        if (stopping || active !== gatt || discovering || reading) return
        try {
            discovering = true
            armTimeout()
            BridgeStatus.receive(ReceiveState.READING)
            if (!active.discoverServices()) { discovering = false; scheduleReconnect() }
        } catch (e: SecurityException) {
            discovering = false
            Log.e(TAG, "service discovery denied", e)
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
        }
    }

    private fun handleServicesDiscovered(active: BluetoothGatt, status: Int) {
        if (stopping || active !== gatt || !discovering) return
        discovering = false
        Log.i(TAG, "services discovered status=$status")
        if (status != BluetoothGatt.GATT_SUCCESS) { scheduleReconnect(); return }
        if (rediscoverPending) {
            rediscoverPending = false
            startDiscovery(active)
            return
        }
        val service = active.getService(BAS)
        if (service == null) { Log.e(TAG, "BAS missing"); scheduleReconnect("상대 기기에서 배터리 서비스를 찾지 못했습니다. 배터리 보내기를 확인해 주세요."); return }
        val level = service.getCharacteristic(LEVEL)
        if (level == null) { scheduleReconnect("상대 기기에 잔량 정보가 없습니다."); return }
        lastLevel = null
        charging = null
        plugged = null
        operations.clear()
        service.getCharacteristic(BatteryService.HANDSHAKE)?.let { operations.add(Operation(it, false, true)) }
        operations.add(Operation(level, false))
        service.getCharacteristic(STATUS)?.let { operations.add(Operation(it, false)) }
        for (uuid in listOf(LEVEL, STATUS)) {
            service.getCharacteristic(uuid)?.let {
                if (it.properties and (BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_INDICATE) != 0) {
                    operations.add(Operation(it, true))
                }
            }
        }
        runNextOperation(active)
    }

    @Suppress("DEPRECATION")
    private fun runNextOperation(active: BluetoothGatt) {
        if (stopping || active !== gatt) return
        val operation = operations.pollFirst()
        activeOperation = operation
        reading = operation != null
        if (operation == null) {
            handler.removeCallbacks(operationTimeout)
            if (rediscoverPending) { rediscoverPending = false; startDiscovery(active) }
            return
        }
        try {
            armTimeout()
            val accepted = if (operation.handshake) {
                val payload = byteArrayOf(if (operation.stop) 2 else 1)
                if (Build.VERSION.SDK_INT >= 33) active.writeCharacteristic(operation.characteristic, payload, BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothStatusCodes.SUCCESS
                else {
                    operation.characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                    operation.characteristic.value = payload
                    active.writeCharacteristic(operation.characteristic)
                }
            } else if (!operation.subscribe) active.readCharacteristic(operation.characteristic) else {
                val descriptor = operation.characteristic.getDescriptor(CCC)
                if (descriptor == null || !active.setCharacteristicNotification(operation.characteristic, true)) false
                else {
                    val value = if (operation.characteristic.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0)
                        BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE else BluetoothGattDescriptor.ENABLE_INDICATION_VALUE
                    if (Build.VERSION.SDK_INT >= 33) active.writeDescriptor(descriptor, value) == BluetoothStatusCodes.SUCCESS
                    else { descriptor.value = value; active.writeDescriptor(descriptor) }
                }
            }
            if (!accepted) {
                if (operation.stop) finishStop() else if (operation.handshake) finishOperation(active) else scheduleReconnect("배터리 정보 요청에 실패해 다시 연결합니다.")
            }
        } catch (e: SecurityException) {
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
        }
    }

    private fun finishOperation(active: BluetoothGatt) {
        activeOperation = null
        runNextOperation(active)
    }

    private fun handleRead(active: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray?, status: Int) {
        if (stopping || active !== gatt) return
        val operation = activeOperation ?: return
        if (operation.subscribe || operation.characteristic !== characteristic) return
        if (status != BluetoothGatt.GATT_SUCCESS || value == null || !acceptValue(characteristic.uuid, value)) {
            scheduleReconnect("배터리를 읽지 못해 다시 연결합니다.")
            return
        }
        finishOperation(active)
    }

    private fun acceptValue(uuid: UUID, value: ByteArray): Boolean {
        when (uuid) {
            LEVEL -> {
                val level = value.firstOrNull()?.toInt()?.and(255) ?: return false
                if (level !in 0..100) return false
                lastLevel = level
            }
            STATUS -> {
                val status = BatteryStatusCodec.decode(value) ?: return false
                status.level?.let { lastLevel = it }
                charging = status.charging
                plugged = status.plugged
            }
            else -> return false
        }
        val level = lastLevel ?: return true
        val peer = device ?: return false
        try {
            BridgeStatus.recordReading(this, peer.address, peerName, level, charging, plugged)
            reconnectAttempts = 0
        } catch (e: SecurityException) {
            BridgeStatus.receive(ReceiveState.ERROR, "주변 기기 권한을 허용한 뒤 다시 연결해 주세요.")
            stopSelf()
            return false
        }
        return true
    }

    override fun onDestroy() {
        stopping = true
        unregisterReceiver(bluetoothReceiver)
        stopScan()
        handler.removeCallbacksAndMessages(null)
        try { gatt?.disconnect(); gatt?.close() }
        catch (e: SecurityException) { Log.e(TAG, "close failed", e) }
        BridgeStatus.receiveStopped()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val ACTION_STOP_PEER = "ae.heej.nearlink.STOP_PEER"
        private const val TAG = "BatteryBleReceiver"
        private val BAS = UUID.fromString("0000180f-0000-1000-8000-00805f9b34fb")
        private val STATUS = UUID.fromString("00002bed-0000-1000-8000-00805f9b34fb")
        private val CCC = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        private val LEVEL = UUID.fromString("00002a19-0000-1000-8000-00805f9b34fb")
    }
}
