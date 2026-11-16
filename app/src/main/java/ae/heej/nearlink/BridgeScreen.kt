package ae.heej.nearlink

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

/** A one-tap fix offered next to a message. */
enum class NoticeAction(val label: String) { APP_SETTINGS("앱 설정 열기"), ENABLE_BLUETOOTH("블루투스 켜기") }

internal data class Notice(val text: String, val isError: Boolean, val action: NoticeAction?, val dismissible: Boolean)

internal class BridgeActions(
    val onSend: () -> Unit,
    val onConnect: () -> Unit,
    val onAdd: () -> Unit,
    val onStopAll: () -> Unit,
    val onReconnect: (String) -> Unit,
    val onStopPeer: (String) -> Unit,
    val onRename: (String, String) -> Unit,
    val onForget: (String) -> Unit,
    val onSetupRequested: () -> Unit,
    val onDismissSetup: () -> Unit,
    val onDismissNotice: () -> Unit,
    val onNoticeAction: (NoticeAction) -> Unit,
    val onBatterySettings: () -> Unit,
    val onAlertSettings: (PeerAlertSettings) -> Unit,
)

@Composable
internal fun BridgeScreen(status: BridgeSnapshot, notice: Notice?, pairing: Boolean, setupRequested: Boolean,
    batteryRestricted: Boolean, alertSettings: PeerAlertSettings, actions: BridgeActions) {
    val managed = status.hasCompletedSetup() && !setupRequested
    var showAddOptions by rememberSaveable { mutableStateOf(false) }
    var confirmStopAll by rememberSaveable { mutableStateOf(false) }
    // Reception ages are relative, so the clock ticks while the screen is visible.
    val now by produceState(System.currentTimeMillis()) {
        while (true) { delay(30_000); value = System.currentTimeMillis() }
    }
    BackHandler(enabled = setupRequested && status.hasCompletedSetup(), onBack = actions.onDismissSetup)
    BoxWithConstraints(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
        val expanded = maxWidth >= BridgeDesign.expandedWidth
        Column(Modifier.widthIn(max = BridgeDesign.maxWidth).fillMaxSize()) {
            // Each mode has its own scroll position. A successful first read starts management at the top.
            key(managed) {
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState())
                    .padding(horizontal = BridgeDesign.pageInset)) {
                    Column(Modifier.fillMaxWidth().padding(top = if (expanded) 28.dp else 40.dp, bottom = 28.dp)) {
                        Text("NearLink", style = MaterialTheme.typography.headlineLarge,
                            modifier = Modifier.semantics { heading() })
                        Spacer(Modifier.height(8.dp))
                        SecondaryText(if (managed) "연결한 기기의 배터리를 확인하세요." else "기기의 배터리를 삼성 위젯으로 연결하세요.")
                    }
                    notice?.let { NoticeCard(it, actions) }
                    if (managed) {
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            FilledTonalButton(onClick = { showAddOptions = true }, enabled = !pairing,
                                modifier = Modifier.weight(1f).heightIn(min = 48.dp)) { Text("기기 추가") }
                            OutlinedButton(onClick = actions.onSend, enabled = !pairing && status.sendState != SendState.STARTING &&
                                status.sendState != SendState.WAITING_FOR_BLUETOOTH,
                                modifier = Modifier.weight(1f).heightIn(min = 48.dp)) { Text(preparationButtonText(status)) }
                        }
                        if (status.preparationState != PreparationState.IDLE || status.sendState == SendState.ERROR ||
                            status.sendState == SendState.STARTING || status.sendState == SendState.WAITING_FOR_BLUETOOTH) {
                            Column(Modifier.fillMaxWidth().padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                PreparationFeedback(status)
                            }
                        }
                        if (batteryRestricted && status.peers.values.any {
                                it.receiveState == ReceiveState.RETRYING || it.receiveState == ReceiveState.ERROR }) {
                            Spacer(Modifier.height(BridgeDesign.gap))
                            BatteryHint(actions.onBatterySettings)
                        }
                        Spacer(Modifier.height(BridgeDesign.gap))
                        SectionTitle("연결한 기기")
                        PeerList(status, now, actions)
                        Group {
                            TextButton(onClick = { confirmStopAll = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp),
                                colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("모든 연결 중지") }
                        }
                        Spacer(Modifier.height(BridgeDesign.gap))
                        AlertSettingsGroup(alertSettings, actions.onAlertSettings)
                        Spacer(Modifier.height(BridgeDesign.gap))
                        HistoryGroup(status)
                    } else {
                        if (setupRequested && status.hasCompletedSetup()) {
                            TextButton(onClick = actions.onDismissSetup, modifier = Modifier.heightIn(min = 48.dp)) { Text("배터리 화면으로 돌아가기") }
                        }
                        if (expanded) {
                            Row(horizontalArrangement = Arrangement.spacedBy(BridgeDesign.gap)) {
                                Column(Modifier.weight(1.1f)) { SetupGuide() }
                                Column(Modifier.weight(1f)) { PreparationGroup(status, pairing, actions.onSend, actions.onStopAll) }
                            }
                        } else {
                            SetupGuide()
                            Spacer(Modifier.height(BridgeDesign.gap))
                            PreparationGroup(status, pairing, actions.onSend, actions.onStopAll)
                        }
                        if (status.peers.isNotEmpty()) {
                            Spacer(Modifier.height(BridgeDesign.gap))
                            SectionTitle("기기 연결 상태")
                            PeerList(status, now, actions)
                        }
                    }
                    Spacer(Modifier.height(24.dp))
                }
            }
            if (!managed) {
                Surface(color = MaterialTheme.colorScheme.background) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = BridgeDesign.pageInset, vertical = 12.dp),
                        horizontalArrangement = if (expanded) Arrangement.End else Arrangement.Center) {
                        Button(onClick = { showAddOptions = true }, enabled = !pairing, shape = BridgeDesign.controlShape,
                            modifier = (if (expanded) Modifier.widthIn(min = 240.dp, max = 360.dp) else Modifier.fillMaxWidth()).heightIn(min = 52.dp),
                            contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp)) {
                            Text(if (pairing) "기기 추가 중…" else "기기 추가")
                        }
                    }
                }
            }
        }
    }
    if (showAddOptions) {
        AlertDialog(onDismissRequest = { showAddOptions = false }, title = { Text("기기 추가") },
            text = {
                Column {
                    SecondaryText("상대 기기에서 연결 준비를 먼저 눌러 주세요.")
                    TextButton(onClick = { showAddOptions = false; actions.onAdd() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("새 기기 찾기") }
                    TextButton(onClick = { showAddOptions = false; actions.onConnect() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("기존 페어링 기기 연결") }
                }
            }, confirmButton = { TextButton(onClick = { showAddOptions = false }) { Text("닫기") } })
    }
    if (confirmStopAll) {
        AlertDialog(onDismissRequest = { confirmStopAll = false }, title = { Text("모든 연결을 중지할까요?") },
            text = { Text("모든 기기와 배터리를 주고받지 않습니다. 기기 목록과 페어링은 유지되며 각 기기에서 다시 연결할 수 있어요.") },
            confirmButton = {
                TextButton(onClick = { confirmStopAll = false; actions.onStopAll() },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("모두 중지") }
            },
            dismissButton = { TextButton(onClick = { confirmStopAll = false }) { Text("취소") } })
    }
}

@Composable
private fun NoticeCard(notice: Notice, actions: BridgeActions) {
    Surface(shape = BridgeDesign.groupShape,
        color = if (notice.isError) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.primaryContainer,
        contentColor = if (notice.isError) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onPrimaryContainer,
        modifier = Modifier.fillMaxWidth().padding(bottom = BridgeDesign.gap)) {
        Column(Modifier.padding(start = 20.dp, end = 12.dp, top = 20.dp, bottom = if (notice.action != null || notice.dismissible) 8.dp else 20.dp)) {
            Text(notice.text, style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(end = 8.dp).semantics { liveRegion = LiveRegionMode.Polite })
            if (notice.action != null || notice.dismissible) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    if (notice.dismissible) TextButton(onClick = actions.onDismissNotice,
                        colors = ButtonDefaults.textButtonColors(contentColor = LocalContentColor.current),
                        modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = "메시지 닫기" }) { Text("닫기") }
                    notice.action?.let { action ->
                        TextButton(onClick = { actions.onNoticeAction(action) },
                            colors = ButtonDefaults.textButtonColors(contentColor = LocalContentColor.current),
                            modifier = Modifier.heightIn(min = 48.dp)) { Text(action.label, fontWeight = FontWeight.SemiBold) }
                    }
                }
            }
        }
    }
}

@Composable
private fun BatteryHint(onOpen: () -> Unit) {
    Group {
        Column(Modifier.padding(start = BridgeDesign.groupInset, end = 12.dp, top = 20.dp, bottom = 8.dp)) {
            Text("연결이 자주 끊기나요?", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            SecondaryText("배터리 사용 제한 때문에 화면이 꺼지면 연결이 끊길 수 있어요. 제한을 해제하면 배터리 사용량이 늘어날 수 있습니다.")
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onOpen, modifier = Modifier.heightIn(min = 48.dp)) { Text("배터리 설정 열기") }
            }
        }
    }
}

@Composable
private fun SetupGuide() {
    var showNotes by rememberSaveable { mutableStateOf(false) }
    SectionTitle("연결 방법")
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("한 기기에서 준비하고,\n다른 기기에서 추가하세요", style = MaterialTheme.typography.titleLarge)
            SecondaryText("한 기기에서 ‘연결 준비’를 누르고, 2분 안에 다른 기기의 ‘기기 추가’에서 선택하세요.")
            SecondaryText("페어링이 끝나면 양쪽에서 배터리를 주고받아요.")
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant, modifier = Modifier.padding(horizontal = 24.dp))
        Disclosure("알아둘 점", showNotes, { showNotes = !showNotes })
        if (showNotes) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp)) {
            SecondaryText("앱 연결 후 삼성 위젯에서도 표시를 확인해 주세요. 선택한 페어링 기기에만 배터리 정보를 제공하도록 변경했으며 위젯 호환성은 확인 중이에요.")
        }
    }
}

@Composable
private fun PreparationGroup(status: BridgeSnapshot, pairing: Boolean, onSend: () -> Unit, onStop: () -> Unit) {
    SectionTitle("연결 준비")
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            PreparationFeedback(status)
            FilledTonalButton(onClick = onSend,
                enabled = !pairing && status.sendState != SendState.STARTING && status.sendState != SendState.WAITING_FOR_BLUETOOTH,
                shape = BridgeDesign.controlShape, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), contentPadding = PaddingValues(12.dp)) {
                Text(preparationButtonText(status))
            }
            if (pairing || (!status.hasCompletedSetup() && status.receiveState != ReceiveState.STOPPED)) {
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                StatusText(if (pairing) "기기 선택과 페어링을 기다리고 있어요" else receiveText(status.receiveState),
                    status.receiveState == ReceiveState.ERROR)
                status.receiveDetail?.let { SecondaryText(it) }
            }
            // Preparation and a first connection can run services before the first read: keep cancellation reachable.
            if (pairing || status.sendState != SendState.STOPPED || status.receiveState != ReceiveState.STOPPED) {
                TextButton(onClick = onStop, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text("연결 시도 중지") }
            }
        }
    }
}

private fun preparationButtonText(status: BridgeSnapshot): String = when {
    status.sendState == SendState.ERROR -> "다시 준비"
    status.sendState == SendState.STARTING -> "준비 중…"
    status.preparationState == PreparationState.OPEN -> "준비 시간 연장"
    status.preparationState == PreparationState.EXPIRED -> "다시 준비"
    else -> "연결 준비"
}

@Composable
private fun PreparationFeedback(status: BridgeSnapshot) {
    var now by remember(status.preparationUntil) { mutableLongStateOf(android.os.SystemClock.elapsedRealtime()) }
    LaunchedEffect(status.preparationState, status.preparationUntil) {
        while (status.preparationState == PreparationState.OPEN) {
            now = android.os.SystemClock.elapsedRealtime()
            if (now >= status.preparationUntil) break
            delay(250)
        }
    }
    val seconds = status.preparationSecondsRemaining(now)
    val title = when {
        status.sendState == SendState.ERROR -> "연결 준비에 실패했어요"
        status.preparationState == PreparationState.EXPIRED -> "연결 준비 시간이 끝났어요"
        status.sendState == SendState.WAITING_FOR_BLUETOOTH -> "블루투스가 꺼져 있어요"
        status.sendState == SendState.STARTING -> "연결 준비 중…"
        status.preparationState == PreparationState.ACCEPTED -> "기기 등록 요청을 수락했어요"
        status.preparationState == PreparationState.OPEN && seconds == 0L -> "연결 준비 시간이 끝났어요"
        status.preparationState == PreparationState.OPEN && status.sendState == SendState.ADVERTISING -> "이제 다른 기기에서 추가할 수 있어요"
        else -> "새 기기를 추가하려면 연결 준비를 눌러 주세요"
    }
    StatusText(title, status.sendState == SendState.ERROR)
    if (status.preparationState == PreparationState.OPEN && seconds > 0L && status.sendState == SendState.ADVERTISING) {
        // Keep the ticking value outside the live region so TalkBack announces the state, not every second.
        Text("남은 시간 ${seconds / 60}분 ${seconds % 60}초", style = MaterialTheme.typography.titleMedium)
        SecondaryText("Windows 또는 상대 기기의 NearLink에서 ‘기기 추가’를 누르고 이 기기를 선택하세요.")
    } else if (status.sendState != SendState.ERROR && (status.preparationState == PreparationState.EXPIRED ||
        (status.preparationState == PreparationState.OPEN && seconds == 0L))) {
        SecondaryText("새 기기를 추가하려면 ‘다시 준비’를 누르세요. 기존 기기의 등록은 유지돼요.")
    } else if (status.sendState == SendState.WAITING_FOR_BLUETOOTH) {
        SecondaryText("블루투스를 켜 주세요. 준비 시간이 지나면 다시 준비해 주세요.")
    } else if (status.preparationState == PreparationState.ACCEPTED && status.sendState != SendState.ERROR) {
        SecondaryText("상대 기기에서 연결이 완료되는지 확인해 주세요.")
    }
    status.sendDetail?.let { SecondaryText(it) }
}

@Composable
private fun PeerList(status: BridgeSnapshot, now: Long, actions: BridgeActions) {
    status.peers.values.sortedBy { it.displayName }.forEach { peer ->
        key(peer.peerId) { PeerItem(peer, now, actions) }
        Spacer(Modifier.height(BridgeDesign.gap))
    }
}

@Composable
private fun PeerItem(peer: PeerSnapshot, now: Long, actions: BridgeActions) {
    var menu by remember { mutableStateOf(false) }
    var renaming by rememberSaveable { mutableStateOf(false) }
    var forgetting by rememberSaveable { mutableStateOf(false) }
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(peer.displayName, style = MaterialTheme.typography.titleLarge)
            if (peer.alias != null) SecondaryText("블루투스 이름 · ${peer.peerName}")
            StatusText(receiveText(peer.receiveState), peer.receiveState == ReceiveState.ERROR)
            if (peer.sendOnly) SecondaryText("이 기기로 배터리를 보내요. PC 배터리는 받지 않습니다.")
            else peer.lastReading?.let { reading ->
                Text("${reading.level}%", style = MaterialTheme.typography.displayMedium)
                Text(when {
                    reading.charging == true -> "충전 중"
                    reading.charging == false && reading.plugged == true && reading.level == 100 -> "전원 연결 · 완충"
                    reading.charging == false && reading.plugged == true -> "전원 연결 · 충전 중 아님"
                    reading.charging == false -> "충전 중 아님"
                    else -> "충전 상태 알 수 없음"
                }, style = MaterialTheme.typography.bodyLarge)
                SecondaryText("마지막 수신 · ${relativeAge(now, reading.timeMillis)} (" +
                    DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(reading.timeMillis)) + ")")
                if (peer.receiveState != ReceiveState.CONNECTED) SecondaryText("마지막으로 받은 잔량이며 현재 상태와 다를 수 있어요.")
            } ?: SecondaryText("아직 배터리 정보를 받지 못했어요.")
            peer.detail?.let { SecondaryText(it) }
            if (peer.receiveState == ReceiveState.STOPPED || peer.receiveState == ReceiveState.ERROR) {
                FilledTonalButton(onClick = { actions.onReconnect(peer.peerId) }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text("다시 연결") }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (peer.receiveState != ReceiveState.STOPPED) {
                    TextButton(onClick = { actions.onStopPeer(peer.peerId) }, modifier = Modifier.weight(1f).heightIn(min = 48.dp),
                        colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("연결 중지") }
                }
                Box(Modifier.weight(1f)) {
                    TextButton(onClick = { menu = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text("기기 설정") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text("별명 변경") }, onClick = { menu = false; renaming = true })
                        DropdownMenuItem(
                            text = { Text(if (peer.receiveState == ReceiveState.STOPPED) "목록에서 제거" else "목록에서 제거 (먼저 연결 중지)") },
                            enabled = peer.receiveState == ReceiveState.STOPPED,
                            onClick = { menu = false; forgetting = true })
                    }
                }
            }
        }
    }
    if (renaming) RenameDialog(peer, onDismiss = { renaming = false }, onSave = { actions.onRename(peer.peerId, it); renaming = false })
    if (forgetting) {
        AlertDialog(onDismissRequest = { forgetting = false }, title = { Text("목록에서 제거할까요?") },
            text = { Text("${peer.displayName}의 저장된 배터리 기록과 별명을 삭제합니다. 블루투스 페어링은 유지되며 ‘기존 페어링 기기 연결’로 다시 추가할 수 있어요.") },
            confirmButton = {
                TextButton(onClick = { forgetting = false; actions.onForget(peer.peerId) },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("제거") }
            },
            dismissButton = { TextButton(onClick = { forgetting = false }) { Text("취소") } })
    }
}

@Composable
private fun RenameDialog(peer: PeerSnapshot, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf(peer.alias ?: "") }
    AlertDialog(onDismissRequest = onDismiss, title = { Text("별명 변경") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = text, onValueChange = { text = it.take(48) }, singleLine = true,
                    label = { Text("별명") }, placeholder = { Text(peer.peerName) },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done), modifier = Modifier.fillMaxWidth())
                SecondaryText("이 앱에서만 표시됩니다. 비워 두면 블루투스 이름을 사용하며, 삼성 위젯의 이름은 바뀌지 않아요.")
            }
        },
        confirmButton = { TextButton(onClick = { onSave(text) }) { Text("저장") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("취소") } })
}

@Composable
private fun AlertSettingsGroup(settings: PeerAlertSettings, onChange: (PeerAlertSettings) -> Unit) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    var threshold by remember(settings.threshold) { mutableFloatStateOf(settings.threshold.toFloat()) }
    Group {
        Disclosure("배터리 알림", expanded, { expanded = !expanded })
        if (expanded) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SettingSwitch("배터리 부족 알림", "충전 중이 아닐 때 한 번 알려요.", settings.low) { onChange(settings.copy(low = it)) }
            if (settings.low) {
                Text("${threshold.roundToInt()}% 이하", style = MaterialTheme.typography.bodyLarge)
                Slider(value = threshold, onValueChange = { threshold = it }, valueRange = 5f..50f, steps = 8,
                    onValueChangeFinished = { onChange(settings.copy(threshold = threshold.roundToInt())) },
                    modifier = Modifier.semantics { contentDescription = "배터리 부족 알림 기준" })
            }
            SettingSwitch("충전 완료 알림", "100%가 되면 알려요.", settings.full) { onChange(settings.copy(full = it)) }
            SecondaryText("연결된 기기에서 새 배터리 정보를 받을 때만 알려요. 알림 권한이 꺼져 있으면 표시되지 않습니다.")
        }
    }
}

@Composable
private fun SettingSwitch(title: String, description: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            SecondaryText(description)
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

private fun receiveText(state: ReceiveState): String = when (state) {
    ReceiveState.STOPPED -> "연결 중지됨"
    ReceiveState.SEARCHING -> "기기를 찾고 있어요"
    ReceiveState.CONNECTING -> "연결 중…"
    ReceiveState.READING -> "배터리 확인 중…"
    ReceiveState.CONNECTED -> "연결됨 · 배터리 수신 중"
    ReceiveState.RETRYING -> "연결을 다시 시도하고 있어요"
    ReceiveState.WAITING_FOR_BLUETOOTH -> "블루투스를 켜면 다시 연결해요"
    ReceiveState.ERROR -> "연결 오류"
    ReceiveState.SEND_ONLY -> "배터리 보내기 켜짐 · 송신 전용"
}

@Composable
private fun HistoryGroup(status: BridgeSnapshot) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Group {
        Disclosure("최근 상태 기록", expanded, { expanded = !expanded })
        if (expanded) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SecondaryText("앱 실행 중의 최근 상태만 표시해요.")
            if (status.events.isEmpty()) SecondaryText("아직 기록이 없어요.")
            status.events.asReversed().forEach { event ->
                Column {
                    Text(event.description, style = MaterialTheme.typography.bodyLarge)
                    SecondaryText(DateFormat.getTimeInstance(DateFormat.MEDIUM).format(Date(event.timeMillis)))
                }
            }
        }
    }
}

@Composable
private fun Group(content: @Composable ColumnScope.() -> Unit) {
    Surface(shape = BridgeDesign.groupShape, color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) { Column(content = content) }
}

@Composable
private fun SectionTitle(title: String) {
    Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 8.dp, bottom = 10.dp).semantics { heading() })
}

@Composable
private fun SecondaryText(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun StatusText(text: String, error: Boolean) {
    Text(text, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
}

@Composable
private fun Disclosure(label: String, expanded: Boolean, onClick: () -> Unit) {
    TextButton(onClick = onClick, shape = BridgeDesign.groupShape,
        modifier = Modifier.fillMaxWidth().heightIn(min = 60.dp).semantics { stateDescription = if (expanded) "펼쳐짐" else "접힘" },
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 16.dp),
        colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.onSurface)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(12.dp))
            Text(if (expanded) "접기" else "보기", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}
