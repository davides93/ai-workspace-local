package dev.aiworkspace.nativellm

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.BatteryManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.CoroutineWorker
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.Calendar
import java.util.concurrent.TimeUnit

/*
 * Background checks of the proactive assistant on Android (WorkManager). No server and no always-on service:
 * the system wakes a worker now and then (at least every 15 minutes, when it decides to), the worker asks the
 * Rust library for ONE short check (`BackgroundBridge.tick`, hard budget of 25 s) and shows the answer as a local
 * notification that opens `#/inbox`. The decisions (what is worth a notification, what needs a model) are all in
 * the engine; nothing here talks to the network.
 *
 * NOT COMPILED OR RUN in the repository's CI: the Android build only happens in the store workflow.
 */

/** Calls the shell's Rust library (`aiw_background` → `Java_dev_aiworkspace_nativellm_BackgroundBridge_nativeTick`). */
object BackgroundBridge {
    @Volatile private var loaded = false

    /** Same library Tauri loads for the activity (`[lib] name` of `apps/desktop/src-tauri`); loading twice is a no-op. */
    private fun load() {
        if (!loaded) {
            System.loadLibrary("aiw_desktop_lib")
            loaded = true
        }
    }

    @JvmStatic
    private external fun nativeTick(request: String): String

    fun tick(request: JSONObject): JSONObject {
        load()
        return JSONObject(nativeTick(request.toString()))
    }
}

/** What the Rust shell last handed over (`schedule_payload` in `background.rs`), kept for the workers. */
data class BackgroundConfig(
    val enabled: Boolean,
    val intervalMinutes: Int,
    val briefingTime: String,
    val onlyOnPower: Boolean,
    val notifications: Boolean,
    val dataDir: String?,
    val workspaceRoot: String?,
) {
    companion object {
        fun from(o: JSONObject) = BackgroundConfig(
            enabled = o.optBoolean("enabled", false),
            intervalMinutes = o.optInt("intervalMinutes", 30),
            briefingTime = o.optString("briefingTime", "08:00"),
            onlyOnPower = o.optBoolean("onlyOnPower", true),
            notifications = o.optBoolean("notifications", true),
            dataDir = o.optString("dataDir").takeIf { it.isNotEmpty() },
            workspaceRoot = o.optString("workspaceRoot").takeIf { it.isNotEmpty() },
        )
    }
}

object BackgroundScheduler {
    private const val PREFS = "aiw-background"
    private const val KEY_CONFIG = "config"
    private const val KEY_ASKED = "notifications-asked"
    const val PERIODIC = "aiw-proactive-periodic"
    const val BRIEFING = "aiw-proactive-briefing"
    const val NOW = "aiw-proactive-now"

    fun save(context: Context, cfg: JSONObject) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY_CONFIG, cfg.toString()).apply()
    }

    fun load(context: Context): BackgroundConfig? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_CONFIG, null)?.let { BackgroundConfig.from(JSONObject(it)) }

    fun markAsked(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_ASKED, true).apply()
    fun asked(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ASKED, false)

    /** Schedules (or cancels) the periodic check and the morning one from the settings the shell handed over. */
    fun apply(context: Context, cfg: BackgroundConfig) {
        val wm = WorkManager.getInstance(context)
        if (!cfg.enabled) {
            wm.cancelUniqueWork(PERIODIC)
            wm.cancelUniqueWork(BRIEFING)
            return
        }
        // `proactive.only_on_power`: the periodic check waits for the charger. WorkManager never runs it more
        // often than every 15 minutes, and the system decides the exact moment (Doze, battery saver).
        val constraints = Constraints.Builder().setRequiresCharging(cfg.onlyOnPower).build()
        val periodic = PeriodicWorkRequestBuilder<ProactiveWorker>(maxOf(15, cfg.intervalMinutes).toLong(), TimeUnit.MINUTES)
            .setConstraints(constraints)
            .setInputData(Data.Builder().putString("reason", "interval").build())
            .build()
        wm.enqueueUniquePeriodicWork(PERIODIC, ExistingPeriodicWorkPolicy.UPDATE, periodic)
        scheduleBriefing(context, cfg)
    }

    /** One-off check at the next `proactive.briefing_time`; the worker schedules the following one when it ends. */
    fun scheduleBriefing(context: Context, cfg: BackgroundConfig? = load(context)) {
        if (cfg == null || !cfg.enabled) return
        val request = OneTimeWorkRequestBuilder<ProactiveWorker>()
            .setInitialDelay(millisUntil(cfg.briefingTime), TimeUnit.MILLISECONDS)
            .setInputData(Data.Builder().putString("reason", "briefing").build())
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(BRIEFING, ExistingWorkPolicy.REPLACE, request)
    }

    /** «Controlla adesso»: an expedited one-off job, so that it also finishes when the app goes to the background. */
    fun checkNow(context: Context) {
        val request = OneTimeWorkRequestBuilder<ProactiveWorker>()
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setInputData(Data.Builder().putString("reason", "shortcut").build())
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(NOW, ExistingWorkPolicy.REPLACE, request)
    }

    fun isScheduled(context: Context): Boolean = try {
        WorkManager.getInstance(context).getWorkInfosForUniqueWork(PERIODIC).get()
            .any { it.state == WorkInfo.State.ENQUEUED || it.state == WorkInfo.State.RUNNING }
    } catch (e: Exception) {
        false
    }

    fun millisUntil(hhmm: String, now: Calendar = Calendar.getInstance()): Long {
        val parts = hhmm.split(":")
        val h = parts.getOrNull(0)?.trim()?.toIntOrNull()?.coerceIn(0, 23) ?: 8
        val m = parts.getOrNull(1)?.trim()?.toIntOrNull()?.coerceIn(0, 59) ?: 0
        val target = (now.clone() as Calendar).apply {
            set(Calendar.HOUR_OF_DAY, h)
            set(Calendar.MINUTE, m)
            set(Calendar.SECOND, 0)
            set(Calendar.MILLISECOND, 0)
            if (timeInMillis <= now.timeInMillis) add(Calendar.DAY_OF_YEAR, 1)
        }
        return target.timeInMillis - now.timeInMillis
    }
}

object ProactiveNotifier {
    private const val CHANNEL = "aiw_proactive"
    private const val ID = 4001

    private fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = context.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, "Proposte dell'assistente", NotificationManager.IMPORTANCE_DEFAULT).apply {
                    description = "Il riepilogo del mattino e le proposte che l'assistente prepara mentre non ci sei."
                },
            )
        }
    }

    /** Android 13+ asks for POST_NOTIFICATIONS at run time; before that the person can only switch the app off. */
    fun permissionGranted(context: Context): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return false
        }
        return NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    /** A tap opens the app on `#/inbox` (the plugin turns the `aiw_route` extra into the hash). */
    fun show(context: Context, title: String, body: String) {
        if (!permissionGranted(context)) return
        ensureChannel(context)
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            putExtra("aiw_route", "#/inbox")
            flags = android.content.Intent.FLAG_ACTIVITY_NEW_TASK or android.content.Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val tap = launch?.let { PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT) }
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setContentIntent(tap)
            .build()
        try {
            NotificationManagerCompat.from(context).notify(ID, n)
        } catch (e: SecurityException) {
            // The permission was withdrawn between the check and the call.
        }
    }

    /** What an expedited job shows on Android 11 and older while it runs (a foreground service of a few seconds). */
    fun foregroundInfo(context: Context): ForegroundInfo {
        ensureChannel(context)
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle("L'assistente sta controllando")
            .setOngoing(true)
            .build()
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(ID + 1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(ID + 1, n)
        }
    }
}

/** One check: asks the engine, shows what it says. Always ends `success`: a failed check is simply tried again next time. */
class ProactiveWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val ctx = applicationContext
        val reason = inputData.getString("reason") ?: "interval"
        val cfg = BackgroundScheduler.load(ctx)
        if (cfg == null || !cfg.enabled) return@withContext Result.success()
        try {
            val charging = ctx.getSystemService(BatteryManager::class.java)?.isCharging
            val request = JSONObject()
                .put("reason", reason)
                .put("budgetS", 25)
                .put("dataDir", cfg.dataDir)
                .put("workspaceRoot", cfg.workspaceRoot)
            if (charging != null) request.put("onPower", charging)
            val summary = BackgroundBridge.tick(request)
            val notice = summary.optJSONObject("notification")
            if (cfg.notifications && notice != null) {
                ProactiveNotifier.show(ctx, notice.optString("title"), notice.optString("body"))
            }
        } catch (e: Throwable) {
            android.util.Log.w("AiwBackground", "controllo non riuscito", e)
        } finally {
            if (reason == "briefing") BackgroundScheduler.scheduleBriefing(ctx, cfg)
        }
        Result.success()
    }

    override suspend fun getForegroundInfo(): ForegroundInfo = ProactiveNotifier.foregroundInfo(applicationContext)
}
