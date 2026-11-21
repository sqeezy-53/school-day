package io.github.sqeezy53.schoolday

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.lifecycle.ViewModelProvider
import java.io.File
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class PhoneSmokeTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    private fun click(text: String) {
        val target = hasClickAction() and (hasText(text) or hasAnyDescendant(hasText(text)))
        ui.onAllNodes(target, useUnmergedTree = true).onLast().performSemanticsAction(
            SemanticsActions.OnClick
        ) {
            it()
        }
    }

    private fun editField(label: String, value: String) {
        ui.onNode(
                (hasText(label) or hasAnyDescendant(hasText(label))) and hasSetTextAction(),
                useUnmergedTree = true,
            )
            .performSemanticsAction(SemanticsActions.SetText) { it(AnnotatedString(value)) }
    }

    @OptIn(ExperimentalTestApi::class)
    private fun screenshot(name: String) {
        val directory = File(ui.activity.filesDir, "qa-screenshots").apply { mkdirs() }
        File(directory, "$name.png").outputStream().use {
            assertTrue(
                ui.onRoot()
                    .captureToImage()
                    .asAndroidBitmap()
                    .compress(Bitmap.CompressFormat.PNG, 100, it)
            )
        }
    }

    @Test
    fun releaseScreenshots() {
        assertTrue(ui.activity.packageName.endsWith(".qa"))
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(10_000) { !model.busy && model.view != null }
        if (ui.onAllNodesWithText("Пропустить").fetchSemanticsNodes().isNotEmpty())
            click("Пропустить")
        assertTrue(model.view!!.getJSONObject("document").list("lessons").any { it.value("title") == "Алгебра" })
        screenshot("release-schedule")
        click("ДЗ")
        screenshot("release-homework")
        click("Настройки")
        screenshot("release-settings")
    }

    @Test
    fun isolatedPhoneProfile() {
        assertTrue("Use the separate QA package", ui.activity.packageName.endsWith(".qa"))
        val model = ViewModelProvider(ui.activity)[Store::class.java]
        ui.waitUntil(10000) { !model.busy && model.view != null }
        if (ui.onAllNodesWithText("Пропустить").fetchSemanticsNodes().isNotEmpty())
            click("Пропустить")
        val marker = System.currentTimeMillis().toString().takeLast(5)
        ui.onNodeWithText("Школьный день").assertExists()
        assertTrue(
            ui.activity.getDrawable(R.mipmap.ic_launcher)
                is android.graphics.drawable.AdaptiveIconDrawable
        )
        screenshot("Schedule")

        click("Урок")
        editField("Предмет", "Проверка $marker")
        ui.onNodeWithText("Сохранить", useUnmergedTree = true).performScrollTo()
        click("Сохранить")
        ui.waitUntil(10000) {
            !model.busy &&
                model.view?.optJSONObject("document")?.list("lessons")?.any {
                    it.getString("title") == "Проверка $marker"
                } == true
        }
        click("ДЗ")
        screenshot("Homework")
        click("Задание")
        editField("Предмет", "ДЗ $marker")
        editField("Текст задания", "Тестовое задание")
        ui.onNodeWithText("Сохранить", useUnmergedTree = true).performScrollTo()
        click("Сохранить")
        ui.waitUntil(10000) {
            !model.busy &&
                model.view?.list("tasks")?.any { it.getString("title") == "ДЗ $marker" } == true
        }
        val pending = model.view!!.list("tasks").first { it.getString("title") == "ДЗ $marker" }
        ui.runOnIdle {
            model.edit(
                obj(
                    "op" to "complete",
                    "id" to pending.getString("id"),
                    "linked" to false,
                    "done" to true,
                )
            )
        }
        ui.waitUntil(10000) {
            !model.busy &&
                model.view!!.list("tasks").any {
                    it.getString("id") == pending.getString("id") && it.getBoolean("completed")
                }
        }
        ui.onNode(hasScrollToIndexAction()).performScrollToNode(hasText("Очистить выполненные"))
        click("Очистить выполненные")
        ui.onNodeWithText("Будет удалено заданий:", substring = true).assertExists()
        click("Удалить")
        ui.waitUntil(10000) { !model.busy && model.canUndoCleanup }
        click("Вернуть")
        ui.waitUntil(10000) {
            !model.busy &&
                !model.canUndoCleanup &&
                model.view!!.list("tasks").any { it.getString("id") == pending.getString("id") }
        }
        click("Настройки")
        if (model.dark) click("Тёмная тема")
        ui.waitUntil(10000) { !model.dark }
        screenshot("Settings")
        click("Тёмная тема")
        ui.waitUntil(10000) { model.dark }
        screenshot("Settings-Dark")
        ui.onNodeWithText("Создать резервную копию", useUnmergedTree = true).performScrollTo()
        click("Создать резервную копию")
        ui.waitUntil(10000) { !model.busy && model.message == "Резервная копия создана" }
        ui.onNodeWithText("Восстановить последнюю копию", useUnmergedTree = true).performScrollTo()
        click("Восстановить последнюю копию")
        ui.onNodeWithText("Заменить расписание?").assertExists()
        click("Заменить")
        ui.waitUntil(10000) { !model.busy && model.message == "Данные восстановлены" }
        click("Расписание")
        screenshot("Schedule-Dark")
    }
}
