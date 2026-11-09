package ae.heej.nearlink

import android.app.Application
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanFilter
import android.companion.AssociationInfo
import android.companion.AssociationRequest
import android.companion.BluetoothLeDeviceFilter
import android.companion.CompanionDeviceManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.IntentSender
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import java.util.UUID

/** Retained across activity recreation; never retains the activity or a chooser launcher. */
class PairingModel(application: Application) : AndroidViewModel(application) {
    private val context = application
    private val manager = application.getSystemService(CompanionDeviceManager::class.java)
    private val handler = Handler(Looper.getMainLooper())
    var busy by mutableStateOf(false)
        private set
    var message by mutableStateOf<String?>(null)
        private set
    var chooser by mutableStateOf<IntentSender?>(null)
    var readyAddress by mutableStateOf<String?>(null)
    private var pendingAddress: String? = null
    private var generation = 0
    private val timeout = Runnable { cancel("기기 추가 시간이 초과됐습니다. 상대 앱의 연결 준비를 확인하고 다시 시도해 주세요.") }

    private val bondReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            @Suppress("DEPRECATION")
            val device = intent.getParcelableExtra<BluetoothDevice>(BluetoothDevice.EXTRA_DEVICE) ?: return
            try {
                if (device.address != pendingAddress || !busy) return
                when (device.bondState) {
                    BluetoothDevice.BOND_BONDED -> complete(device.address)
                    BluetoothDevice.BOND_NONE -> cancel("페어링이 완료되지 않았습니다. 기기를 다시 추가해 주세요.")
                }
            } catch (_: SecurityException) { cancel("주변 기기 권한을 확인해 주세요.") }
        }
    }

    init {
        val filter = IntentFilter(BluetoothDevice.ACTION_BOND_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= 33) context.registerReceiver(bondReceiver, filter, Context.RECEIVER_EXPORTED)
        else context.registerReceiver(bondReceiver, filter)
    }

    fun start() {
        if (busy) return
        generation++
        val requestGeneration = generation
        busy = true
        message = "주변 기기를 찾습니다. 상대 기기에서도 앱을 열고 연결 준비를 눌러 주세요."
        handler.postDelayed(timeout, 120_000)
        val filter = BluetoothLeDeviceFilter.Builder().setScanFilter(
            ScanFilter.Builder().setServiceUuid(ParcelUuid(BRIDGE_UUID)).build()).build()
        val request = AssociationRequest.Builder().addDeviceFilter(filter).setSingleDevice(false).build()
        val cb = object : CompanionDeviceManager.Callback() {
            @Deprecated("Legacy callback")
            override fun onDeviceFound(chooserLauncher: IntentSender) {
                handler.post { if (busy && requestGeneration == generation) chooser = chooserLauncher }
            }

            override fun onAssociationPending(intentSender: IntentSender) {
                handler.post { if (busy && requestGeneration == generation) chooser = intentSender }
            }

            override fun onAssociationCreated(associationInfo: AssociationInfo) {
                if (Build.VERSION.SDK_INT >= 33) handler.post {
                    if (busy && requestGeneration == generation) associationInfo.deviceMacAddress?.toString()?.let(::bond)
                }
            }

            @Suppress("OVERRIDE_DEPRECATION")
            override fun onFailure(error: CharSequence?) {
                handler.post {
                    if (busy && requestGeneration == generation) cancel("기기를 추가하지 못했습니다. 상대 기기의 연결 준비와 블루투스·위치 서비스 상태를 확인해 주세요.")
                }
            }
        }
        try {
            if (Build.VERSION.SDK_INT >= 33) manager.associate(request, context.mainExecutor, cb)
            else {
                @Suppress("DEPRECATION")
                manager.associate(request, cb, handler)
            }
        } catch (_: Exception) { cancel("기기 검색을 시작하지 못했습니다. 블루투스와 주변 기기 권한을 확인해 주세요.") }
    }

    fun selected(data: Intent?) {
        if (!busy) return
        @Suppress("DEPRECATION")
        val result = data?.getParcelableExtra<android.os.Parcelable>(CompanionDeviceManager.EXTRA_DEVICE)
        val device = when (result) {
            is BluetoothDevice -> result
            is android.bluetooth.le.ScanResult -> result.device
            else -> null
        }
        try {
            if (device != null) bond(device.address)
            else if (Build.VERSION.SDK_INT >= 33) {
                data?.getParcelableExtra(CompanionDeviceManager.EXTRA_ASSOCIATION, AssociationInfo::class.java)
                    ?.deviceMacAddress?.toString()?.let(::bond)
            } else cancel("선택한 기기 정보를 받지 못했습니다. 다시 시도해 주세요.")
        } catch (_: SecurityException) { cancel("주변 기기 권한을 확인해 주세요.") }
    }

    private fun bond(rawAddress: String) {
        val address = rawAddress.uppercase(java.util.Locale.ROOT)
        if (!busy || pendingAddress == address) return
        try {
            val device = context.getSystemService(BluetoothManager::class.java).adapter.getRemoteDevice(address)
            pendingAddress = address
            message = "페어링 확인을 기다립니다. 양쪽 기기에서 요청을 확인해 주세요."
            when (device.bondState) {
                BluetoothDevice.BOND_BONDED -> complete(address)
                BluetoothDevice.BOND_NONE -> if (!device.createBond()) cancel("페어링을 시작하지 못했습니다. 다시 시도해 주세요.")
            }
        } catch (_: SecurityException) { cancel("주변 기기 권한을 확인해 주세요.")
        } catch (e: Exception) {
            android.util.Log.e("BridgePairing", "Could not start bonding", e)
            cancel("페어링할 수 없습니다. 블루투스와 주변 기기 권한을 확인해 주세요.")
        }
    }

    private fun complete(address: String) {
        handler.removeCallbacks(timeout)
        pendingAddress = null
        busy = false
        message = null
        readyAddress = address
    }

    fun cancel(reason: String? = null) {
        generation++
        handler.removeCallbacksAndMessages(null)
        busy = false
        chooser = null
        readyAddress = null
        pendingAddress = null
        message = reason
    }

    override fun onCleared() {
        cancel()
        context.unregisterReceiver(bondReceiver)
    }

    companion object {
        val BRIDGE_UUID: UUID = UUID.fromString("734b7300-53b1-4cf0-8d2e-d4b5c742b65b")
    }
}
