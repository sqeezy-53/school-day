package io.github.sqeezy53.schoolday

import android.app.Application
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.AtomicFile
import androidx.compose.runtime.*
import androidx.lifecycle.AndroidViewModel
import java.io.*
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import org.json.JSONObject

internal object Core {
    init {
        System.loadLibrary("school_day_android")
    }

    external fun execute(document: ByteArray, command: ByteArray): ByteArray
    external fun lanBegin(): Long
    external fun lanInviteValid(code: String): Boolean
    external fun lanEnd(operation: Long)
    external fun lanCancel(operation: Long)
    external fun lanCommand(
        directory: String,
        action: String,
        payload: String,
        operation: Long,
        observer: LanObserver,
    ): String
}

internal fun interface LanObserver {
    fun respond(event: String): String?
}

private class LanRun(val operation: Long, val automatic: Boolean) {
    @Volatile var stopped = false
    @Volatile var answer: CompletableFuture<String?>? = null

    fun cancel() {
        stopped = true
        answer?.complete(null)
        Core.lanCancel(operation)
    }
}

internal fun obj(vararg pairs: Pair<String, Any?>) =
    JSONObject().apply { pairs.forEach { (k, v) -> put(k, v ?: JSONObject.NULL) } }

internal fun call(doc: ByteArray, cmd: JSONObject): JSONObject {
    val result = JSONObject(Core.execute(doc, cmd.toString().toByteArray()).decodeToString())
    if (result.has("error")) throw IOException(result.getString("error"))
    return result
}

private const val LIMIT = 32 * 1024 * 1024

private fun InputStream.bounded(): ByteArray = use { input ->
    val bytes = ByteArrayOutputStream()
    val buffer = ByteArray(8192)
    while (true) {
        val n = input.read(buffer)
        if (n < 0) break
        if (bytes.size() + n > LIMIT) throw IOException("Файл превышает 32 МиБ")
        bytes.write(buffer, 0, n)
    }
    bytes.toByteArray()
}

private fun AtomicFile.store(bytes: ByteArray) {
    val target = baseFile.toPath()
    val temporary = Files.createTempFile(target.parent, ".school-day-", ".tmp")
    try {
        FileOutputStream(temporary.toFile()).use { out ->
            out.write(bytes)
            out.fd.sync()
        }
        Files.move(
            temporary,
            target,
            StandardCopyOption.ATOMIC_MOVE,
            StandardCopyOption.REPLACE_EXISTING,
        )
    } finally {
        Files.deleteIfExists(temporary)
    }
}

class Store(val app: Application) : AndroidViewModel(app) {
    private val worker = LanRuntime.worker
    private val main = Handler(Looper.getMainLooper())
    private val file = AtomicFile(File(app.filesDir, "lessons.json"))
    private var bytes = "[]".toByteArray()
    private var cleanupCheckpoint: ByteArray? = null
    var canUndoCleanup by mutableStateOf(false)
        private set

    var canRestoreBackup by mutableStateOf(false)
        private set

    var view by mutableStateOf<JSONObject?>(null)
        private set

    var preview by mutableStateOf<JSONObject?>(null)
        private set

    var busy by mutableStateOf(false)
        private set

    var message by mutableStateOf("")
        private set

    private val prefs = app.getSharedPreferences("display", 0)
    val deviceId: String =
        prefs.getString("device", null)
            ?: java.util.UUID.randomUUID().toString().also {
                prefs.edit().putString("device", it).apply()
            }
    var dark by mutableStateOf(prefs.getBoolean("dark", false))
        private set

    var rooms by mutableStateOf(prefs.getBoolean("rooms", true))
        private set

    var systemColors by mutableStateOf(prefs.getBoolean("system_colors", false))
        private set

    fun useSystemColors(value: Boolean) {
        prefs.edit().putBoolean("system_colors", value).apply()
        systemColors = value
    }

    val hasSeenTour = prefs.getBoolean("tour_seen", false)

    fun rememberTour() {
        prefs.edit().putBoolean("tour_seen", true).apply()
    }

    fun clearFeedback() {
        message = ""
    }

    fun theme(value: Boolean) {
        prefs.edit().putBoolean("dark", value).apply()
        dark = value
    }

    fun showRooms(value: Boolean) {
        prefs.edit().putBoolean("rooms", value).apply()
        rooms = value
    }

    init {
        load()
    }

    private fun work(block: () -> Unit) {
        if (busy) return
        lanRun?.takeIf { it.automatic }?.cancel()
        busy = true
        message = ""
        worker.execute {
            try {
                block()
            } catch (e: Exception) {
                main.post { message = e.localizedMessage ?: "Не удалось выполнить действие" }
            } finally {
                main.post { busy = false }
            }
        }
    }

    private fun newestBackup() =
        app.filesDir
            .listFiles()
            ?.filter {
                it.isFile &&
                    it.extension == "json" &&
                    listOf("before-migration-", "before-import-", "manual-backup-").any { prefix ->
                        it.name.startsWith(prefix)
                    }
            }
            ?.maxByOrNull { it.lastModified() }

    private fun show(result: JSONObject, info: String = "") {
        val canUndo = cleanupCheckpoint != null
        val hasBackup = newestBackup() != null
        main.post {
            view = result
            message = info
            canUndoCleanup = canUndo
            canRestoreBackup = hasBackup
        }
    }

    private fun currentFile(): ByteArray? =
        try {
            file.openRead().bounded()
        } catch (e: FileNotFoundException) {
            if (file.baseFile.exists() || File(file.baseFile.path + ".bak").exists()) throw e
            null
        }

    private fun lanCommand(action: String, code: String = "", id: Long = 0,
                           observer: LanObserver = LanObserver { null }): JSONObject {
        val result = JSONObject(Core.lanCommand(app.filesDir.path, action, code, id, observer))
        if (result.has("error")) throw LanFailure(result.getString("error"))
        return result
    }

    var lanPaired by mutableStateOf(false)
        private set
    var lanLastSync by mutableStateOf<Long?>(null)
        private set
    var lanMessage by mutableStateOf("")
        private set
    var lanPrompt by mutableStateOf<JSONObject?>(null)
        private set
    private var lanRun: LanRun? = null

    fun answerLan(answer: String? = null) {
        lanRun?.answer?.complete(answer)
        lanPrompt = null
    }

    fun cancelLan() {
        lanRun?.cancel()
        lanPrompt = null
    }

    var lanMode by mutableStateOf(LanMode.read(prefs))
        private set
    var lanBackground by mutableStateOf(prefs.getBoolean("lan_background", false))
        private set
    var lanQueued by mutableStateOf(false)
        private set
    private var retryDelay = 2_000L
    private var needsReview = false
    private val automaticTick = Runnable { tryAutomatic() }

    fun changeLanMode(mode: LanMode) {
        prefs.edit().putString("lan_mode", mode.name).apply()
        lanMode = mode
        if (mode != LanMode.Automatic) cancelLan()
        scheduleBackground()
        scheduleAutomatic(0)
    }

    fun changeLanBackground(value: Boolean) {
        lanBackground = value
        prefs.edit().putBoolean("lan_background", value).apply()
        scheduleBackground()
    }

    private fun scheduleBackground() {
        if (!LanBackground.update(app, lanPaired && lanMode == LanMode.Automatic && lanBackground)) {
            lanMessage = "Система не смогла запланировать фоновый обмен"
        }
    }

    fun setForeground(visible: Boolean) {
        LanRuntime.foreground = visible
        if (visible) {
            LanRuntime.cancelBackground()
            if (!busy) load()
            retryDelay = 2_000L
            scheduleAutomatic(1_000)
        } else {
            main.removeCallbacks(automaticTick)
            cancelLan()
        }
    }

    private fun scheduleAutomatic(delay: Long) {
        main.removeCallbacks(automaticTick)
        if (LanRuntime.foreground && lanMode == LanMode.Automatic) {
            main.postDelayed(automaticTick, delay)
        }
    }

    private fun tryAutomatic() {
        if (!LanRuntime.foreground || lanMode != LanMode.Automatic) return
        if (busy || lanRun != null) {
            scheduleAutomatic(1_000)
            return
        }
        if (!lanPaired) {
            sync("info")
            return
        }
        val run = LanRun(Core.lanBegin(), automatic = true)
        lanRun = run
        worker.execute { runLan("auto", "", run) }
    }

    fun sync(action: String, code: String = "") {
        if (busy || (action == "sync" && lanMode == LanMode.Off)) return
        if (action == "info" && lanRun != null) return
        lanRun?.takeIf { it.automatic }?.cancel()
        val run = LanRun(Core.lanBegin(), automatic = false)
        lanRun = run
        if (action != "info") lanMessage = "Ожидаем компьютер…"
        work { runLan(action, code, run) }
    }

    private fun runLan(action: String, code: String, run: LanRun) {
        try {
            val reply = runCatching {
                lanCommand(action, code, run.operation) { data ->
                    if (run.stopped) return@lanCommand null
                    val response = CompletableFuture<String?>()
                    run.answer = response
                    main.post { if (lanRun == run && !run.stopped) lanPrompt = JSONObject(data) }
                    try {
                        if (run.stopped) null else response.get(105, TimeUnit.SECONDS)
                    } catch (_: Exception) {
                        null
                    } finally {
                        run.answer = null
                    }
                }
            }
            val outcome = reply.getOrNull()?.value("result")
            if (action in setOf("sync", "auto") && outcome !in setOf("unchanged", "review", "cancelled")) {
                runCatching { readDocument(onlyIfChanged = true) }.onFailure {
                    main.post { if (lanRun == run) lanMessage = "Откройте расписание повторно" }
                }
            }
            val latest = reply.getOrNull() ?: if (!run.automatic) {
                runCatching { lanCommand("info") }.getOrNull()
            } else null
            main.post {
                if (lanRun != run) return@post
                latest?.let {
                    lanPaired = it.optBoolean("paired")
                    lanLastSync = it.takeUnless { it.isNull("last_sync") }?.optLong("last_sync")
                    lanQueued = it.optBoolean("queued")
                }
                lanRun = null
                lanPrompt = null
                if (action == "pair" || action == "revoke" || action == "info") scheduleBackground()
                if (action == "revoke" || outcome in setOf("applied", "unchanged")) needsReview = false
                if (outcome == "review") needsReview = true
                if (!run.stopped && reply.isSuccess && (action != "info")) {
                    lanMessage = latest?.value("message").orEmpty()
                }
                if (!run.stopped && reply.isFailure) {
                    lanMessage = if (run.automatic) "Ожидается подключение компьютера"
                    else (reply.exceptionOrNull() as? LanFailure)?.message.orEmpty()
                        .ifBlank { "Не удалось завершить обмен. Изменения сохранены" }
                }
                if (run.stopped && !run.automatic) lanMessage = "Обмен отменён"
                if (run.automatic && !run.stopped && (reply.isFailure || outcome == "receipt")) {
                    scheduleAutomatic(retryDelay)
                    retryDelay = (retryDelay * 2).coerceAtMost(120_000)
                } else {
                    retryDelay = 2_000
                    if (lanPaired) scheduleAutomatic(
                        when {
                            run.stopped -> 1_000
                            needsReview -> 120_000
                            else -> 20_000
                        }
                    )
                }
            }
        } finally {
            Core.lanEnd(run.operation)
        }
    }

    fun load() = work { readDocument() }

    private fun readDocument(onlyIfChanged: Boolean = false) {
        val recovery = runCatching { lanCommand("recover") }.exceptionOrNull()
        val original = currentFile()
        val raw = original ?: "[]".toByteArray()
        val result = call(raw, obj("op" to "open"))
        val next = result.getJSONObject("document").toString().toByteArray()
        val migrated = original != null && result.getBoolean("migrated")
        if (migrated) {
            AtomicFile(File(app.filesDir, "before-migration-${java.util.UUID.randomUUID()}.json"))
                .store(raw)
            file.store(next)
        }
        if (onlyIfChanged && next.contentEquals(bytes)) return
        bytes = next
        cleanupCheckpoint = null
        show(result, recovery?.localizedMessage
            ?: if (migrated) "Формат обновлён. Исходная копия сохранена." else "")
    }

    fun edit(cmd: JSONObject, saved: () -> Unit = {}) = work {
        check(view != null) { "Сначала откройте расписание" }
        val previous = bytes
        val result = call(previous, cmd)
        val cleanup = cmd.optString("op") == "clear_completed"
        storeDocument(result)
        cleanupCheckpoint = if (cleanup) previous else null
        show(result, if (cleanup) "Выполненные задания удалены" else "Сохранено")
        main.post(saved)
    }

    fun undoCleanup() = work {
        val checkpoint = cleanupCheckpoint ?: return@work
        val restored = call(checkpoint, obj("op" to "open"))
        storeDocument(restored)
        cleanupCheckpoint = null
        show(restored, "Выполненные задания восстановлены")
    }

    fun backup() = work {
        check(view != null) { "Сначала откройте расписание" }
        AtomicFile(File(app.filesDir, "manual-backup-${java.util.UUID.randomUUID()}.json"))
            .store(bytes)
        main.post {
            canRestoreBackup = true
            message = "Резервная копия создана"
        }
    }

    fun inspectBackup() = work {
        val backup = newestBackup() ?: throw IOException("Резервных копий пока нет")
        val result = call(backup.inputStream().bounded(), obj("op" to "open"))
        main.post { preview = result }
    }

    private fun storeDocument(result: JSONObject) {
        lanCommand("guard")
        val encoded = result.getJSONObject("document").toString().toByteArray()
        file.store(encoded)
        bytes = encoded
        main.post {
            lanQueued = lanPaired
            // Debounce local writes. A blocked conflict still needs explicit review.
            scheduleAutomatic(900)
        }
    }

    fun inspect(uri: Uri) = work {
        val input =
            app.contentResolver.openInputStream(uri) ?: throw IOException("Не удалось открыть файл")
        val result = call(input.bounded(), obj("op" to "open"))
        main.post { preview = result }
    }

    fun cancelImport() {
        preview = null
    }

    fun importData() {
        val next = preview ?: return
        work {
            val protected = currentFile() ?: bytes
            AtomicFile(File(app.filesDir, "before-import-${java.util.UUID.randomUUID()}.json"))
                .store(protected)
            storeDocument(next)
            cleanupCheckpoint = null
            main.post { preview = null }
            show(next, "Данные восстановлены")
        }
    }

    fun export(uri: Uri) = work {
        check(view != null) { "Нет доступных данных" }
        (app.contentResolver.openOutputStream(uri, "wt")
                ?: throw IOException("Не удалось создать файл"))
            .use { it.write(bytes) }
        main.post { message = "Копия экспортирована" }
    }

    override fun onCleared() {
        main.removeCallbacks(automaticTick)
        cancelLan()
    }
}
