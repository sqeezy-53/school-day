package io.github.sqeezy53.schoolday

import android.animation.ValueAnimator
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.LocalTime
import org.json.JSONObject

class MainActivity : ComponentActivity() {
    private val store by viewModels<Store>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val animate = savedInstanceState == null && ValueAnimator.areAnimatorsEnabled()
        if (Build.VERSION.SDK_INT >= 31) {
            splashScreen.setOnExitAnimationListener { it.remove() }
        }
        setContent {
            val dark = store.dark
            SideEffect {
                enableEdgeToEdge(
                    statusBarStyle = SystemBarStyle.auto(0, 0) { dark },
                    navigationBarStyle = SystemBarStyle.auto(0, 0) { dark },
                )
            }
            val context = LocalContext.current
            val scheme =
                when {
                    store.systemColors && Build.VERSION.SDK_INT >= 31 ->
                        if (dark) dynamicDarkColorScheme(context)
                        else dynamicLightColorScheme(context)
                    dark -> nightColors
                    else -> dayColors
                }
            MaterialTheme(colorScheme = scheme) {
                if (animate) StartupScene(ready = !store.busy) { App(store) }
                else App(store)
            }
        }
    }
    override fun onStart() {
        super.onStart()
        store.setForeground(true)
    }

    override fun onStop() {
        store.setForeground(false)
        super.onStop()
    }

}

private val dayColors =
    lightColorScheme(
        primary = Color(0xFF40558A),
        onPrimary = Color.White,
        primaryContainer = Color(0xFFDDE5FF),
        onPrimaryContainer = Color(0xFF162644),
        secondary = Color(0xFF535F76),
        secondaryContainer = Color(0xFFE3E8F4),
        onSecondaryContainer = Color(0xFF2D384E),
        background = Color(0xFFF5F6FA),
        surface = Color(0xFFF5F6FA),
        surfaceContainer = Color(0xFFE9EBF2),
        surfaceContainerLow = Color(0xFFEFF1F7),
        surfaceContainerHigh = Color(0xFFE0E4EE),
        outlineVariant = Color(0xFFC5CAD8),
    )
private val nightColors =
    darkColorScheme(
        primary = Color(0xFFAFC1F1),
        onPrimary = Color(0xFF192D53),
        primaryContainer = Color(0xFF32456D),
        onPrimaryContainer = Color(0xFFE2E9FF),
        secondary = Color(0xFFBBC5DC),
        secondaryContainer = Color(0xFF364158),
        onSecondaryContainer = Color(0xFFDFE7FC),
        background = Color(0xFF17191F),
        surface = Color(0xFF17191F),
        surfaceContainer = Color(0xFF242731),
        surfaceContainerLow = Color(0xFF20232C),
        surfaceContainerHigh = Color(0xFF2C303C),
        outlineVariant = Color(0xFF484D5D),
    )

internal fun JSONObject.list(key: String): List<JSONObject> {
    val a = optJSONArray(key) ?: return emptyList()
    return List(a.length()) { a.getJSONObject(it) }
}

internal fun JSONObject.value(name: String) = if (isNull(name)) "" else optString(name)

@Composable
private fun App(store: Store) {
    var clearCount by rememberSaveable { mutableIntStateOf(0) }

    var tour by rememberSaveable { mutableIntStateOf(if (store.hasSeenTour) -1 else 0) }
    var tourReturnTab by rememberSaveable { mutableIntStateOf(0) }
    var page by rememberSaveable { mutableIntStateOf(0) }
    var date by rememberSaveable { mutableStateOf(LocalDate.now().toString()) }
    var form by rememberSaveable { mutableStateOf("") }
    var editing by rememberSaveable { mutableStateOf("{}") }
    var deletion by rememberSaveable { mutableStateOf("") }
    fun open(kind: String, item: JSONObject = JSONObject()) {
        store.clearFeedback()
        editing = item.toString()
        form = kind
    }
    val importing =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) {
            it?.let(store::inspect)
        }
    val exporting =
        rememberLauncherForActivityResult(
            ActivityResultContracts.CreateDocument("application/json")
        ) {
            it?.let(store::export)
        }

    val doc = store.view?.optJSONObject("document")
    val day = doc?.optJSONObject("days")?.optJSONObject(date) ?: JSONObject()
    val lessons =
        remember(doc, date) {
            doc?.list("lessons").orEmpty().filter { it.getString("date") == date }
        }
    val view = store.view
    val tasks = remember(view) { view?.list("tasks").orEmpty() }
    fun closeTour() {
        tour = -1
        page = tourReturnTab
        store.rememberTour()
    }
    BackHandler(tour >= 0 || form.isNotEmpty() || page != 0) {
        when {
            tour >= 0 -> closeTour()
            form.isNotEmpty() -> {
                if (form == "sync") store.cancelLan()
                form = ""
            }
            else -> page = 0
        }
    }
    LaunchedEffect(tour) { if (tour >= 0) page = tourTabs[tour] }
    val tabs = listOf("Расписание", "ДЗ", "Настройки")
    val icons = listOf(R.drawable.ic_calendar, R.drawable.ic_tasks, R.drawable.ic_settings)
    val snackbars = remember { SnackbarHostState() }
    LaunchedEffect(store.message, store.canUndoCleanup, form) {
        if (store.message.isNotBlank() && form.isEmpty()) {
            val undoable = store.canUndoCleanup && store.message == "Выполненные задания удалены"
            val result =
                snackbars.showSnackbar(
                    store.message,
                    actionLabel = if (undoable) "Вернуть" else null,
                    duration = if (undoable) SnackbarDuration.Long else SnackbarDuration.Short,
                )
            if (result == SnackbarResult.ActionPerformed) store.undoCleanup()
        }
    }
    fun addLesson() {
        val slot =
            view?.list("bells")?.firstOrNull { bell ->
                val start = LocalTime.parse(bell.getString("time"))
                val end = LocalTime.parse(bell.getString("end"))
                lessons.none { lesson ->
                    LocalTime.parse(lesson.getString("time")) < end &&
                        start < LocalTime.parse(lesson.getString("end"))
                }
            }
        open("lesson", slot ?: JSONObject())
    }
    Scaffold(
        topBar = {
            SchoolHeader(
                if (form.isNotEmpty()) editorTitle(form, JSONObject(editing))
                else if (page == 0) "Школьный день" else tabs[page],
                branded = page == 0 && form.isEmpty(),
                back =
                    if (form.isEmpty()) null
                    else {
                        {
                            if (form == "sync") store.cancelLan()
                            form = ""
                        }
                    },
            )
        },
        snackbarHost = { SnackbarHost(snackbars) },
        bottomBar = {
            if (form.isEmpty())
                NavigationBar {
                    tabs.forEachIndexed { i, title ->
                        NavigationBarItem(
                            selected = page == i,
                            onClick = {
                                if (tour >= 0) closeTour()
                                page = i
                            },
                            icon = { SchoolIcon(icons[i], null) },
                            label = { Text(title) },
                        )
                    }
                }
        },
        floatingActionButton = {
            if (form.isEmpty() && page < 2 && doc != null) {
                ExtendedFloatingActionButton(
                    modifier =
                        if (tour == 2) Modifier.guideOutline(MaterialTheme.shapes.large)
                        else Modifier,
                    text = { Text(if (page == 0) "Урок" else "Задание") },
                    icon = { SchoolIcon(R.drawable.ic_add, null) },
                    onClick = { if (page == 0) addLesson() else open("task") },
                )
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            if (tour >= 0)
                MobileTour(
                    tour,
                    { if (tour == tourTabs.lastIndex) closeTour() else tour++ },
                    ::closeTour,
                )
            if (store.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            val content = Modifier.fillMaxSize().padding(horizontal = 16.dp)
            when {
                form.isNotEmpty() ->
                    Column(content) {
                        key(form, editing) {
                            when (form) {
                                "bells" -> BellEditor(JSONObject(editing), store) { form = "" }
                                "sync" -> SyncScreen(store)
                                else -> Editor(form, JSONObject(editing), date, store) { form = "" }
                            }
                        }
                    }
                page == 2 ->
                    PreferencesScreen(
                        content,
                        store,
                        canExport = doc != null && !store.busy,
                        showDataTip = tour == 3,
                        onGuide = {
                            tourReturnTab = page
                            tour = 0
                        },
                        onImport = {
                            importing.launch(
                                arrayOf(
                                    "application/json",
                                    "text/plain",
                                    "application/octet-stream",
                                )
                            )
                        },
                        onExport = { exporting.launch("school-day.json") },
                        onBells = { view?.optJSONObject("bells_config")?.let { open("bells", it) } },
                        onSync = { open("sync") },
                    )
                doc == null ->
                    Column(content) {
                        EmptyState(R.drawable.ic_calendar, "Расписание не открыто", "")
                        TextButton(onClick = store::load, enabled = !store.busy) {
                            Text("Повторить")
                        }
                    }
                page == 0 ->
                    ScheduleScreen(
                        content,
                        date,
                        { date = it },
                        day,
                        lessons,
                        view?.optJSONObject("short_ends"),
                        store.rooms,
                        tour,
                        onDay = { open("day", day) },
                        onEdit = { open("lesson", it) },
                        onDelete = {
                            deletion =
                                obj("op" to "delete_lesson", "id" to it.getString("id")).toString()
                        },
                    )
                else ->
                    HomeworkScreen(
                        content,
                        tasks,
                        !store.busy,
                        onClear = { clearCount = it },
                        onEdit = { open("task", it) },
                        onDone = { item, done ->
                            store.edit(taskCommand("complete", item).put("done", done))
                        },
                        onDelete = { deletion = taskCommand("delete_task", it).toString() },
                    )
            }
        }
    }
    if (clearCount > 0)
        AlertDialog(
            onDismissRequest = { clearCount = 0 },
            title = { Text("Очистить выполненные задания?") },
            text = { Text("Будет удалено заданий: $clearCount.") },
            confirmButton = {
                TextButton(
                    enabled = !store.busy,
                    onClick = {
                        store.edit(obj("op" to "clear_completed", "expected" to clearCount)) {
                            clearCount = 0
                        }
                    },
                ) {
                    Text("Удалить")
                }
            },
            dismissButton = { TextButton(onClick = { clearCount = 0 }) { Text("Отмена") } },
        )
    if (deletion != "")
        AlertDialog(
            onDismissRequest = { deletion = "" },
            title = { Text("Удалить запись?") },
            text = {
                Text(
                    if (JSONObject(deletion).optString("op") == "delete_lesson")
                        "При удалении урока будет удалено и привязанное ДЗ."
                    else "Задание будет удалено из списка."
                )
            },
            confirmButton = {
                TextButton(
                    enabled = !store.busy,
                    onClick = { store.edit(JSONObject(deletion)) { deletion = "" } },
                ) {
                    Text("Удалить")
                }
            },
            dismissButton = { TextButton(onClick = { deletion = "" }) { Text("Отмена") } },
        )
    store.preview?.let { p ->
        AlertDialog(
            onDismissRequest = store::cancelImport,
            title = { Text("Заменить расписание?") },
            text = {
                Text(
                    "Уроков: ${p.getJSONObject("document").list("lessons").size}. Заданий: ${p.list("tasks").size}. Текущие данные будут сохранены в копии."
                )
            },
            confirmButton = {
                TextButton(enabled = !store.busy, onClick = store::importData) { Text("Заменить") }
            },
            dismissButton = { TextButton(onClick = store::cancelImport) { Text("Отмена") } },
        )
    }
}

private val tourTabs = intArrayOf(0, 0, 1, 2)
private val tourCards =
    listOf(
        "Выберите дату" to "Нажмите на дату, чтобы открыть календарь.",
        "Настройте день" to "В меню «День» можно включить дистант или сократить уроки.",
        "Добавьте задание" to "Нажмите «Задание», чтобы записать домашнюю работу.",
        "Перенесите расписание" to
            "В разделе «Данные» можно импортировать или экспортировать расписание.",
    )

private fun taskCommand(action: String, item: JSONObject) =
    obj("op" to action, "id" to item.getString("id"), "linked" to item.getBoolean("linked"))

private fun editorTitle(kind: String, item: JSONObject) =
    when (kind) {
        "day" -> "Настройки дня"
        "bells" -> "Расписание звонков"
        "sync" -> "Синхронизация"
        "lesson" -> if (item.has("id")) "Изменить урок" else "Новый урок"
        else -> if (item.has("id")) "Изменить задание" else "Новое задание"
    }

@Composable
private fun MobileTour(step: Int, next: () -> Unit, close: () -> Unit) {
    val (title, body) = tourCards[step]
    ElevatedCard(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        colors =
            CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer
            ),
    ) {
        Column(Modifier.padding(start = 12.dp, end = 10.dp, top = 8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(title, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
                Text("${step+1} / ${tourCards.size}", style = MaterialTheme.typography.labelSmall)
            }
            Text(body, style = MaterialTheme.typography.bodySmall)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                TextButton(onClick = close) { Text("Пропустить") }
                TextButton(onClick = next) {
                    Text(if (step == tourCards.lastIndex) "Готово" else "Далее")
                }
            }
        }
    }
}

@Composable
private fun Editor(
    kind: String,
    item: JSONObject,
    selected: String,
    store: Store,
    close: () -> Unit,
) {
    val opts = item.optJSONObject("options") ?: JSONObject()
    var title by rememberSaveable { mutableStateOf(item.optString("title")) }
    var date by rememberSaveable { mutableStateOf(item.value("date").ifBlank { selected }) }
    var start by rememberSaveable { mutableStateOf(item.optString("time", "08:00").take(5)) }
    var end by rememberSaveable { mutableStateOf(item.optString("end", "08:40").take(5)) }
    var room by rememberSaveable { mutableStateOf(item.optString("room")) }
    var note by rememberSaveable { mutableStateOf(item.optString("note")) }
    var replacement by rememberSaveable { mutableStateOf(opts.optString("replacement")) }
    var work by rememberSaveable { mutableStateOf(opts.optString("work", "regular")) }
    var cancelled by rememberSaveable { mutableStateOf(opts.optBoolean("cancelled")) }
    var body by rememberSaveable { mutableStateOf(item.optString("body")) }
    var dated by rememberSaveable { mutableStateOf(item.value("date").isNotBlank()) }
    var remote by rememberSaveable { mutableStateOf(item.optBoolean("remote")) }
    var short by rememberSaveable { mutableStateOf(item.has("short_minutes")) }
    var mins by rememberSaveable { mutableStateOf(item.optInt("short_minutes", 30).toString()) }
    var expanded by rememberSaveable { mutableStateOf(opts.length() > 0 || note.isNotBlank()) }
    var error by rememberSaveable { mutableStateOf("") }
    val linked = item.optBoolean("linked")
    fun lessonTime(field: String, value: String): String {
        val previous = item.value(field)
        if (previous.isNotEmpty() && value == previous.take(5)) return previous
        return LocalTime.parse(value).format(java.time.format.DateTimeFormatter.ISO_LOCAL_TIME)
    }
    Column(
        Modifier.verticalScroll(rememberScrollState()).imePadding(),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (kind == "day") {
            Text(displayDate(date), style = MaterialTheme.typography.titleMedium)
            Toggle("Дистант", remote) { remote = it }
            Toggle("Сокращённый день", short) { short = it }
            if (short) Field("Минут на урок", mins) { mins = it }
        } else {
            if (!linked) Field("Предмет", title) { title = it } else Text(title)
            if (kind == "lesson") {
                DateButton(date) { date = it }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Field("Начало", start, Modifier.weight(1f), singleLine = true) { start = it }
                    Field("Конец", end, Modifier.weight(1f), singleLine = true) { end = it }
                }

                Field("Кабинет", room) { room = it }
                TextButton(onClick = { expanded = !expanded }) {
                    Text(if (expanded) "Свернуть отметки" else "Заметка и отметки")
                }
                if (expanded) {
                    Field("Заметка", note) { note = it }
                    Field("Замена предмета", replacement) { replacement = it }
                    Toggle("Урок отменён", cancelled) { cancelled = it }
                    listOf(
                            "regular" to "Обычный урок",
                            "test" to "Контрольная",
                            "independent" to "Самостоятельная",
                        )
                        .forEach { (key, label) ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                RadioButton(work == key, { work = key })
                                Text(label)
                            }
                        }
                }
            } else {
                if (!linked) {
                    Toggle("Указать дату", dated) { dated = it }
                    if (dated) DateButton(date) { date = it }
                }
                Field("Текст задания", body) { body = it }
            }
        }
        val feedback = error.ifBlank { store.message }
        if (feedback.isNotBlank()) Text(feedback, color = MaterialTheme.colorScheme.error)

        Row {
            Button(
                enabled = !store.busy,
                onClick = {
                    val id = item.value("id").ifBlank { null }
                    val cmd =
                        try {
                            when (kind) {
                                "day" -> {
                                    val n =
                                        if (short) mins.toInt().also { require(it in 1..1439) }
                                        else null
                                    obj(
                                        "op" to "day",
                                        "date" to date,
                                        "options" to obj("remote" to remote, "short_minutes" to n),
                                    )
                                }
                                "lesson" ->
                                    obj(
                                        "op" to "save_lesson",
                                        "id" to id,
                                        "lesson" to
                                            obj(
                                                "title" to title,
                                                "date" to date,
                                                "time" to lessonTime("time", start),
                                                "end" to lessonTime("end", end),
                                                "room" to room,
                                                "note" to note,
                                                "options" to
                                                    obj(
                                                        "work" to work,
                                                        "cancelled" to cancelled,
                                                        "replacement" to replacement,
                                                    ),
                                            ),
                                    )
                                else ->
                                    if (linked)
                                        obj("op" to "linked_text", "id" to id, "body" to body)
                                    else
                                        obj(
                                            "op" to "save_task",
                                            "id" to id,
                                            "title" to title,
                                            "date" to (if (dated) date else null),
                                            "body" to body,
                                        )
                            }
                        } catch (e: Exception) {
                            error = "Проверьте время или длительность урока"
                            return@Button
                        }
                    store.edit(cmd, close)
                },
            ) {
                Text("Сохранить")
            }
            TextButton(onClick = close) { Text("Назад") }
        }
    }
}

@Composable
private fun Field(
    title: String,
    text: String,
    modifier: Modifier = Modifier.fillMaxWidth(),
    singleLine: Boolean = false,
    set: (String) -> Unit,
) {
    OutlinedTextField(
        value = text,
        onValueChange = set,
        label = { Text(title) },
        modifier = modifier,
        singleLine = singleLine,
    )
}
