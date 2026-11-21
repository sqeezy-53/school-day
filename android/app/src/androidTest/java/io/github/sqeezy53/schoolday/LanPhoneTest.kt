package io.github.sqeezy53.schoolday

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.lifecycle.ViewModelProvider
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import java.io.File
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject

class LanPhoneTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    private fun click(text: String) {
        val matcher = hasClickAction() and (hasText(text) or hasAnyDescendant(hasText(text)))
        ui.onAllNodes(matcher, useUnmergedTree = true).onLast()
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
    }

    private fun openSync() {
        click("Настройки")
        val sync = ui.onAllNodes(
            hasClickAction() and hasAnyDescendant(hasText("Синхронизация")),
            useUnmergedTree = true,
        )
        ui.onNode(hasScrollToIndexAction()).performScrollToNode(hasText("Синхронизация"))
        sync.onLast().performSemanticsAction(SemanticsActions.OnClick) { it() }
    }

    @OptIn(ExperimentalTestApi::class)
    private fun screenshot(file: File, dialog: Boolean = false) {
        val node = if (dialog) ui.onNode(isDialog()) else ui.onRoot()
        file.outputStream().use { out ->
            assertTrue(node.captureToImage().asAndroidBitmap().compress(Bitmap.CompressFormat.PNG, 100, out))
        }
    }

    @Test
    fun wifiExchangeWithDesktop() {
        assertTrue("Use the separate QA package", ui.activity.packageName.endsWith(".qa"))
        val work = File(ui.activity.filesDir, "qa-lan").apply { mkdirs() }
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(15_000) { !model.busy && model.view != null }
        if (ui.onAllNodesWithText("Пропустить").fetchSemanticsNodes().isNotEmpty()) click("Пропустить")
        val code = File(work, "invite").readText().trim()
        val marker = File(work, "marker").readText().trim()

        // Verify that a camera-sized QR payload decodes through the production scanner.
        val matrix = QRCodeWriter().encode(code, BarcodeFormat.QR_CODE, 340, 340)
        val pixels = IntArray(matrix.width * matrix.height) { offset ->
            if (matrix[offset % matrix.width, offset / matrix.width]) 0xff000000.toInt() else -1
        }
        val qr = Bitmap.createBitmap(pixels, matrix.width, matrix.height, Bitmap.Config.ARGB_8888)
        try {
            assertTrue("Production QR decoder", decodeInvitation(qr) == code)
        } finally {
            qr.recycle()
        }

        ui.runOnIdle {
            model.edit(obj("op" to "save_lesson", "lesson" to obj(
                "title" to "Урок с телефона $marker",
                "date" to java.time.LocalDate.of(2080, 1, 1).plusDays(marker.toLong() % 2048).toString(),
                "time" to "10:00:00", "end" to "10:30:00",
                "homework" to "Задание с телефона $marker",
            )))
        }
        ui.waitUntil(10_000) {
            !model.busy && model.view!!.getJSONObject("document").list("lessons").any {
                it.value("title") == "Урок с телефона $marker"
            }
        }
        openSync()
        ui.waitUntil(10_000) { !model.busy }
        if (model.lanPaired) {
            click("Отключить устройство")
            click("Отключить")
            ui.waitUntil(10_000) { !model.busy && !model.lanPaired }
        }

        ui.onNode((hasText("Код сопряжения") or hasAnyDescendant(hasText("Код сопряжения")))
            and hasSetTextAction(), useUnmergedTree = true)
            .performSemanticsAction(SemanticsActions.SetText) { it(AnnotatedString(code)) }
        click("Подключить компьютер")
        ui.waitUntil(30_000) { model.lanPrompt?.value("type") == "pair" || !model.busy }
        assertTrue("Pairing state: ${model.lanMessage}", model.lanPrompt?.value("type") == "pair")
        val realCode = model.lanPrompt!!.getString("code")
        File(work, "verification").writeText(realCode)
        ui.waitUntil(30_000) { File(work, "approval").isFile }
        assertTrue("Both endpoints show the same confirmation code",
            File(work, "approval").readText().trim() == realCode)
        click("Код совпадает")
        ui.waitUntil(20_000) { !model.busy && model.lanPaired }
        screenshot(File(work, "connected.png"))

        ui.waitUntil(30_000) { File(work, "sync").isFile }
        click("Синхронизировать")
        ui.waitUntil(30_000) { model.lanPrompt?.value("type") == "review" }
        click("Применить")
        ui.waitUntil(30_000) { !model.busy && model.lanMessage == "Синхронизировано" }
        assertTrue(model.view!!.getJSONObject("document").list("lessons").any {
            it.value("title") == "Урок с ПК $marker" &&
                it.value("homework") == "Задание с ПК $marker"
        })
        screenshot(File(work, "synced.png"))
    }

    @Test
    fun wifiConflictAndDeletionAfterRestart() {
        assertTrue(ui.activity.packageName.endsWith(".qa"))
        val work = File(ui.activity.filesDir, "qa-lan")
        val marker = File(work, "marker").readText().trim()
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(15_000) { !model.busy && model.view != null }
        val pc = model.view!!.getJSONObject("document").list("lessons").first {
            it.value("title") == "Урок с ПК $marker"
        }
        val id = pc.getString("id")
        ui.runOnIdle {
            model.edit(obj("op" to "save_lesson", "id" to id,
                "lesson" to JSONObject(pc.toString()).put("note", "Офлайн-правка Android")))
        }
        ui.waitUntil(10_000) { !model.busy && model.message == "Сохранено" }
        ui.runOnIdle { model.edit(obj("op" to "complete", "id" to id, "linked" to true, "done" to true)) }
        ui.waitUntil(10_000) { !model.busy }
        openSync()
        ui.waitUntil(10_000) { !model.busy && model.lanPaired }
        val file = File(ui.activity.filesDir, "lessons.json")
        val before = file.readBytes()

        fun attempt(trigger: String, button: String, choose: Boolean = false) {
            ui.waitUntil(30_000) { File(work, trigger).isFile }
            click("Синхронизировать")
            ui.waitUntil(30_000) { model.lanPrompt?.value("type") == "review" }
            if (choose) {
                assertEquals("Local", model.lanPrompt!!.getJSONArray("choices").getString(0))
                ui.onNodeWithText("Версия с Android", useUnmergedTree = true).performScrollTo()
                click("Версия с Android")
                screenshot(File(work, "conflict-choice.png"), dialog = true)
            }
            click(button)
            ui.waitUntil(30_000) { !model.busy }
        }
        attempt("conflict-reject", "Отмена")
        assertTrue("User rejection leaves the file intact", before.contentEquals(file.readBytes()))
        attempt("conflict-accept", "Применить", choose = true)
        assertEquals("Синхронизировано", model.lanMessage)
        val merged = model.view!!.getJSONObject("document").list("lessons")
        val retained = merged.first { it.value("title") == "Урок с ПК $marker" }
        val deletedTask = merged.first { it.value("title") == "Урок с телефона $marker" }
        assertEquals("Офлайн-правка Android", retained.value("note"))
        assertTrue(retained.getBoolean("homework_completed"))
        assertEquals("", deletedTask.value("homework"))
        assertTrue(deletedTask.isNull("homework_id"))
        val after = file.readBytes()
        ui.waitUntil(30_000) { File(work, "conflict-repeat").isFile }
        click("Синхронизировать")
        ui.waitUntil(30_000) { !model.busy }
        assertEquals("Все изменения синхронизированы", model.lanMessage)
        assertNull(model.lanPrompt)
        assertTrue("Idempotent real Wi-Fi sync", after.contentEquals(file.readBytes()))
        screenshot(File(work, "verified-conflict.png"))
    }

    @Test
    fun wifiAutomaticWithoutConfirmation() {
        assertTrue(ui.activity.packageName.endsWith(".qa"))
        val work = File(ui.activity.filesDir, "qa-lan")
        val marker = File(work, "marker").readText().trim()
        val round = File(work, "auto-round").readText().trim()
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(10_000) { !model.busy && model.view != null }
        openSync()
        ui.waitUntil(10_000) { !model.busy && model.lanPaired }
        ui.runOnIdle {
            model.changeLanMode(LanMode.Automatic)
            model.edit(obj("op" to "save_task",
                "title" to "Автоматическое ДЗ $marker $round",
                "body" to "С телефона",
            ))
        }
        fun received(note: String): Boolean =
            model.view?.optJSONObject("document")?.list("lessons")?.any {
                it.value("title") == "Урок с ПК $marker" && it.value("note") == note
            } == true
        ui.waitUntil(40_000) { received("Автоматическая правка с ПК $round") }
        assertNull(model.lanPrompt)
        screenshot(File(work, "auto-synced.png"))

        ui.runOnIdle { model.changeLanBackground(true) }
        File(work, "auto-ready").writeText("1")
        ui.waitUntil(25_000) { File(work, "auto-go-background").isFile }
        val activity = ui.activity
        ui.runOnIdle { assertTrue(activity.moveTaskToBack(true)) }
        val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(30)
        while (LanRuntime.foreground && System.nanoTime() < deadline) Thread.sleep(100)
        assertFalse("QA Activity stopped in the background", LanRuntime.foreground)
        File(work, "auto-background").writeText("1")
        while (!File(work, "auto-finished").isFile && System.nanoTime() < deadline) Thread.sleep(100)
        assertTrue("Background Android job finished", File(work, "auto-finished").exists())
    }

    @Test
    fun wifiLiveCameraLifecycle() {
        assertTrue(ui.activity.packageName.endsWith(".qa"))
        val work = File(ui.activity.filesDir, "qa-lan")
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(10_000) { !model.busy && model.view != null }
        openSync()
        ui.waitUntil(10_000) { !model.busy && !model.lanPaired }
        click("Сканировать QR")
        ui.waitUntil(10_000) { ui.onAllNodes(isDialog()).fetchSemanticsNodes().isNotEmpty() }
        File(work, "camera-visible").writeText("1")
        ui.waitUntil(25_000) { File(work, "camera-close").isFile }
        click("Закрыть")
        ui.waitUntil(10_000) { ui.onAllNodes(isDialog()).fetchSemanticsNodes().isEmpty() }
    }
}
