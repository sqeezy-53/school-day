package io.github.sqeezy53.schoolday

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.LuminanceSource
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import org.json.JSONArray
import org.json.JSONObject

@Composable
internal fun SyncScreen(store: Store) {
    var invite by remember { mutableStateOf("") }
    var scanError by remember { mutableStateOf("") }
    var revoke by remember { mutableStateOf(false) }
    var scanner by remember { mutableStateOf(false) }
    var modes by remember { mutableStateOf(false) }
    var network by remember { mutableStateOf(false) }
    var pcAddress by remember { mutableStateOf("") }
    val context = LocalContext.current
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        scanner = allowed
        if (!allowed) scanError = "Камера недоступна. Введите код с компьютера."
    }
    LaunchedEffect(Unit) { store.sync("info") }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).imePadding().padding(bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (store.lanPaired) {
            Text("Компьютер подключён", style = MaterialTheme.typography.titleMedium)
            store.lanLastSync?.let {
                val date = Instant.ofEpochSecond(it).atZone(ZoneId.systemDefault())
                    .format(DateTimeFormatter.ofPattern("dd.MM.yyyy HH:mm"))
                Text("Последний обмен · $date")
            }
            if (store.lanQueued) Text("Есть изменения, ожидающие обмена")
            Box {
                OutlinedButton(onClick = { modes = true }) { Text(store.lanMode.label) }
                DropdownMenu(expanded = modes, onDismissRequest = { modes = false }) {
                    LanMode.entries.forEach { mode ->
                        DropdownMenuItem(text = { Text(mode.label) }, onClick = {
                            modes = false
                            store.changeLanMode(mode)
                        })
                    }
                }
            }
            if (store.lanMode == LanMode.Automatic) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Фоновые попытки", modifier = Modifier.weight(1f))
                    Switch(checked = store.lanBackground, onCheckedChange = store::changeLanBackground)
                }
            } else if (store.lanMode == LanMode.Manual) {
                Text(if (store.lanLastSync == null) "Запустите первый обмен на обоих устройствах."
                    else "Включите автоматический режим на обоих устройствах для обмена без подтверждения.")
            }
            Button(onClick = { store.sync("sync") }, enabled = !store.busy && store.lanMode != LanMode.Off) {
                Text("Синхронизировать")
            }
            TextButton(onClick = { revoke = true }, enabled = !store.busy) {
                Text("Отключить устройство")
            }
            TextButton(onClick = { network = !network }) { Text("Параметры сети") }
            if (network) {
                OutlinedTextField(
                    value = pcAddress,
                    onValueChange = { if (it.length <= 15) pcAddress = it },
                    label = { Text("Новый адрес компьютера") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                TextButton(
                    onClick = { store.sync("address", pcAddress) },
                    enabled = pcAddress.isNotBlank() && !store.busy,
                ) { Text("Обновить адрес") }
            }
        } else {
            Text("Подключите компьютер", style = MaterialTheme.typography.titleMedium)
            Text("Откройте QR-код сопряжения в School Day на компьютере.")
            OutlinedButton(
                onClick = {
                    scanError = ""
                    if (context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
                        scanner = true
                    } else permission.launch(Manifest.permission.CAMERA)
                },
                enabled = !store.busy,
            ) { Text("Сканировать QR") }
            OutlinedTextField(
                value = invite,
                onValueChange = {
                    if (it.length <= 240) invite = it
                },
                label = { Text("Код сопряжения") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = {
                    store.sync("pair", invite)
                    invite = ""
                },
                enabled = invite.isNotBlank() && !store.busy,
            ) { Text("Подключить компьютер") }
        }
        if (scanError.isNotBlank()) Text(scanError, color = MaterialTheme.colorScheme.error)
        if (store.lanMessage.isNotBlank()) Text(store.lanMessage, style = MaterialTheme.typography.bodyMedium)
        if (store.busy) TextButton(onClick = store::cancelLan) { Text("Отменить обмен") }
    }

    if (scanner) {
        LanQrScanner(
            onClose = { scanner = false },
            onCode = { code ->
                scanner = false
                store.sync("pair", code)
            },
            onError = {
                scanner = false
                scanError = it
            },
        )
    }
    if (revoke) {
        AlertDialog(
            onDismissRequest = { revoke = false },
            title = { Text("Отключить компьютер?") },
            text = { Text("Для следующего обмена потребуется новое сопряжение.") },
            confirmButton = {
                TextButton(onClick = {
                    revoke = false
                    store.sync("revoke")
                }) { Text("Отключить") }
            },
            dismissButton = { TextButton(onClick = { revoke = false }) { Text("Отмена") } },
        )
    }
    store.lanPrompt?.let { prompt ->
        val pair = prompt.value("type") == "pair"
        var choices by remember(prompt) {
            val preferred = prompt.optJSONArray("choices")?.let { versions ->
                List(versions.length(), versions::getString)
            }.orEmpty()
            mutableStateOf(preferred)
        }
        AlertDialog(
            onDismissRequest = { store.answerLan() },
            title = { Text(if (pair) "Сравните код с ПК" else "Применить изменения?") },
            text = {
                if (pair) Text(prompt.getString("code"), style = MaterialTheme.typography.headlineSmall)
                else SyncReview(prompt.getJSONObject("review"), choices) { index, option ->
                    choices = choices.toMutableList().apply { set(index, option) }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    store.answerLan(if (pair) "true" else JSONArray(choices).toString())
                }) {
                    Text(if (pair) "Код совпадает" else "Применить")
                }
            },
            dismissButton = { TextButton(onClick = { store.answerLan() }) { Text("Отмена") } },
        )
    }
}

@Composable
private fun SyncReview(review: JSONObject, choices: List<String>, select: (Int, String) -> Unit) {
    val incoming = review.getJSONObject("outgoing")
    val outgoing = review.getJSONObject("incoming")
    Column(
        Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(delta("На телефон", incoming))
        Text(delta("На ПК", outgoing))
        review.list("conflicts").forEachIndexed { index, conflict ->
            HorizontalDivider()
            ConflictOption("Версия с ПК", conflict.optJSONObject("desktop"), choices[index] == "Local") {
                select(index, "Local")
            }
            ConflictOption("Версия с Android", conflict.optJSONObject("android"), choices[index] == "Incoming") {
                select(index, "Incoming")
            }
        }
    }
}

@Composable
private fun ConflictOption(label: String, record: JSONObject?, selected: Boolean, select: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp)
            .selectable(selected = selected, role = Role.RadioButton, onClick = select),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Column {
            Text(label, style = MaterialTheme.typography.labelLarge)
            Text(describe(record), style = MaterialTheme.typography.bodySmall)
        }
    }
}

private fun delta(name: String, counts: JSONObject): String =
    "$name · новых ${counts.getInt("added")}, изменённых ${counts.getInt("changed")}, удалённых ${counts.getInt("deleted")}"

private fun describe(record: JSONObject?): String {
    if (record == null) return "Запись удалена"
    return buildList {
        for (key in listOf("title", "date", "time", "end", "room", "note", "homework", "body")) {
            val value = record.value(key)
            if (value.isNotBlank()) add(value.take(950))
        }
        if (record.optBoolean("remote")) add("Дистант")
        if (record.has("short_minutes")) add("${record.getInt("short_minutes")} мин на урок")
        record.optJSONObject("options")?.let { options ->
            if (options.optBoolean("cancelled")) add("Урок отменён")
            if (options.value("replacement").isNotEmpty()) add("Замена · ${options.value("replacement")}")
            when (options.value("work")) {
                "test" -> add("Контрольная")
                "independent" -> add("Самостоятельная")
            }
        }
        for (key in listOf("completed", "homework_completed")) {
            if (record.has(key)) {
                add(if (record.optBoolean(key)) "ДЗ выполнено" else "ДЗ к выполнению")
                break
            }
        }
    }.joinToString("\n").ifBlank { "Обычный день" }
}

internal fun decodeInvitation(photo: Bitmap): String? {
    val longest = maxOf(photo.width, photo.height)
    if (longest == 0) return null
    val ratio = 1_280.0 / longest
    val image = if (ratio >= 1) photo else Bitmap.createScaledBitmap(
        photo, (photo.width * ratio).toInt().coerceAtLeast(1),
        (photo.height * ratio).toInt().coerceAtLeast(1), true,
    )
    return try {
        val pixels = IntArray(image.width * image.height)
        image.getPixels(pixels, 0, image.width, 0, 0, image.width, image.height)
        val source = RGBLuminanceSource(image.width, image.height, pixels)
        decodeInvitation(source)
    } finally {
        if (image !== photo) image.recycle()
    }
}

internal fun decodeInvitation(source: LuminanceSource): String? =
    runCatching {
        QRCodeReader().decode(
            BinaryBitmap(HybridBinarizer(source)), mapOf(DecodeHintType.TRY_HARDER to true)
        ).text
    }.getOrNull()?.takeIf { it.startsWith("SD1/") && it.length <= 240 && Core.lanInviteValid(it) }
