package cn.edu.bjut.al

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.core.content.ContextCompat

class KeepAliveRestartReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        val appContext = context.applicationContext
        val enabled = appContext.getSharedPreferences("service_state", Context.MODE_PRIVATE)
            .getBoolean("auto_login_enabled", false)
        if (!enabled) return
        // Keep a one-shot watchdog armed even when an OEM kills the process without
        // delivering Service.onDestroy. The service refreshes this alarm while alive.
        KeepAliveRestartScheduler.scheduleWatchdog(appContext)
        var reason = "系统广播=${intent?.action ?: "unknown"}"
        if (intent?.action == KeepAliveRestartScheduler.ACTION_RESTART) {
            val heartbeat = appContext.getSharedPreferences("service_state", Context.MODE_PRIVATE)
                .getLong("service_heartbeat", 0L)
            val now = System.currentTimeMillis()
            val heartbeatAge = if (heartbeat in 1..now) now - heartbeat else null
            if (heartbeatAge != null && heartbeatAge < 90_000L) {
                return
            }
            reason = if (heartbeatAge == null) {
                "看门狗未找到有效服务心跳"
            } else {
                "看门狗检测到服务心跳已过期 ${heartbeatAge / 1000} 秒"
            }
        }
        try {
            ContextCompat.startForegroundService(appContext, Intent(appContext, KeepAliveService::class.java))
            KeepAliveJournal.append(appContext, "后台保活恢复广播已请求启动或唤醒前台服务（$reason）")
        } catch (error: Exception) {
            KeepAliveJournal.append(
                appContext,
                "后台保活恢复广播无法拉起服务：${error.javaClass.simpleName}: ${error.message.orEmpty()}",
                "error"
            )
        }
    }
}

class KeepAliveBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        KeepAliveRestartReceiver().onReceive(context, intent)
    }
}

object KeepAliveRestartScheduler {
    const val ACTION_RESTART = "cn.edu.bjut.al.action.RESTART_KEEP_ALIVE"
    private const val WATCHDOG_INTERVAL_MILLIS = 30 * 60 * 1000L

    @JvmStatic
    fun schedule(context: Context, delayMillis: Long = 15_000L, reason: String = "unknown") {
        scheduleInternal(context, delayMillis, reason, true)
    }

    @JvmStatic
    fun scheduleWatchdog(context: Context) {
        scheduleInternal(context, WATCHDOG_INTERVAL_MILLIS, "watchdog", false)
    }

    private fun scheduleInternal(
        context: Context,
        delayMillis: Long,
        reason: String,
        writeJournal: Boolean
    ) {
        val appContext = context.applicationContext
        val enabled = appContext.getSharedPreferences("service_state", Context.MODE_PRIVATE)
            .getBoolean("auto_login_enabled", false)
        if (!enabled) return
        try {
            val intent = Intent(appContext, KeepAliveRestartReceiver::class.java).setAction(ACTION_RESTART)
            val pendingIntent = PendingIntent.getBroadcast(
                appContext,
                41,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            val alarmManager = appContext.getSystemService(AlarmManager::class.java)
            val triggerAt = SystemClock.elapsedRealtime() + delayMillis.coerceAtLeast(5_000L)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                alarmManager.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, triggerAt, pendingIntent)
            } else {
                alarmManager.set(AlarmManager.ELAPSED_REALTIME_WAKEUP, triggerAt, pendingIntent)
            }
            if (writeJournal) {
                KeepAliveJournal.append(appContext, "已安排后台保活恢复（原因=$reason，延迟=${delayMillis / 1000}秒）", "debug")
            }
        } catch (error: Exception) {
            KeepAliveJournal.append(appContext, "安排后台保活恢复失败：${error.javaClass.simpleName}", "error")
        }
    }

    @JvmStatic
    fun cancel(context: Context) {
        val appContext = context.applicationContext
        val pendingIntent = PendingIntent.getBroadcast(
            appContext,
            41,
            Intent(appContext, KeepAliveRestartReceiver::class.java).setAction(ACTION_RESTART),
            PendingIntent.FLAG_NO_CREATE or PendingIntent.FLAG_IMMUTABLE
        ) ?: return
        appContext.getSystemService(AlarmManager::class.java).cancel(pendingIntent)
        pendingIntent.cancel()
    }
}
