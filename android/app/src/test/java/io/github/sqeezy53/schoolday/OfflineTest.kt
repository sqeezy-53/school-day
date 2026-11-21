package io.github.sqeezy53.schoolday

import android.net.Uri
import android.os.Looper
import androidx.lifecycle.ViewModelProvider
import java.io.File
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class OfflineTest {
    private fun await(store: Store) {
        val stop = System.nanoTime() + 10_000_000_000L
        while (store.busy && System.nanoTime() < stop) {
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(10)
        }
        assertFalse("Операция не завершилась", store.busy)
    }

    @Test
    fun editRotateReopenAndImport() {
        val controller = Robolectric.buildActivity(MainActivity::class.java).setup()
        val store = ViewModelProvider(controller.get())[Store::class.java]
        await(store)
        val lesson =
            obj(
                "title" to "Физика",
                "date" to "2031-10-12",
                "time" to "08:00:00",
                "end" to "08:40:00",
                "homework" to "Упражнение 1",
            )
        store.edit(obj("op" to "save_lesson", "lesson" to lesson))
        await(store)
        val task = store.view!!.getJSONArray("tasks").getJSONObject(0)
        val id = task.getString("id")
        store.edit(obj("op" to "complete", "id" to id, "linked" to true, "done" to true))
        await(store)
        val date = "2031-10-12"
        store.edit(
            obj(
                "op" to "day",
                "date" to date,
                "options" to obj("remote" to true, "short_minutes" to 30),
            )
        )
        await(store)
        assertEquals("08:30:00", store.view!!.getJSONObject("short_ends").getString(id))
        assertTrue(
            store.view!!
                .getJSONObject("document")
                .getJSONObject("days")
                .getJSONObject(date)
                .getBoolean("remote")
        )
        lesson.put(
            "options",
            obj("cancelled" to true, "replacement" to "География", "work" to "test"),
        )
        store.edit(obj("op" to "save_lesson", "lesson" to lesson, "id" to id))
        await(store)
        val options =
            store.view!!
                .getJSONObject("document")
                .getJSONArray("lessons")
                .getJSONObject(0)
                .getJSONObject("options")
        assertTrue(options.getBoolean("cancelled"))
        assertEquals("География", options.getString("replacement"))
        assertEquals("test", options.getString("work"))
        store.edit(obj("op" to "save_task", "title" to "История", "body" to "Вопрос 1"))
        await(store)
        val separate = store.view!!.getJSONArray("tasks").getJSONObject(1).getString("id")
        store.edit(
            obj("op" to "save_task", "id" to separate, "title" to "История", "body" to "Вопрос 2")
        )
        await(store)
        assertEquals(
            "Вопрос 2",
            store.view!!.getJSONArray("tasks").getJSONObject(1).getString("body"),
        )
        store.edit(obj("op" to "delete_task", "id" to separate, "linked" to false))
        await(store)
        assertEquals(1, store.view!!.getJSONArray("tasks").length())
        controller.configurationChange()
        val rotated = ViewModelProvider(controller.get())[Store::class.java]
        await(rotated)
        assertTrue(rotated.view!!.getJSONArray("tasks").getJSONObject(0).getBoolean("completed"))
        val output = File(rotated.app.cacheDir, "export.json")
        rotated.export(Uri.fromFile(output))
        await(rotated)
        assertTrue(output.isFile)
        lesson.put("title", "Геометрия")
        rotated.edit(obj("op" to "save_lesson", "lesson" to lesson, "id" to id))
        await(rotated)
        controller.pause().stop().destroy()
        val fresh = Robolectric.buildActivity(MainActivity::class.java).setup()
        val reopened = ViewModelProvider(fresh.get())[Store::class.java]
        await(reopened)
        assertEquals(
            "Геометрия",
            reopened.view!!.getJSONArray("tasks").getJSONObject(0).getString("title"),
        )
        reopened.inspect(Uri.fromFile(output))
        await(reopened)
        assertNotNull(reopened.preview)
        reopened.importData()
        await(reopened)
        assertEquals(
            "Физика",
            reopened.view!!.getJSONArray("tasks").getJSONObject(0).getString("title"),
        )
        val source = File(reopened.app.filesDir, "lessons.json")
        val before = source.readBytes()
        output.writeText("{broken")
        reopened.inspect(Uri.fromFile(output))
        await(reopened)
        assertTrue(reopened.message.isNotEmpty())
        assertNull(reopened.preview)
        reopened.edit(obj("op" to "unknown"))
        await(reopened)
        assertTrue(reopened.message.isNotEmpty())
        assertArrayEquals(before, source.readBytes())
        reopened.edit(obj("op" to "delete_lesson", "id" to id))
        await(reopened)
        assertEquals(0, reopened.view!!.getJSONArray("tasks").length())
        assertEquals(0, reopened.view!!.getJSONObject("document").getJSONArray("lessons").length())
        fresh.pause().stop().destroy()
    }

    @Test
    fun cleanupUndoBellsAndLocalBackup() {
        val controller = Robolectric.buildActivity(MainActivity::class.java).setup()
        val store = ViewModelProvider(controller.get())[Store::class.java]
        await(store)
        val lesson =
            obj(
                "title" to "Физика",
                "date" to "2026-11-21",
                "time" to "08:00:00",
                "end" to "08:40:00",
                "homework" to "Задача",
            )
        store.edit(obj("op" to "save_lesson", "lesson" to lesson))
        await(store)
        val id = store.view!!.getJSONArray("tasks").getJSONObject(0).getString("id")
        store.edit(obj("op" to "complete", "id" to id, "linked" to true, "done" to true))
        await(store)
        store.edit(obj("op" to "save_task", "title" to "Музыка", "body" to "Ноты"))
        await(store)
        store.backup()
        await(store)
        assertTrue(store.canRestoreBackup)
        store.edit(obj("op" to "clear_completed", "expected" to 1))
        await(store)
        assertEquals(1, store.view!!.getJSONArray("tasks").length())
        assertEquals(1, store.view!!.getJSONObject("document").getJSONArray("lessons").length())
        assertTrue(store.canUndoCleanup)
        store.undoCleanup()
        await(store)
        assertEquals(2, store.view!!.getJSONArray("tasks").length())
        assertFalse(store.canUndoCleanup)

        val bells =
            obj(
                "lesson_minutes" to 45,
                "first_start" to "08:20:00",
                "breaks" to org.json.JSONArray(listOf(15)),
            )
        store.edit(obj("op" to "set_bells", "bells" to bells))
        await(store)
        assertEquals(
            "08:20:00",
            store.view!!.getJSONArray("bells").getJSONObject(0).getString("time"),
        )
        assertEquals(
            "09:20:00",
            store.view!!.getJSONArray("bells").getJSONObject(1).getString("time"),
        )
        store.inspectBackup()
        await(store)
        assertNotNull(store.preview)
        store.importData()
        await(store)
        assertEquals(
            "08:00:00",
            store.view!!.getJSONArray("bells").getJSONObject(0).getString("time"),
        )
        controller.pause().stop().destroy()
    }
}

private fun assertLaunchWithAdaptiveIcon() {
    val controller = Robolectric.buildActivity(MainActivity::class.java).setup()
    val activity = controller.get()
    val store = ViewModelProvider(activity)[Store::class.java]
    val timeout = System.nanoTime() + 10_000_000_000L
    while (store.busy && System.nanoTime() < timeout) {
        shadowOf(Looper.getMainLooper()).idle()
        Thread.sleep(10)
    }
    assertNotNull(store.message, store.view)
    assertEquals(R.mipmap.ic_launcher, activity.applicationInfo.icon)
    assertTrue(
        activity.getDrawable(R.mipmap.ic_launcher) is android.graphics.drawable.AdaptiveIconDrawable
    )
    if (android.os.Build.VERSION.SDK_INT >= 31) {
        val attributes =
            activity.obtainStyledAttributes(
                intArrayOf(android.R.attr.windowSplashScreenAnimatedIcon)
            )
        try {
            assertTrue(attributes.getResourceId(0, 0) != 0)
        } finally {
            attributes.recycle()
        }
    }
    controller.pause().stop().destroy()
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [26])
class LaunchApi26Test {
    @Test fun launch() = assertLaunchWithAdaptiveIcon()
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [31])
class LaunchApi31Test {
    @Test fun launch() = assertLaunchWithAdaptiveIcon()
}
