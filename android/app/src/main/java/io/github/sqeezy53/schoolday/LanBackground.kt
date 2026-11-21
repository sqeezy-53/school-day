package io.github.sqeezy53.schoolday

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.SharedPreferences
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

enum class LanMode(val label: String) {
    Automatic("Автоматическая"),
    Manual("Вручную"),
    Off("Отключена");

    companion object {
        fun read(preferences: SharedPreferences): LanMode =
            entries.firstOrNull { it.name == preferences.getString("lan_mode", null) } ?: Manual
    }
}

internal class LanFailure(message: String) : Exception(message)

internal object LanRuntime {
    // The Store and JobService operate on the same files, even during Activity startup.
    val worker = Executors.newSingleThreadExecutor()
    val backgroundOperation = AtomicLong()
    @Volatile var foreground = false

    fun cancelBackground() {
        backgroundOperation.get().takeIf { it != 0L }?.let(Core::lanCancel)
    }
}

class LanBackground : JobService() {
    private var active = 0L
    private val main = Handler(Looper.getMainLooper())

    override fun onStartJob(params: JobParameters): Boolean {
        if (LanRuntime.foreground || !enabled(this) || !hasUnmeteredRoute()) return false
        val operation = Core.lanBegin()
        if (operation == 0L) return false
        active = operation
        LanRuntime.backgroundOperation.set(operation)
        LanRuntime.worker.execute {
            try {
                if (!LanRuntime.foreground && enabled(this) && hasUnmeteredRoute()) {
                    // A background task never approves a first merge or conflict.
                    Core.lanCommand(filesDir.path, "auto", "", operation, LanObserver { null })
                }
            } finally {
                LanRuntime.backgroundOperation.compareAndSet(operation, 0)
                Core.lanEnd(operation)
                main.post {
                    if (active == operation) {
                        active = 0
                        jobFinished(params, false)
                    }
                }
            }
        }
        return true
    }

    private fun hasUnmeteredRoute(): Boolean {
        val network = getSystemService(ConnectivityManager::class.java)
        return network.getNetworkCapabilities(network.activeNetwork)
            ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        if (active != 0L) {
            Core.lanCancel(active)
            active = 0
        }
        return false
    }

    companion object {
        private const val JOB_ID = 0x5344

        private fun enabled(context: Context): Boolean {
            val preferences = context.getSharedPreferences("display", Context.MODE_PRIVATE)
            return LanMode.read(preferences) == LanMode.Automatic &&
                preferences.getBoolean("lan_background", false)
        }

        fun update(context: Context, enabled: Boolean): Boolean {
            val scheduler = context.getSystemService(JobScheduler::class.java)
            if (!enabled) {
                LanRuntime.cancelBackground()
                scheduler.cancel(JOB_ID)
                return true
            }
            if (scheduler.getPendingJob(JOB_ID) != null) return true
            val request = JobInfo.Builder(JOB_ID, ComponentName(context, LanBackground::class.java))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_UNMETERED)
                .setRequiresBatteryNotLow(true)
                .setPeriodic(TimeUnit.MINUTES.toMillis(30), TimeUnit.MINUTES.toMillis(10))
                .build()
            return try {
                scheduler.schedule(request) == JobScheduler.RESULT_SUCCESS
            } catch (_: SecurityException) {
                false
            } catch (_: IllegalStateException) {
                false
            }
        }
    }
}
