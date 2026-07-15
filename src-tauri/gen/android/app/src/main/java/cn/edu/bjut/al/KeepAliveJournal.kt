package cn.edu.bjut.al

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object KeepAliveJournal {
    private const val FILE_NAME = "keepalive-journal.log"

    private fun dataDirectory(context: Context): File =
        File(context.applicationInfo.dataDir)

    private fun journalFile(context: Context): File =
        File(dataDirectory(context), FILE_NAME)

    /**
     * Older builds wrote the foreground-service journal below filesDir while
     * Tauri resolves app_data_dir() to the private data root. Move that journal
     * before Rust starts so startup import, clearing and complete export all see
     * the same history.
     */
    private fun migrateLegacyJournal(context: Context): File {
        val destination = journalFile(context)
        val legacy = File(context.applicationContext.filesDir, FILE_NAME)
        if (!legacy.isFile || legacy.length() == 0L || legacy == destination) return destination
        try {
            if (!destination.exists() && legacy.renameTo(destination)) return destination
            destination.parentFile?.mkdirs()
            FileOutputStream(destination, true).buffered().use { output ->
                if (destination.length() > 0L) output.write('\n'.code)
                legacy.inputStream().buffered().use { input -> input.copyTo(output) }
            }
            legacy.delete()
        } catch (_: Exception) {
            // Keep both files; filesForExport() still includes the legacy copy.
        }
        return destination
    }

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
            migrateLegacyJournal(context).appendText(
                "[$timestamp] [$safeType] [$safeModule] $safeMessage\n",
                Charsets.UTF_8
            )
        } catch (_: Exception) {
            // The foreground service must not fail just because diagnostics cannot be written.
        }
    }

    @JvmStatic
    @Synchronized
    fun filesForExport(context: Context): List<File> {
        migrateLegacyJournal(context)
        val directories = listOf(dataDirectory(context), context.applicationContext.filesDir).distinct()
        return directories.flatMap { directory ->
            directory.listFiles()
                ?.filter {
                    it.isFile && (
                        it.name == FILE_NAME ||
                            (it.name.startsWith("keepalive-journal.importing") && it.name.endsWith(".log"))
                        ) && it.length() > 0L
                }
                .orEmpty()
        }.distinctBy { it.absolutePath }.sortedBy { it.name }
    }

    @JvmStatic
    @Synchronized
    fun clear(context: Context) {
        val directories = listOf(dataDirectory(context), context.applicationContext.filesDir).distinct()
        directories.forEach { directory ->
            directory.listFiles()
                ?.filter {
                    it.name == FILE_NAME ||
                        (it.name.startsWith("keepalive-journal.importing") && it.name.endsWith(".log"))
                }
                ?.forEach { it.delete() }
        }
    }

    @JvmStatic
    @Synchronized
    fun recordPreviousProcessExit(context: Context) {
        migrateLegacyJournal(context)
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
