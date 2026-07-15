package cn.edu.bjut.al

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object KeepAliveJournal {
    private const val FILE_NAME = "keepalive-journal.log"

    @JvmStatic
    @Synchronized
    fun append(
        context: Context,
        message: String,
        type: String = "info",
        module: String = "Android后台"
    ) {
        try {
            val timestamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date())
            val safeType = type.replace(Regex("[^a-z]"), "").ifEmpty { "info" }
            val safeModule = module.replace('\n', ' ').replace('\r', ' ')
            val safeMessage = message.replace('\n', ' ').replace('\r', ' ')
            File(context.applicationContext.filesDir, FILE_NAME).appendText(
                "[$timestamp] [$safeType] [$safeModule] $safeMessage\n",
                Charsets.UTF_8
            )
        } catch (_: Exception) {
            // The foreground service must not fail just because diagnostics cannot be written.
        }
    }

    @JvmStatic
    fun recordPreviousProcessExit(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        try {
            val appContext = context.applicationContext
            val manager = appContext.getSystemService(ActivityManager::class.java)
            val latest = manager.getHistoricalProcessExitReasons(appContext.packageName, 0, 5)
                .maxByOrNull { it.timestamp } ?: return
            val preferences = appContext.getSharedPreferences("service_state", Context.MODE_PRIVATE)
            val lastRecorded = preferences.getLong("last_exit_timestamp", 0L)
            if (latest.timestamp <= lastRecorded) return
            preferences.edit().putLong("last_exit_timestamp", latest.timestamp).apply()
            val reason = when (latest.reason) {
                ApplicationExitInfo.REASON_ANR -> "ANR"
                ApplicationExitInfo.REASON_CRASH -> "Java崩溃"
                ApplicationExitInfo.REASON_CRASH_NATIVE -> "原生崩溃"
                ApplicationExitInfo.REASON_DEPENDENCY_DIED -> "依赖进程退出"
                ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "资源占用过高"
                ApplicationExitInfo.REASON_EXIT_SELF -> "应用主动退出"
                ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "初始化失败"
                ApplicationExitInfo.REASON_LOW_MEMORY -> "系统低内存回收"
                ApplicationExitInfo.REASON_OTHER -> "其他"
                ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "权限变化"
                ApplicationExitInfo.REASON_SIGNALED -> "收到系统信号"
                ApplicationExitInfo.REASON_UNKNOWN -> "未知"
                ApplicationExitInfo.REASON_USER_REQUESTED -> "用户或系统任务管理器停止"
                else -> "代码${latest.reason}"
            }
            val description = latest.description?.replace('\n', ' ')?.take(180).orEmpty()
            val details = buildString {
                append("检测到上次进程退出：$reason")
                append("，时间=")
                append(SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(latest.timestamp)))
                append("，重要性=${latest.importance}，PSS=${latest.pss / 1024}MB，RSS=${latest.rss / 1024}MB")
                if (description.isNotEmpty()) append("，说明=$description")
            }
            append(appContext, details, if (latest.reason == ApplicationExitInfo.REASON_EXIT_SELF) "info" else "error")
        } catch (error: Exception) {
            append(context, "读取上次进程退出原因失败：${error.javaClass.simpleName}", "debug")
        }
    }
}
