package ae.heej.nearlink

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
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
enum class NoticeAction(@param:StringRes val label: Int) { APP_SETTINGS(R.string.notice_open_app_settings), ENABLE_BLUETOOTH(R.string.notice_enable_bluetooth) }

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
        Column(Modifier.widthIn(max = if (expanded) BridgeDesign.maxWidth else BridgeDesign.contentWidth).fillMaxSize()) {
            // Each mode has its own scroll position. A successful first read starts management at the top.
            key(managed) {
                if (managed && expanded) {
                    // Wide windows keep controls beside the device list instead of stretching one column.
                    Row(Modifier.weight(1f).padding(horizontal = BridgeDesign.pageInset),
                        horizontalArrangement = Arrangement.spacedBy(BridgeDesign.gap)) {
                        Column(Modifier.width(BridgeDesign.sidePanelWidth).fillMaxHeight().verticalScroll(rememberScrollState())) {
                            PageHeader(managed = true, expanded = true)
                            notice?.let { NoticeCard(it, actions) }
                            ManagedControls(status, pairing, batteryRestricted, { showAddOptions = true }, actions)
                            Spacer(Modifier.height(BridgeDesign.gap))
                            ManagedSettings(status, alertSettings, actions) { confirmStopAll = true }
                            Spacer(Modifier.height(24.dp))
                        }
                        Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState())) {
                            Spacer(Modifier.height(28.dp))
                            ManagedPeers(status, now, actions)
                            Spacer(Modifier.height(24.dp))
                        }
                    }
                } else {
                    Column(Modifier.weight(1f).verticalScroll(rememberScrollState())
                        .padding(horizontal = BridgeDesign.pageInset)) {
                        PageHeader(managed, expanded)
                        notice?.let { NoticeCard(it, actions) }
                        if (managed) {
                            ManagedControls(status, pairing, batteryRestricted, { showAddOptions = true }, actions)
                            Spacer(Modifier.height(BridgeDesign.gap))
                            ManagedPeers(status, now, actions)
                            ManagedSettings(status, alertSettings, actions) { confirmStopAll = true }
                        } else {
                            if (setupRequested && status.hasCompletedSetup()) {
                                TextButton(onClick = actions.onDismissSetup, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.back_to_battery)) }
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
                                SectionTitle(stringResource(R.string.section_connection_status))
                                PeerList(status, now, actions)
                            }
                        }
                        Spacer(Modifier.height(24.dp))
                    }
                }
            }
            if (!managed) {
                Surface(color = MaterialTheme.colorScheme.background) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = BridgeDesign.pageInset, vertical = 12.dp),
                        horizontalArrangement = if (expanded) Arrangement.End else Arrangement.Center) {
                        Button(onClick = { showAddOptions = true }, enabled = !pairing, shape = BridgeDesign.controlShape,
                            modifier = (if (expanded) Modifier.widthIn(min = 240.dp, max = 360.dp) else Modifier.fillMaxWidth()).heightIn(min = 52.dp),
                            contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp)) {
                            Text(if (pairing) stringResource(R.string.adding_device) else stringResource(R.string.add_device))
                        }
                    }
                }
            }
        }
    }
    if (showAddOptions) {
        AlertDialog(onDismissRequest = { showAddOptions = false }, title = { Text(stringResource(R.string.add_device)) },
            text = {
                Column {
                    SecondaryText(stringResource(R.string.add_options_hint))
                    TextButton(onClick = { showAddOptions = false; actions.onAdd() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text(stringResource(R.string.find_new_device)) }
                    TextButton(onClick = { showAddOptions = false; actions.onConnect() }, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp)) { Text(stringResource(R.string.connect_paired_device)) }
                }
            }, confirmButton = { TextButton(onClick = { showAddOptions = false }) { Text(stringResource(R.string.close)) } })
    }
    if (confirmStopAll) {
        AlertDialog(onDismissRequest = { confirmStopAll = false }, title = { Text(stringResource(R.string.stop_all_title)) },
            text = { Text(stringResource(R.string.stop_all_body)) },
            confirmButton = {
                TextButton(onClick = { confirmStopAll = false; actions.onStopAll() },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(stringResource(R.string.stop_all_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmStopAll = false }) { Text(stringResource(R.string.cancel)) } })
    }
}

@Composable
private fun PageHeader(managed: Boolean, expanded: Boolean) {
    Column(Modifier.fillMaxWidth().padding(top = if (expanded) 28.dp else 40.dp, bottom = 28.dp)) {
        Text("NearLink", style = MaterialTheme.typography.headlineLarge,
            modifier = Modifier.semantics { heading() })
        Spacer(Modifier.height(8.dp))
        SecondaryText(if (managed) stringResource(R.string.header_managed) else stringResource(R.string.header_setup))
    }
}

@Composable
private fun ManagedControls(status: BridgeSnapshot, pairing: Boolean, batteryRestricted: Boolean,
    onAdd: () -> Unit, actions: BridgeActions) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalButton(onClick = onAdd, enabled = !pairing,
            modifier = Modifier.weight(1f).heightIn(min = 48.dp)) { Text(stringResource(R.string.add_device)) }
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
}

@Composable
private fun ManagedPeers(status: BridgeSnapshot, now: Long, actions: BridgeActions) {
    SectionTitle(stringResource(R.string.section_connected_devices))
    PeerList(status, now, actions)
}

@Composable
private fun ManagedSettings(status: BridgeSnapshot, alertSettings: PeerAlertSettings, actions: BridgeActions,
    onStopAll: () -> Unit) {
    Group {
        TextButton(onClick = onStopAll, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp),
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(stringResource(R.string.stop_all_connections)) }
    }
    Spacer(Modifier.height(BridgeDesign.gap))
    AlertSettingsGroup(alertSettings, actions.onAlertSettings)
    Spacer(Modifier.height(BridgeDesign.gap))
    HistoryGroup(status)
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
                    if (notice.dismissible) {
                        val dismissLabel = stringResource(R.string.dismiss_message)
                        TextButton(onClick = actions.onDismissNotice,
                            colors = ButtonDefaults.textButtonColors(contentColor = LocalContentColor.current),
                            modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = dismissLabel }) { Text(stringResource(R.string.close)) }
                    }
                    notice.action?.let { action ->
                        TextButton(onClick = { actions.onNoticeAction(action) },
                            colors = ButtonDefaults.textButtonColors(contentColor = LocalContentColor.current),
                            modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(action.label), fontWeight = FontWeight.SemiBold) }
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
            Text(stringResource(R.string.hint_drops_title), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            SecondaryText(stringResource(R.string.hint_drops_body))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onOpen, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.open_battery_settings)) }
            }
        }
    }
}

@Composable
private fun SetupGuide() {
    var showNotes by rememberSaveable { mutableStateOf(false) }
    SectionTitle(stringResource(R.string.section_how_to))
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.guide_title), style = MaterialTheme.typography.titleLarge)
            SecondaryText(stringResource(R.string.guide_step))
            SecondaryText(stringResource(R.string.guide_paired))
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant, modifier = Modifier.padding(horizontal = 24.dp))
        Disclosure(stringResource(R.string.guide_notes_title), showNotes, { showNotes = !showNotes })
        if (showNotes) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp)) {
            SecondaryText(stringResource(R.string.guide_notes_body))
        }
    }
}

@Composable
private fun PreparationGroup(status: BridgeSnapshot, pairing: Boolean, onSend: () -> Unit, onStop: () -> Unit) {
    SectionTitle(stringResource(R.string.prepare_to_connect))
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
                StatusText(if (pairing) stringResource(R.string.waiting_for_pairing) else receiveText(status.receiveState),
                    status.receiveState == ReceiveState.ERROR)
                status.receiveDetail?.let { SecondaryText(it.asString()) }
            }
            // Preparation and a first connection can run services before the first read: keep cancellation reachable.
            if (pairing || status.sendState != SendState.STOPPED || status.receiveState != ReceiveState.STOPPED) {
                TextButton(onClick = onStop, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(stringResource(R.string.stop_connecting)) }
            }
        }
    }
}

@Composable
private fun preparationButtonText(status: BridgeSnapshot): String = when {
    status.sendState == SendState.ERROR -> stringResource(R.string.prepare_again)
    status.sendState == SendState.STARTING -> stringResource(R.string.preparing_ellipsis)
    status.preparationState == PreparationState.OPEN -> stringResource(R.string.extend_preparation)
    status.preparationState == PreparationState.EXPIRED -> stringResource(R.string.prepare_again)
    else -> stringResource(R.string.prepare_to_connect)
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
        status.sendState == SendState.ERROR -> stringResource(R.string.prep_failed)
        status.preparationState == PreparationState.EXPIRED -> stringResource(R.string.prep_expired)
        status.sendState == SendState.WAITING_FOR_BLUETOOTH -> stringResource(R.string.bt_is_off)
        status.sendState == SendState.STARTING -> stringResource(R.string.prep_in_progress)
        status.preparationState == PreparationState.ACCEPTED -> stringResource(R.string.prep_accepted)
        status.preparationState == PreparationState.OPEN && seconds == 0L -> stringResource(R.string.prep_expired)
        status.preparationState == PreparationState.OPEN && status.sendState == SendState.ADVERTISING -> stringResource(R.string.prep_ready)
        else -> stringResource(R.string.prep_idle)
    }
    StatusText(title, status.sendState == SendState.ERROR)
    if (status.preparationState == PreparationState.OPEN && seconds > 0L && status.sendState == SendState.ADVERTISING) {
        // Keep the ticking value outside the live region so TalkBack announces the state, not every second.
        Text(stringResource(R.string.time_left, seconds / 60, seconds % 60), style = MaterialTheme.typography.titleMedium)
        SecondaryText(stringResource(R.string.prep_ready_hint))
    } else if (status.sendState != SendState.ERROR && (status.preparationState == PreparationState.EXPIRED ||
        (status.preparationState == PreparationState.OPEN && seconds == 0L))) {
        SecondaryText(stringResource(R.string.prep_expired_hint))
    } else if (status.sendState == SendState.WAITING_FOR_BLUETOOTH) {
        SecondaryText(stringResource(R.string.prep_bt_hint))
    } else if (status.preparationState == PreparationState.ACCEPTED && status.sendState != SendState.ERROR) {
        SecondaryText(stringResource(R.string.prep_accepted_hint))
    }
    status.sendDetail?.let { SecondaryText(it.asString()) }
}

@Composable
private fun PeerList(status: BridgeSnapshot, now: Long, actions: BridgeActions) {
    val peers = status.peers.values.sortedBy { it.displayName }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val columns = if (maxWidth >= BridgeDesign.gridWidth) 2 else 1
        val wide = (maxWidth - BridgeDesign.gap * (columns - 1)) / columns >= BridgeDesign.wideCardWidth
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(BridgeDesign.gap)) {
            repeat(columns) { column ->
                Column(Modifier.weight(1f)) {
                    peers.forEachIndexed { index, peer ->
                        if (index % columns == column) {
                            key(peer.peerId) { PeerItem(peer, now, actions, wide) }
                            Spacer(Modifier.height(BridgeDesign.gap))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun PeerItem(peer: PeerSnapshot, now: Long, actions: BridgeActions, wide: Boolean) {
    var renaming by rememberSaveable { mutableStateOf(false) }
    var forgetting by rememberSaveable { mutableStateOf(false) }
    val reading = if (peer.sendOnly) null else peer.lastReading
    val canReconnect = peer.receiveState == ReceiveState.STOPPED || peer.receiveState == ReceiveState.ERROR
    val canStop = peer.receiveState != ReceiveState.STOPPED
    Group {
        Column(Modifier.padding(BridgeDesign.groupInset), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (wide && reading != null) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        PeerSummary(peer)
                        ReadingDetails(peer, reading, now)
                    }
                    Text("${reading.level}%", style = MaterialTheme.typography.displayMedium)
                }
            } else {
                PeerSummary(peer)
                if (peer.sendOnly) SecondaryText(stringResource(R.string.peer_send_only_note))
                else if (reading != null) {
                    Text("${reading.level}%", style = MaterialTheme.typography.displayMedium)
                    ReadingDetails(peer, reading, now)
                } else SecondaryText(stringResource(R.string.peer_no_reading))
            }
            peer.detail?.let { SecondaryText(it.asString()) }
            if (wide) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                    verticalAlignment = Alignment.CenterVertically) {
                    if (canReconnect) FilledTonalButton(onClick = { actions.onReconnect(peer.peerId) },
                        modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.reconnect)) }
                    if (canStop) TextButton(onClick = { actions.onStopPeer(peer.peerId) }, modifier = Modifier.heightIn(min = 48.dp),
                        colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(stringResource(R.string.stop_connection)) }
                    DeviceMenu(peer, Modifier, fill = false, onRename = { renaming = true }, onForget = { forgetting = true })
                }
            } else {
                if (canReconnect) {
                    FilledTonalButton(onClick = { actions.onReconnect(peer.peerId) }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(stringResource(R.string.reconnect)) }
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (canStop) {
                        TextButton(onClick = { actions.onStopPeer(peer.peerId) }, modifier = Modifier.weight(1f).heightIn(min = 48.dp),
                            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(stringResource(R.string.stop_connection)) }
                    }
                    DeviceMenu(peer, Modifier.weight(1f), fill = true, onRename = { renaming = true }, onForget = { forgetting = true })
                }
            }
        }
    }
    if (renaming) RenameDialog(peer, onDismiss = { renaming = false }, onSave = { actions.onRename(peer.peerId, it); renaming = false })
    if (forgetting) {
        AlertDialog(onDismissRequest = { forgetting = false }, title = { Text(stringResource(R.string.forget_title)) },
            text = { Text(stringResource(R.string.forget_body, peer.displayText().asString())) },
            confirmButton = {
                TextButton(onClick = { forgetting = false; actions.onForget(peer.peerId) },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(stringResource(R.string.remove)) }
            },
            dismissButton = { TextButton(onClick = { forgetting = false }) { Text(stringResource(R.string.cancel)) } })
    }
}

@Composable
private fun PeerSummary(peer: PeerSnapshot) {
    Text(peer.displayText().asString(), style = MaterialTheme.typography.titleLarge)
    if (peer.alias != null) SecondaryText(stringResource(R.string.bluetooth_name, peer.peerName))
    StatusText(receiveText(peer.receiveState), peer.receiveState == ReceiveState.ERROR)
}

@Composable
private fun ReadingDetails(peer: PeerSnapshot, reading: LastReading, now: Long) {
    Text(when {
        reading.charging == true -> stringResource(R.string.charge_charging)
        reading.charging == false && reading.plugged == true && reading.level == 100 -> stringResource(R.string.charge_full_plugged)
        reading.charging == false && reading.plugged == true -> stringResource(R.string.charge_plugged_not_charging)
        reading.charging == false -> stringResource(R.string.charge_not_charging)
        else -> stringResource(R.string.charge_unknown)
    }, style = MaterialTheme.typography.bodyLarge)
    SecondaryText(stringResource(R.string.last_received, relativeAge(now, reading.timeMillis).asString(),
        DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(reading.timeMillis))))
    if (peer.receiveState != ReceiveState.CONNECTED) SecondaryText(stringResource(R.string.reading_stale))
}

@Composable
private fun DeviceMenu(peer: PeerSnapshot, modifier: Modifier, fill: Boolean, onRename: () -> Unit, onForget: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    val stateText = stringResource(if (menu) R.string.state_expanded else R.string.state_collapsed)
    Box(modifier) {
        TextButton(onClick = { menu = true }, modifier = (if (fill) Modifier.fillMaxWidth() else Modifier).heightIn(min = 48.dp)
            .semantics { stateDescription = stateText }) { Text(stringResource(R.string.device_settings)) }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false },
            shape = BridgeDesign.controlShape, containerColor = MaterialTheme.colorScheme.surface,
            tonalElevation = 0.dp, shadowElevation = 8.dp) {
            DropdownMenuItem(text = { Text(stringResource(R.string.change_nickname), style = MaterialTheme.typography.bodyLarge) },
                modifier = Modifier.heightIn(min = 48.dp),
                contentPadding = PaddingValues(horizontal = 20.dp),
                onClick = { menu = false; onRename() })
            val canForget = peer.receiveState == ReceiveState.STOPPED
            DropdownMenuItem(
                text = {
                    Column {
                        Text(stringResource(R.string.remove_from_list), style = MaterialTheme.typography.bodyLarge)
                        if (!canForget) Text(stringResource(R.string.stop_connection_first), style = MaterialTheme.typography.bodyMedium)
                    }
                },
                modifier = Modifier.heightIn(min = 48.dp),
                contentPadding = PaddingValues(horizontal = 20.dp),
                enabled = canForget,
                onClick = { menu = false; onForget() })
        }
    }
}

@Composable
private fun RenameDialog(peer: PeerSnapshot, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf(peer.alias ?: "") }
    AlertDialog(onDismissRequest = onDismiss, title = { Text(stringResource(R.string.change_nickname)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = text, onValueChange = { text = it.take(48) }, singleLine = true,
                    label = { Text(stringResource(R.string.nickname)) }, placeholder = { Text(peer.peerName) },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done), modifier = Modifier.fillMaxWidth())
                SecondaryText(stringResource(R.string.nickname_hint))
            }
        },
        confirmButton = { TextButton(onClick = { onSave(text) }) { Text(stringResource(R.string.save)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } })
}

@Composable
private fun AlertSettingsGroup(settings: PeerAlertSettings, onChange: (PeerAlertSettings) -> Unit) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    var threshold by remember(settings.threshold) { mutableFloatStateOf(settings.threshold.toFloat()) }
    val thresholdLabel = stringResource(R.string.alert_low_threshold_desc)
    Group {
        Disclosure(stringResource(R.string.alerts_title), expanded, { expanded = !expanded })
        if (expanded) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SettingSwitch(stringResource(R.string.alert_low_title), stringResource(R.string.alert_low_desc), settings.low) { onChange(settings.copy(low = it)) }
            if (settings.low) {
                Text(stringResource(R.string.threshold_at_or_below, threshold.roundToInt()), style = MaterialTheme.typography.bodyLarge)
                Slider(value = threshold, onValueChange = { threshold = it }, valueRange = 5f..50f, steps = 8,
                    onValueChangeFinished = { onChange(settings.copy(threshold = threshold.roundToInt())) },
                    modifier = Modifier.semantics { contentDescription = thresholdLabel })
            }
            SettingSwitch(stringResource(R.string.alert_full_title), stringResource(R.string.alert_full_desc), settings.full) { onChange(settings.copy(full = it)) }
            SecondaryText(stringResource(R.string.alerts_note))
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

@Composable
private fun receiveText(state: ReceiveState): String = when (state) {
    ReceiveState.STOPPED -> stringResource(R.string.recv_text_stopped)
    ReceiveState.SEARCHING -> stringResource(R.string.recv_text_searching)
    ReceiveState.CONNECTING -> stringResource(R.string.recv_text_connecting)
    ReceiveState.READING -> stringResource(R.string.recv_text_reading)
    ReceiveState.CONNECTED -> stringResource(R.string.recv_text_connected)
    ReceiveState.RETRYING -> stringResource(R.string.recv_text_retrying)
    ReceiveState.WAITING_FOR_BLUETOOTH -> stringResource(R.string.recv_text_waiting_bt)
    ReceiveState.ERROR -> stringResource(R.string.recv_text_error)
    ReceiveState.SEND_ONLY -> stringResource(R.string.recv_text_send_only)
}

@Composable
private fun HistoryGroup(status: BridgeSnapshot) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Group {
        Disclosure(stringResource(R.string.history_title), expanded, { expanded = !expanded })
        if (expanded) Column(Modifier.padding(horizontal = BridgeDesign.groupInset).padding(bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SecondaryText(stringResource(R.string.history_note))
            if (status.events.isEmpty()) SecondaryText(stringResource(R.string.history_empty))
            status.events.asReversed().forEach { event ->
                Column {
                    Text(event.description.asString(), style = MaterialTheme.typography.bodyLarge)
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
    val stateText = stringResource(if (expanded) R.string.state_expanded else R.string.state_collapsed)
    TextButton(onClick = onClick, shape = BridgeDesign.groupShape,
        modifier = Modifier.fillMaxWidth().heightIn(min = 60.dp).semantics { stateDescription = stateText },
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 16.dp),
        colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.onSurface)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(12.dp))
            Text(if (expanded) stringResource(R.string.collapse) else stringResource(R.string.expand), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}
