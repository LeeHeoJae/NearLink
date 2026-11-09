package ae.heej.nearlink

import android.Manifest
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

/** This is still a connection experiment; a requested start is not proof of a connection. */
class MainActivity : ComponentActivity() {
    private var message by mutableStateOf<String?>(null)
    private var peers by mutableStateOf<List<BluetoothDevice>?>(null)
    private var pendingAction: Action? = null
    private var setupRequested by mutableStateOf(false)
    private var preparationNotice by mutableStateOf<String?>(null)
    private var showBatteryRecommendation by mutableStateOf(false)

    private val pairing by lazy { ViewModelProvider(this)[PairingModel::class.java] }
    private enum class Action { SEND, CONNECT, ADD, RESUME }
    private val chooserResult = registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) pairing.selected(result.data)
        else pairing.cancel("기기 추가를 취소했습니다.")
    }

    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        val action = pendingAction
        pendingAction = null
        if (result.values.all { it } && action != null) perform(action)
        else message = "주변 기기 권한이 필요합니다. 앱 권한 설정에서 허용한 후 다시 시도해 주세요."
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        BridgeStatus.initialize(this)
        pendingAction = savedInstanceState?.getString("pending_action")?.let { Action.valueOf(it) }
        setupRequested = savedInstanceState?.getBoolean("setup_requested") ?: false
        showBatteryRecommendation = savedInstanceState?.getBoolean("battery_recommendation") ?: false
        setContent {
            val status by BridgeStatus.state.collectAsState()
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
            BridgeTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    BridgeScreen(status, message ?: pairing.message ?: preparationNotice.takeIf { !status.hasCompletedSetup() || setupRequested },
                        message != null || (pairing.message != null && !pairing.busy), pairing.busy, ::requestSend, ::requestConnect,
                        { ensurePermissions(Action.ADD) }, ::stopBridge,
                        onReconnect = { ensurePermissions(Action.RESUME) }, setupRequested = setupRequested,
                        onSetupRequested = { setupRequested = true }, onDismissSetup = { setupRequested = false })
                }
                peers?.let { devices ->
                    AlertDialog(
                        onDismissRequest = { peers = null },
                        title = { Text("연결할 기기 선택") },
                        text = {
                            Column(Modifier.heightIn(max = 400.dp).verticalScroll(rememberScrollState())) {
                                devices.forEach { device ->
                                    TextButton(onClick = { peers = null; connect(device) }, modifier = Modifier.fillMaxWidth()) {
                                        Text(deviceName(device), modifier = Modifier.fillMaxWidth())
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

    override fun onSaveInstanceState(outState: Bundle) {
        pendingAction?.let { outState.putString("pending_action", it.name) }
        outState.putBoolean("setup_requested", setupRequested)
        outState.putBoolean("battery_recommendation", showBatteryRecommendation)
        super.onSaveInstanceState(outState)
    }

    override fun onResume() {
        super.onResume()
        startReadyPeer()
        if (!pairing.busy && pairing.readyAddress == null) {
            BridgeSession.savedPeer(this)?.let { address ->
                if (BridgeSession.hasPermissions(this)) {
                    try { BridgeSession.start(this, address) }
                    catch (_: Exception) { message = "자동 연결을 시작하지 못했습니다. 기기를 다시 선택해 주세요." }
                } else message = "자동 연결을 재개하려면 주변 기기 권한을 허용하고 기기를 다시 선택해 주세요."
            }
        }
    }

    private fun startReadyPeer() {
        val address = pairing.readyAddress ?: return
        if (!lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) return
        try {
            BridgeSession.start(this, address)
            setupRequested = false
            pairing.readyAddress = null
            message = null
            recommendBatteryExemption()
        } catch (_: Exception) { message = "페어링됐지만 송수신을 시작하지 못했습니다. 기존 기기 선택을 눌러 다시 연결해 주세요." }
    }

    private fun requestSend() = ensurePermissions(Action.SEND)
    private fun requestConnect() = ensurePermissions(Action.CONNECT)

    private fun ensurePermissions(action: Action) {
        preparationNotice = null
        val required = if (Build.VERSION.SDK_INT >= 31) {
            arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_ADVERTISE)
        } else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        val missing = required.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isEmpty()) perform(action) else {
            pendingAction = action
            permissions.launch(missing.toTypedArray())
        }
    }

    private fun perform(action: Action) {
        try {
            val adapter = getSystemService(BluetoothManager::class.java)?.adapter
            if (adapter == null || !adapter.isEnabled) {
                message = "블루투스를 켠 다음 다시 시도해 주세요."
                return
            }
            when (action) {
                Action.ADD -> {
                    if (!packageManager.hasSystemFeature(PackageManager.FEATURE_COMPANION_DEVICE_SETUP)) {
                        message = "이 기기는 앱 내 기기 추가를 지원하지 않습니다. 블루투스 설정에서 페어링한 뒤 기존 기기를 선택해 주세요."
                        return
                    }
                    message = null
                    startForegroundService(Intent(this, BatteryService::class.java))
                    pairing.start()
                }
                Action.SEND -> {
                    startForegroundService(Intent(this, BatteryService::class.java).setAction(BatteryService.ACTION_PREPARE))
                    message = null
                    preparationNotice = "연결 준비를 요청했어요. 상대 기기에서 2분 안에 이 기기를 추가해 주세요."
                    recommendBatteryExemption()
                }
                Action.CONNECT -> {
                    val bonded = adapter.bondedDevices.sortedBy { deviceName(it) }
                    if (bonded.isEmpty()) message = "페어링된 기기가 없습니다. 기기 추가를 눌러 연결해 주세요."
                    else { message = null; peers = bonded }
                }
                Action.RESUME -> {
                    val snapshot = BridgeStatus.state.value
                    val address = snapshot.peerId ?: snapshot.visibleReading()?.peerId
                    if (address == null) message = "기기를 다시 선택해 주세요."
                    else {
                        BridgeSession.start(this, address)
                        setupRequested = false
                        message = null
                        recommendBatteryExemption()
                    }
                }
            }
        } catch (_: SecurityException) {
            message = "블루투스 권한을 확인한 후 다시 시도해 주세요."
        } catch (_: IllegalStateException) {
            message = "서비스를 시작하지 못했습니다. 앱을 다시 열고 시도해 주세요."
        }
    }

    private fun deviceName(device: BluetoothDevice): String = try {
        (if (Build.VERSION.SDK_INT >= 30) device.alias else device.name) ?: "이름 없는 기기"
    } catch (_: SecurityException) { "이름 없는 기기" }

    private fun connect(device: BluetoothDevice) {
        try {
            pairing.cancel()
            BridgeSession.start(this, device.address)
            setupRequested = false
            message = null
            recommendBatteryExemption()
        } catch (_: SecurityException) {
            message = "블루투스 권한을 확인한 후 다시 시도해 주세요."
        } catch (_: IllegalStateException) {
            message = "연결 서비스를 시작하지 못했습니다. 앱을 다시 열고 시도해 주세요."
        }
    }

    private fun stopBridge() {
        preparationNotice = null
        pendingAction = null
        peers = null
        pairing.cancel()
        BridgeSession.requestStop(this)
        message = null
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
            message = "배터리 설정을 열지 못했어요. 기기의 앱 설정에서 배터리 사용 제한을 변경할 수 있습니다."
        }
    }
}
