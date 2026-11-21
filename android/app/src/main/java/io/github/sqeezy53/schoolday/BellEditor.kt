package io.github.sqeezy53.schoolday

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import org.json.JSONArray
import org.json.JSONObject

@Composable
internal fun BellEditor(initial: JSONObject, store: Store, close: () -> Unit) {
    val originalStart = initial.getString("first_start")
    var start by rememberSaveable { mutableStateOf(originalStart.take(5)) }
    var duration by rememberSaveable { mutableIntStateOf(initial.getInt("lesson_minutes")) }
    var breaks by rememberSaveable {
        val values = initial.getJSONArray("breaks")
        mutableStateOf(ArrayList(List(values.length()) { values.getInt(it).toString() }))
    }
    var error by rememberSaveable { mutableStateOf("") }
    Column(
        Modifier.fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .imePadding()
            .padding(bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            start,
            {
                start = it
                error = ""
            },
            label = { Text("Начало занятий (ЧЧ:ММ)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Text("Длительность урока", style = MaterialTheme.typography.titleSmall)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (length in listOf(40, 45)) {
                FilterChip(
                    selected = duration == length,
                    onClick = { duration = length },
                    label = { Text("$length мин") },
                )
            }
        }
        Text("Перемены", style = MaterialTheme.typography.titleSmall)
        breaks.forEachIndexed { index, minutes ->
            OutlinedTextField(
                minutes,
                { value ->
                    breaks = ArrayList(breaks).also { it[index] = value }
                    error = ""
                },
                label = { Text("После урока ${index+1}, мин") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            )
        }
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            TextButton(
                enabled = breaks.size < 11,
                onClick = { breaks = ArrayList(breaks + "10") },
            ) {
                Text("Добавить перемену")
            }
            if (breaks.isNotEmpty())
                TextButton(onClick = { breaks = ArrayList(breaks.dropLast(1)) }) {
                    Text("Убрать последнюю")
                }
        }
        val feedback = error.ifEmpty { store.message }
        if (feedback.isNotEmpty()) Text(feedback, color = MaterialTheme.colorScheme.error)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                enabled = !store.busy,
                onClick = {
                    val first =
                        if (start == originalStart.take(5)) originalStart
                        else
                            runCatching {
                                    LocalTime.parse(start).format(DateTimeFormatter.ISO_LOCAL_TIME)
                                }
                                .getOrNull()
                    if (first == null) {
                        error = "Проверьте время начала"
                        return@Button
                    }
                    if (breaks.any { it.toIntOrNull()?.let { v -> v in 0..120 } != true }) {
                        error = "Введите для каждой перемены от 0 до 120 минут"
                        return@Button
                    }
                    error = ""
                    store.edit(
                        obj(
                            "op" to "set_bells",
                            "bells" to
                                obj(
                                    "first_start" to first,
                                    "lesson_minutes" to duration,
                                    "breaks" to JSONArray(breaks.map(String::toInt)),
                                ),
                        ),
                        close,
                    )
                },
            ) {
                Text("Сохранить")
            }
            TextButton(onClick = close) { Text("Назад") }
        }
    }
}
