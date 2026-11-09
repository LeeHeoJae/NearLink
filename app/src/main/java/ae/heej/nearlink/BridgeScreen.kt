package ae.heej.nearlink

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

@Composable
internal fun BridgeScreen(status: BridgeSnapshot, message: String?, messageIsError: Boolean, pairing: Boolean,
    onSend: () -> Unit, onConnect: () -> Unit, onAdd: () -> Unit, onStop: () -> Unit,
    onReconnect: () -> Unit, setupRequested: Boolean, onSetupRequested: () -> Unit, onDismissSetup: () -> Unit) {
    val managed = status.hasCompletedSetup() && !setupRequested
    var showAddOptions by rememberSaveable { mutableStateOf(false) }
    var showDeviceOptions by rememberSaveable { mutableStateOf(false) }
    BackHandler(enabled = setupRequested && status.hasCompletedSetup(), onBack = onDismissSetup)
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
                    message?.let {
                        Surface(shape = BridgeDesign.groupShape,
                            color = if (messageIsError) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.primaryContainer,
                            contentColor = if (messageIsError) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onPrimaryContainer,
                            modifier = Modifier.fillMaxWidth().padding(bottom = BridgeDesign.gap)) {
                            Text(it, style = MaterialTheme.typography.bodyLarge,
                                modifier = Modifier.padding(20.dp).semantics { liveRegion = LiveRegionMode.Polite })
                        }
                    }
                    if (managed) {
                        if (expanded) {
                            Row(horizontalArrangement = Arrangement.spacedBy(BridgeDesign.gap)) {
                                Column(Modifier.weight(1.1f)) { PeerGroup(status, onReconnect) }
                                Column(Modifier.weight(1f)) { ManagementGroup(status, onStop, onReconnect, { showDeviceOptions = true }) }
                            }
                        } else {
                            PeerGroup(status, onReconnect)
                            Spacer(Modifier.height(BridgeDesign.gap))
                            ManagementGroup(status, onStop, onReconnect, { showDeviceOptions = true })
                        }
                        Spacer(Modifier.height(BridgeDesign.gap))
                        HistoryGroup(status)
                    } else {
                        if (setupRequested && status.hasCompletedSetup()) {
                            TextButton(onClick = onDismissSetup, modifier = Modifier.heightIn(min = 48.dp)) { Text("배터리 화면으로 돌아가기") }
                        }
                        if (expanded) {
                            Row(horizontalArrangement = Arrangement.spacedBy(BridgeDesign.gap)) {
                                Column(Modifier.weight(1.1f)) { SetupGuide() }
                                Column(Modifier.weight(1f)) { PreparationGroup(status, pairing, onSend, onStop) }
                            }
                        } else {
                            SetupGuide()
                            Spacer(Modifier.height(BridgeDesign.gap))
                            PreparationGroup(status, pairing, onSend, onStop)
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
                    TextButton(onClick = { showAddOptions = false; onAdd() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("새 기기 찾기") }
                    TextButton(onClick = { showAddOptions = false; onConnect() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("기존 페어링 기기 연결") }
                }
            }, confirmButton = { TextButton(onClick = { showAddOptions = false }) { Text("닫기") } })
    }
    if (showDeviceOptions) {
        AlertDialog(onDismissRequest = { showDeviceOptions = false }, title = { Text("기기 연결 관리") },
            text = {
                Column {
                    TextButton(onClick = { showDeviceOptions = false; onReconnect() }, enabled = !pairing,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("다시 연결") }
                    TextButton(onClick = { showDeviceOptions = false; onSetupRequested() }, enabled = !pairing,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text("다른 기기 연결") }
                    SecondaryText("기기를 변경해도 이전 기기의 잔량을 새 기기의 값으로 표시하지 않아요.")
                }
            }, confirmButton = { TextButton(onClick = { showDeviceOptions = false }) { Text("닫기") } })
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
            StatusText(when (status.sendState) {
                SendState.STOPPED -> "이 기기를 연결할 준비가 필요해요"
                SendState.STARTING -> "연결 준비 중…"
                SendState.ADVERTISING -> "배터리 서비스 실행 중"
                SendState.WAITING_FOR_BLUETOOTH -> "블루투스를 켜면 다시 준비해요"
                SendState.ERROR -> "연결 준비 오류"
            }, status.sendState == SendState.ERROR)
            SecondaryText("이 기기를 상대 앱에서 추가하려면 ‘연결 준비’를 누르세요.")
            status.sendDetail?.let { SecondaryText(it) }
            FilledTonalButton(onClick = onSend,
                enabled = !pairing && status.sendState != SendState.STARTING && status.sendState != SendState.WAITING_FOR_BLUETOOTH,
                shape = BridgeDesign.controlShape, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), contentPadding = PaddingValues(12.dp)) {
                Text(if (status.sendState == SendState.ERROR) "다시 준비" else "연결 준비")
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

@Composable
private fun PeerGroup(status: BridgeSnapshot, onReconnect: () -> Unit) {
    val reading = status.visibleReading() ?: return
    SectionTitle("다른 기기 배터리")
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(status.peerName ?: reading.peerName, style = MaterialTheme.typography.titleLarge)
            StatusText(receiveText(status.receiveState), status.receiveState == ReceiveState.ERROR)
            Text("${reading.level}%", style = MaterialTheme.typography.displayMedium, modifier = Modifier.padding(top = 6.dp))
            Text(when {
                reading.charging == true -> "충전 중"
                reading.charging == false && reading.plugged == true && reading.level == 100 -> "전원 연결 · 완충"
                reading.charging == false && reading.plugged == true -> "전원 연결 · 충전 중 아님"
                reading.charging == false -> "충전 중 아님"
                else -> "충전 상태 알 수 없음"
            }, style = MaterialTheme.typography.bodyLarge)
            SecondaryText("마지막 수신 · " + DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(reading.timeMillis)))
            SecondaryText("마지막으로 읽은 값이며 현재 잔량과 다를 수 있어요.")
            status.receiveDetail?.let { SecondaryText(it) }
            if (status.receiveState == ReceiveState.STOPPED || status.receiveState == ReceiveState.ERROR) {
                FilledTonalButton(onClick = onReconnect, shape = BridgeDesign.controlShape,
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), contentPadding = PaddingValues(12.dp)) { Text("다시 연결") }
            }
        }
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
}

@Composable
private fun ManagementGroup(status: BridgeSnapshot, onStop: () -> Unit, onReconnect: () -> Unit, onOptions: () -> Unit) {
    var showHelp by rememberSaveable { mutableStateOf(false) }
    SectionTitle("연결 관리")
    Group {
        TextButton(onClick = onOptions, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp), contentPadding = PaddingValues(20.dp)) { Text("기기 연결 관리") }
        if (status.sendState == SendState.STOPPED) status.sendDetail?.let {
            Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 12.dp)) { SecondaryText(it) }
        }
        if (status.sendState == SendState.ERROR || status.sendState == SendState.WAITING_FOR_BLUETOOTH) {
            Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                StatusText(if (status.sendState == SendState.ERROR) "배터리 보내기 오류" else "배터리 보내기 · 블루투스 대기", status.sendState == SendState.ERROR)
                status.sendDetail?.let { SecondaryText(it) }
                if (status.sendState == SendState.ERROR) TextButton(onClick = onReconnect) { Text("송수신 다시 시작") }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant, modifier = Modifier.padding(horizontal = 24.dp))
        TextButton(onClick = onStop, enabled = status.sendState != SendState.STOPPED || status.receiveState != ReceiveState.STOPPED,
            modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp), contentPadding = PaddingValues(20.dp),
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("양쪽 연결 중지") }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant, modifier = Modifier.padding(horizontal = 24.dp))
        Disclosure("알아둘 점", showHelp, { showHelp = !showHelp })
        if (showHelp) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SecondaryText("앱의 연결 상태와 삼성 위젯 표시는 다를 수 있어요. 위젯에서도 확인해 주세요.")
            SecondaryText("선택한 페어링 기기에만 암호화된 연결로 배터리 정보를 제공해요. 위젯 호환성은 확인 중이에요.")
            SecondaryText("한쪽에서 중지하면 연결된 상대 기기에도 중지를 요청해요. 연결이 끊겨 있으면 상대 기기에서도 직접 중지해 주세요. 저장된 마지막 잔량은 유지해요.")
        }
    }
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
