package ae.heej.nearlink

import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.core.net.toUri
import androidx.core.view.WindowCompat
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.IntentSenderRequest
import androidx.lifecycle.ViewModelProvider
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** A bonded device offered for an existing-pairing connection, with why it is listed where it is. */
private data class BondedChoice(val device: BluetoothDevice, val name: String, val note: String?, val rank: Int)

/** This is still a connection experiment; a requested start is not proof of a connection. */
class MainActivity : ComponentActivity() {
    private var message by mutableStateOf<String?>(null)
    private var messageAction by mutableStateOf<NoticeAction?>(null)
    private var peers by mutableStateOf<List<BondedChoice>?>(null)
    private var pendingAction: Action? = null
    private var pendingPeer: String? = null
    private var setupRequested by mutableStateOf(false)
    private var showBatteryRecommendation by mutableStateOf(false)
    private var batteryRestricted by mutableStateOf(false)

    private val pairing by lazy { ViewModelProvider(this)[PairingModel::class.java] }
    private enum class Action { SEND, CONNECT, ADD, RESUME }
    private val chooserResult = registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) pairing.selected(result.data)
        else pairing.cancel("기기 추가를 취소했습니다.")
    }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { _ ->
        val action = pendingAction
        pendingAction = null
        // Notifications are optional: a declined notification permission never blocks Bluetooth work.
        if (BridgeSession.hasPermissions(this) && action != null) perform(action)
        else show("주변 기기 권한이 필요합니다. 앱 권한 설정에서 허용한 후 다시 시도해 주세요.", NoticeAction.APP_SETTINGS)
    }

    /** Retries the action that found Bluetooth off once the user turns it on. */
    private val enableBluetooth = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        val action = pendingAction
        pendingAction = null
        if (bluetoothEnabled() && action != null) perform(action)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        BridgeStatus.initialize(this)
        PeerAlerts.load(this)
        pendingAction = savedInstanceState?.getString("pending_action")?.let { Action.valueOf(it) }
        pendingPeer = savedInstanceState?.getString("pending_peer")
        setupRequested = savedInstanceState?.getBoolean("setup_requested") ?: false
        showBatteryRecommendation = savedInstanceState?.getBoolean("battery_recommendation") ?: false
        if (savedInstanceState == null) handleLaunch(intent)
        setContent {
            val status by BridgeStatus.state.collectAsState()
            val alertSettings by PeerAlerts.settings.collectAsState()
            LaunchedEffect(status.preparationState) {
                if (status.preparationState == PreparationState.ACCEPTED) recommendBatteryExemption()
            }
            LaunchedEffect(pairing.chooser) {
                pairing.chooser?.let {
                    pairing.chooser = null
                    try { chooserResult.launch(IntentSenderRequest.Builder(it).build()) }
                    catch (_: Exception) { pairing.cancel("기기 선택 화면을 열지 못했습니다. 다시 시도해 주세요.") }
                }
            }
            LaunchedEffect(pairing.readyAddress) { startReadyPeer() }
            val dark = isSystemInDarkTheme()
            SideEffect {
                WindowCompat.getInsetsController(window, window.decorView).apply {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
            }
            val notice = message?.let { Notice(it, isError = true, action = messageAction, dismissible = true) }
                ?: pairing.message?.let { Notice(it, isError = !pairing.busy, action = null, dismissible = !pairing.busy) }
            BridgeTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    BridgeScreen(status, notice, pairing.busy, setupRequested, batteryRestricted, alertSettings, BridgeActions(
                        onSend = ::requestSend,
                        onConnect = ::requestConnect,
                        onAdd = { ensurePermissions(Action.ADD) },
                        onStopAll = ::stopBridge,
                        onReconnect = { address -> pendingPeer = address; ensurePermissions(Action.RESUME) },
                        onStopPeer = { address -> BridgeSession.requestStop(this, address) },
                        onRename = { address, alias -> BridgeStatus.rename(this, address, alias) },
                        onForget = { address -> BridgeStatus.forget(this, address) },
                        onSetupRequested = { setupRequested = true },
                        onDismissSetup = { setupRequested = false },
                        onDismissNotice = ::dismissNotice,
                        onNoticeAction = ::runNoticeAction,
                        onBatterySettings = ::requestBatteryExemption,
                        onAlertSettings = { PeerAlerts.save(this, it) },
                    ))
                }
                peers?.let { devices ->
                    AlertDialog(
                        onDismissRequest = { peers = null },
                        title = { Text("연결할 기기 선택") },
                        text = {
                            Column(Modifier.heightIn(max = 400.dp).verticalScroll(rememberScrollState())) {
                                devices.forEach { choice ->
                                    TextButton(onClick = { peers = null; connect(choice.device) }, modifier = Modifier.fillMaxWidth()) {
                                        Column(Modifier.fillMaxWidth()) {
                                            Text(choice.name)
                                            choice.note?.let { Text(it, style = MaterialTheme.typography.bodySmall,
                                                color = MaterialTheme.colorScheme.onSurfaceVariant) }
                                        }
                                    }
                                }
                            }
                        },
                        confirmButton = { TextButton(onClick = { peers = null }) { Text("닫기") } }
                    )
                }
                if (showBatteryRecommendation) {
                    AlertDialog(
                        onDismissRequest = { showBatteryRecommendation = false },
                        title = { Text("배터리 사용 제한 해제") },
                        text = { Text("화면이 꺼져 있어도 연결을 안정적으로 유지하려면 배터리 최적화 제외를 허용해 주세요. 배터리 사용량이 늘어날 수 있습니다. 허용하지 않아도 앱을 사용할 수 있어요.") },
                        confirmButton = { TextButton(onClick = { requestBatteryExemption() }) { Text("설정하기") } },
                        dismissButton = { TextButton(onClick = { showBatteryRecommendation = false }) { Text("나중에") } }
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLaunch(intent)
    }

    /** The Quick Settings tile opens the app with this action so permissions and errors stay visible. */
    private fun handleLaunch(intent: Intent?) {
        if (intent?.action == ACTION_PREPARE_FROM_TILE) {
            intent.action = null
            requestSend()
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        pendingAction?.let { outState.putString("pending_action", it.name) }
        pendingPeer?.let { outState.putString("pending_peer", it) }
        outState.putBoolean("setup_requested", setupRequested)
        outState.putBoolean("battery_recommendation", showBatteryRecommendation)
        super.onSaveInstanceState(outState)
    }

    override fun onResume() {
        super.onResume()
        batteryRestricted = !getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName)
        startReadyPeer()
        if (!pairing.busy && pairing.readyAddress == null) {
            val saved = BridgeSession.savedPeers(this)
            if (saved.isNotEmpty() && !BridgeSession.hasPermissions(this)) {
                show("자동 연결을 재개하려면 주변 기기 권한을 허용해 주세요.", NoticeAction.APP_SETTINGS)
            } else saved.forEach { address ->
                try { BridgeSession.start(this, address) }
                catch (_: Exception) { show("자동 연결을 시작하지 못했습니다. 기기를 다시 선택해 주세요.") }
            }
        }
    }

    private fun show(text: String, action: NoticeAction? = null) {
        message = text
        messageAction = action
    }

    private fun clearMessage() {
        message = null
        messageAction = null
    }

    private fun dismissNotice() {
        if (message != null) clearMessage() else if (!pairing.busy) pairing.clearMessage()
    }

    private fun runNoticeAction(action: NoticeAction) {
        clearMessage()
        when (action) {
            NoticeAction.APP_SETTINGS -> try {
                startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, "package:$packageName".toUri()))
            } catch (_: Exception) { show("앱 설정을 열지 못했습니다. 기기 설정에서 NearLink 권한을 확인해 주세요.") }
            NoticeAction.ENABLE_BLUETOOTH -> try {
                enableBluetooth.launch(Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE))
            } catch (_: Exception) { show("블루투스 설정을 열지 못했습니다. 빠른 설정에서 블루투스를 켜 주세요.") }
        }
    }

    private fun startReadyPeer() {
        val address = pairing.readyAddress ?: return
        if (!lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) return
        try {
            BridgeSession.start(this, address)
            setupRequested = false
            pairing.readyAddress = null
            clearMessage()
            recommendBatteryExemption()
        } catch (_: Exception) { show("페어링됐지만 송수신을 시작하지 못했습니다. 기존 기기 선택을 눌러 다시 연결해 주세요.") }
    }

    private fun requestSend() = ensurePermissions(Action.SEND)
    private fun requestConnect() = ensurePermissions(Action.CONNECT)

    private fun ensurePermissions(action: Action) {
        val required = if (Build.VERSION.SDK_INT >= 31) {
            arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_ADVERTISE)
        } else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        // Android 13+ hides the connection-service notification until this is granted.
        val optional = if (Build.VERSION.SDK_INT >= 33) arrayOf(Manifest.permission.POST_NOTIFICATIONS) else emptyArray()
        val missing = (required + optional).filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isEmpty()) perform(action) else {
            pendingAction = action
            permissions.launch(missing.toTypedArray())
        }
    }

    private fun bluetoothEnabled(): Boolean = getSystemService(BluetoothManager::class.java)?.adapter?.isEnabled == true

    private fun perform(action: Action) {
        try {
            val adapter = getSystemService(BluetoothManager::class.java)?.adapter
            if (adapter == null) {
                show("이 기기에서 블루투스를 사용할 수 없습니다.")
                return
            }
            if (!adapter.isEnabled) {
                pendingAction = action
                show("블루투스가 꺼져 있습니다. 켜면 이어서 진행합니다.", NoticeAction.ENABLE_BLUETOOTH)
                return
            }
            when (action) {
                Action.ADD -> {
                    if (!packageManager.hasSystemFeature(PackageManager.FEATURE_COMPANION_DEVICE_SETUP)) {
                        show("이 기기는 앱 내 기기 추가를 지원하지 않습니다. 블루투스 설정에서 페어링한 뒤 기존 기기를 선택해 주세요.")
                        return
                    }
                    clearMessage()
                    startForegroundService(Intent(this, BatteryService::class.java))
                    pairing.start()
                }
                Action.SEND -> {
                    startForegroundService(Intent(this, BatteryService::class.java).setAction(BatteryService.ACTION_PREPARE))
                    clearMessage()
                }
                Action.CONNECT -> {
                    val choices = bondedChoices(adapter.bondedDevices)
                    if (choices.isEmpty()) show("페어링된 기기가 없습니다. 기기 추가를 눌러 연결해 주세요.")
                    else { clearMessage(); peers = choices }
                }
                Action.RESUME -> {
                    val snapshot = BridgeStatus.state.value
                    val address = pendingPeer ?: snapshot.peerId ?: snapshot.visibleReading()?.peerId
                    pendingPeer = null
                    if (address == null) show("기기를 다시 선택해 주세요.")
                    else {
                        BridgeSession.start(this, address)
                        setupRequested = false
                        clearMessage()
                        recommendBatteryExemption()
                    }
                }
            }
        } catch (_: SecurityException) {
            show("블루투스 권한을 확인한 후 다시 시도해 주세요.", NoticeAction.APP_SETTINGS)
        } catch (_: IllegalStateException) {
            show("서비스를 시작하지 못했습니다. 앱을 다시 열고 시도해 주세요.")
        }
    }

    /** NearLink devices first, then phones and tablets, then everything else (earbuds, cars, …). */
    private fun bondedChoices(bonded: Set<BluetoothDevice>): List<BondedChoice> {
        val known = BridgeStatus.state.value.peers
        return bonded.map { device ->
            val peer = known.entries.firstOrNull { it.key.equals(device.address, ignoreCase = true) }?.value
            val major = try { device.bluetoothClass?.majorDeviceClass } catch (_: SecurityException) { null }
            val (rank, note) = when {
                peer != null -> 0 to when (peer.receiveState) {
                    ReceiveState.STOPPED -> "NearLink · 연결 중지됨"
                    ReceiveState.CONNECTED -> "NearLink · 연결됨"
                    else -> "NearLink · 등록된 기기"
                }
                major == BluetoothClass.Device.Major.PHONE -> 1 to "휴대폰"
                major == BluetoothClass.Device.Major.COMPUTER -> 1 to "태블릿·컴퓨터"
                else -> 2 to null
            }
            BondedChoice(device, peer?.displayName ?: deviceName(device), note, rank)
        }.sortedWith(compareBy({ it.rank }, { it.name }))
    }

    private fun deviceName(device: BluetoothDevice): String = try {
        (if (Build.VERSION.SDK_INT >= 30) device.alias else device.name) ?: "이름 없는 기기"
    } catch (_: SecurityException) { "이름 없는 기기" }

    private fun connect(device: BluetoothDevice) {
        try {
            pairing.cancel()
            BridgeSession.start(this, device.address)
            setupRequested = false
            clearMessage()
            recommendBatteryExemption()
        } catch (_: SecurityException) {
            show("블루투스 권한을 확인한 후 다시 시도해 주세요.", NoticeAction.APP_SETTINGS)
        } catch (_: IllegalStateException) {
            show("연결 서비스를 시작하지 못했습니다. 앱을 다시 열고 시도해 주세요.")
        }
    }

    private fun stopBridge() {
        pendingAction = null
        peers = null
        pairing.cancel()
        BridgeSession.requestStop(this)
        clearMessage()
    }

    private fun recommendBatteryExemption() {
        val preferences = getSharedPreferences("battery_recommendation", MODE_PRIVATE)
        if (preferences.getBoolean("shown", false) ||
            getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName)) return
        preferences.edit().putBoolean("shown", true).apply()
        showBatteryRecommendation = true
    }

    private fun requestBatteryExemption() {
        showBatteryRecommendation = false
        try {
            startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName")))
        } catch (_: Exception) {
            show("배터리 설정을 열지 못했어요. 기기의 앱 설정에서 배터리 사용 제한을 변경할 수 있습니다.", NoticeAction.APP_SETTINGS)
        }
    }

    companion object {
        const val ACTION_PREPARE_FROM_TILE = "ae.heej.nearlink.PREPARE_FROM_TILE"
    }
}
