package io.github.sqeezy53.schoolday

import android.os.Build
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import org.json.JSONObject

private val russian = Locale.forLanguageTag("ru")
private val dateFormat = DateTimeFormatter.ofPattern("d MMMM", russian)
private val dayFormat = DateTimeFormatter.ofPattern("EEEE", russian)

internal fun displayDate(date: String): String = LocalDate.parse(date).format(dateFormat)

@Composable
internal fun Modifier.guideOutline(shape: Shape): Modifier =
    border(2.dp, MaterialTheme.colorScheme.primary, shape)

@Composable
internal fun SchoolIcon(res: Int, label: String?, modifier: Modifier = Modifier) {
    Icon(painterResource(res), contentDescription = label, modifier = modifier)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SchoolHeader(title: String, branded: Boolean, back: (() -> Unit)?) {
    TopAppBar(
        title = {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (branded)
                    Surface(
                        shape = RoundedCornerShape(8.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    ) {
                        SchoolIcon(
                            R.drawable.school_day_book,
                            null,
                            Modifier.size(35.dp),
                        )
                    }
                Text(
                    title,
                    style = MaterialTheme.typography.headlineSmall,
                )
            }
        },
        navigationIcon = {
            if (back != null) IconButton(onClick = back) { SchoolIcon(R.drawable.ic_back, "Назад") }
        },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DateButton(date: String, onDate: (String) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    TextButton(onClick = { open = true }) {
        SchoolIcon(R.drawable.ic_calendar, null, Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(displayDate(date))
    }
    if (open) {
        val selected = LocalDate.parse(date).atStartOfDay().toInstant(ZoneOffset.UTC).toEpochMilli()
        val state =
            rememberDatePickerState(
                initialSelectedDateMillis = selected,
                initialDisplayedMonthMillis = selected,
            )
        DatePickerDialog(
            onDismissRequest = { open = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        state.selectedDateMillis?.let {
                            onDate(
                                Instant.ofEpochMilli(it)
                                    .atOffset(ZoneOffset.UTC)
                                    .toLocalDate()
                                    .toString()
                            )
                        }
                        open = false
                    }
                ) {
                    Text("Выбрать")
                }
            },
            dismissButton = { TextButton(onClick = { open = false }) { Text("Отмена") } },
        ) {
            DatePicker(state, title = null)
        }
    }
}

@Composable
internal fun Toggle(title: String, value: Boolean, set: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        trailingContent = { Switch(value, null) },
        modifier = Modifier.toggleable(value, role = Role.Switch, onValueChange = set),
        colors =
            ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    )
}

@Composable
internal fun EmptyState(icon: Int, title: String, description: String) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Surface(
            shape = RoundedCornerShape(18.dp),
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ) {
            SchoolIcon(icon, null, Modifier.padding(12.dp).size(28.dp))
        }
        Text(title, style = MaterialTheme.typography.titleMedium)
        if (description.isNotBlank())
            Text(
                description,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
    }
}

@Composable
internal fun ScheduleScreen(
    modifier: Modifier,
    date: String,
    onDate: (String) -> Unit,
    day: JSONObject,
    lessons: List<JSONObject>,
    ends: JSONObject?,
    rooms: Boolean,
    tour: Int,
    onDay: () -> Unit,
    onEdit: (JSONObject) -> Unit,
    onDelete: (JSONObject) -> Unit,
) {
    val current = LocalDate.parse(date)
    val today = LocalDate.now()
    val remote = day.optBoolean("remote")
    val minutes = day.optInt("short_minutes")
    val dateShape = RoundedCornerShape(18.dp)
    LazyColumn(
        modifier,
        contentPadding = PaddingValues(top = 6.dp, bottom = 86.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            Surface(
                color = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                shape = dateShape,
                modifier = if (tour == 0) Modifier.guideOutline(dateShape) else Modifier,
            ) {
                Column(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        IconButton(onClick = { onDate(current.minusDays(1).toString()) }) {
                            SchoolIcon(R.drawable.ic_previous, "Предыдущий день")
                        }
                        Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                            DateButton(date, onDate)
                        }
                        IconButton(onClick = { onDate(current.plusDays(1).toString()) }) {
                            SchoolIcon(R.drawable.ic_next, "Следующий день")
                        }
                    }
                    Text(
                        if (current == today) "Сегодня · ${current.format(dayFormat)}"
                        else current.format(dayFormat),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.Center,
            ) {
                AssistChip(
                    onClick = onDay,
                    modifier =
                        if (tour == 1) Modifier.guideOutline(MaterialTheme.shapes.small)
                        else Modifier,
                    label = { Text(if (remote) "Дистант" else "День") },
                    leadingIcon = { SchoolIcon(R.drawable.ic_day, null, Modifier.size(18.dp)) },
                )
                if (minutes > 0) AssistChip(onClick = onDay, label = { Text("$minutes мин") })
                if (current != today)
                    TextButton(onClick = { onDate(today.toString()) }) { Text("Сегодня") }
            }
        }
        if (lessons.isEmpty())
            item {
                EmptyState(
                    R.drawable.ic_calendar,
                    "На эту дату уроков нет",
                    "Чтобы начать, нажмите «Урок».",
                )
            }
        items(lessons, key = { it.getString("id") }) { lesson ->
            LessonCard(
                lesson,
                ends?.optString(lesson.getString("id")).orEmpty(),
                rooms && !remote,
                { onEdit(lesson) },
                { onDelete(lesson) },
            )
        }
    }
}

@Composable
private fun LessonCard(
    item: JSONObject,
    effectiveEnd: String,
    showRoom: Boolean,
    edit: () -> Unit,
    delete: () -> Unit,
) {
    val opts = item.optJSONObject("options") ?: JSONObject()
    val replacement = opts.optString("replacement")
    val cancelled = opts.optBoolean("cancelled")
    val end = effectiveEnd.ifBlank { item.getString("end") }
    val time = "${item.getString("time").take(5)} - ${end.take(5)}"
    Card(
        onClick = edit,
        shape = RoundedCornerShape(16.dp),
        colors =
            CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 2.dp, top = 8.dp, bottom = 10.dp)
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    time,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
                Text(
                    replacement.ifBlank { item.getString("title") },
                    style = MaterialTheme.typography.titleMedium,
                    textDecoration = if (cancelled) TextDecoration.LineThrough else null,
                )
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    if (cancelled) StatusTag("Отменён", true)
                    if (replacement.isNotBlank()) StatusTag("Замена · ${item.getString("title")}")
                    when (opts.optString("work")) {
                        "test" -> StatusTag("К/Р")
                        "independent" -> StatusTag("С/Р")
                    }
                }
                if (showRoom && item.value("room").isNotBlank())
                    Detail("Кабинет ${item.value("room")}")
                if (item.value("note").isNotBlank()) Detail(item.value("note"))
                if (item.value("homework").isNotBlank())
                    Detail(
                        (if (item.optBoolean("homework_completed")) "ДЗ выполнено: " else "ДЗ: ") +
                            item.value("homework")
                    )
            }
            RecordMenu(edit, delete)
        }
    }
}

@Composable
private fun Detail(value: String) {
    Text(
        value,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun StatusTag(label: String, error: Boolean = false) {
    Surface(
        shape = RoundedCornerShape(7.dp),
        color =
            if (error) MaterialTheme.colorScheme.errorContainer
            else MaterialTheme.colorScheme.secondaryContainer,
        contentColor =
            if (error) MaterialTheme.colorScheme.onErrorContainer
            else MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Text(
            label,
            Modifier.padding(horizontal = 7.dp, vertical = 3.dp),
            style = MaterialTheme.typography.labelMedium,
        )
    }
}

@Composable
private fun RecordMenu(edit: () -> Unit, delete: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) { SchoolIcon(R.drawable.ic_more, "Действия") }
        DropdownMenu(open, { open = false }) {
            DropdownMenuItem(
                text = { Text("Изменить") },
                onClick = {
                    open = false
                    edit()
                },
            )
            DropdownMenuItem(
                text = { Text("Удалить") },
                onClick = {
                    open = false
                    delete()
                },
            )
        }
    }
}

@Composable
internal fun HomeworkScreen(
    modifier: Modifier,
    tasks: List<JSONObject>,
    enabled: Boolean,
    onClear: (Int) -> Unit,
    onEdit: (JSONObject) -> Unit,
    onDone: (JSONObject, Boolean) -> Unit,
    onDelete: (JSONObject) -> Unit,
) {
    val (complete, pending) = remember(tasks) { tasks.partition { it.getBoolean("completed") } }
    LazyColumn(
        modifier,
        contentPadding = PaddingValues(top = 8.dp, bottom = 86.dp),
        verticalArrangement = Arrangement.spacedBy(9.dp),
    ) {
        if (tasks.isEmpty())
            item {
                EmptyState(
                    R.drawable.ic_tasks,
                    "Заданий пока нет",
                    "Нажмите «Задание», чтобы записать домашнюю работу.",
                )
            }
        for ((title, group) in listOf("В работе" to pending, "Выполнено" to complete)) {
            if (group.isEmpty()) continue
            item {
                if (title == "Выполнено")
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { SectionLabel(title) }
                        TextButton(onClick = { onClear(group.size) }, enabled = enabled) {
                            Text("Очистить выполненные")
                        }
                    }
                else SectionLabel(title)
            }
            items(group, key = { it.getString("id") }) { task ->
                val done = task.getBoolean("completed")
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow,
                ) {
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        verticalAlignment = Alignment.Top,
                    ) {
                        Checkbox(done, { onDone(task, it) }, enabled = enabled)
                        Column(
                            Modifier.weight(1f).padding(top = 10.dp, bottom = 9.dp),
                            verticalArrangement = Arrangement.spacedBy(3.dp),
                        ) {
                            Text(
                                task.getString("title"),
                                style = MaterialTheme.typography.titleMedium,
                                textDecoration = if (done) TextDecoration.LineThrough else null,
                            )
                            if (task.value("date").isNotEmpty())
                                Detail(displayDate(task.value("date")))
                            Text(
                                task.getString("body"),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        RecordMenu({ onEdit(task) }, { onDelete(task) })
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionLabel(title: String) {
    Text(
        title,
        Modifier.padding(start = 4.dp, top = 6.dp, bottom = 3.dp),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
    )
}

@Composable
internal fun PreferencesScreen(
    modifier: Modifier,
    store: Store,
    canExport: Boolean,
    showDataTip: Boolean,
    onGuide: () -> Unit,
    onImport: () -> Unit,
    onExport: () -> Unit,
    onBells: () -> Unit,
    onSync: () -> Unit,
) {
    var about by rememberSaveable { mutableStateOf(false) }
    var showDeviceId by rememberSaveable { mutableStateOf(false) }
    val list = rememberLazyListState()
    LaunchedEffect(showDataTip) { if (showDataTip) list.scrollToItem(1) }
    LazyColumn(
        modifier,
        state = list,
        contentPadding = PaddingValues(bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            PreferenceGroup("Внешний вид") {
                Toggle("Тёмная тема", store.dark, store::theme)
                if (Build.VERSION.SDK_INT >= 31) {
                    HorizontalDivider()
                    Toggle("Цвета системы", store.systemColors, store::useSystemColors)
                }
                HorizontalDivider()
                Toggle("Показывать кабинеты", store.rooms, store::showRooms)
                HorizontalDivider()
                PreferenceAction("Расписание звонков", R.drawable.ic_calendar, canExport, onBells)
            }
        }
        item {
            PreferenceGroup("Данные", showDataTip) {
                PreferenceAction(
                    "Импорт расписания и ДЗ",
                    R.drawable.ic_import,
                    !store.busy,
                    onImport,
                )
                HorizontalDivider()
                PreferenceAction(
                    "Экспорт расписания и ДЗ",
                    R.drawable.ic_export,
                    canExport,
                    onExport,
                )
                HorizontalDivider()
                PreferenceAction(
                    "Создать резервную копию",
                    R.drawable.ic_add,
                    canExport,
                    store::backup,
                )
                HorizontalDivider()
                PreferenceAction(
                    "Восстановить последнюю копию",
                    R.drawable.ic_import,
                    store.canRestoreBackup && !store.busy,
                    store::inspectBackup,
                )
            }
        }
        item {
            PreferenceGroup("Устройства") {
                PreferenceAction("Синхронизация", R.drawable.ic_settings, !store.busy, onSync)
            }
        }
        item {
            PreferenceGroup("О приложении") {
                PreferenceAction("Как пользоваться", R.drawable.ic_info, true, onGuide)
                HorizontalDivider()
                ListItem(
                    headlineContent = { Text("School Day") },
                    supportingContent = { Text("${BuildConfig.VERSION_NAME} · Android Alpha") },
                    colors =
                        ListItemDefaults.colors(
                            containerColor = MaterialTheme.colorScheme.surfaceContainerLow
                        ),
                    modifier = Modifier.clickable { about = true },
                )
            }
        }
    }
    if (about)
        AlertDialog(
            onDismissRequest = {
                about = false
                showDeviceId = false
            },
            title = { Text("School Day") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Версия ${BuildConfig.VERSION_NAME} · Android Alpha")
                    if (showDeviceId) Text(store.deviceId)
                    else
                        TextButton(onClick = { showDeviceId = true }) {
                            Text("Идентификатор устройства")
                        }
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        about = false
                        showDeviceId = false
                    }
                ) {
                    Text("Готово")
                }
            },
        )
}

@Composable
private fun PreferenceGroup(
    title: String,
    spotlight: Boolean = false,
    body: @Composable ColumnScope.() -> Unit,
) {
    SectionLabel(title)
    val shape = RoundedCornerShape(16.dp)
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = shape,
        modifier = if (spotlight) Modifier.guideOutline(shape) else Modifier,
    ) {
        Column(content = body)
    }
}

@Composable
private fun PreferenceAction(title: String, icon: Int, enabled: Boolean, action: () -> Unit) {
    ListItem(
        headlineContent = {
            Text(
                title,
                color =
                    if (enabled) LocalContentColor.current
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        leadingContent = { SchoolIcon(icon, null) },
        trailingContent = { SchoolIcon(R.drawable.ic_next, null) },
        colors =
            ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        modifier = Modifier.clickable(enabled = enabled, onClick = action),
    )
}
