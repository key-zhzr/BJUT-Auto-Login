package cn.edu.bjut.al

import android.os.Bundle
import android.content.Context
import android.content.Intent
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private var resumedFromBackground = false
  private val engineHeartbeatHandler = Handler(Looper.getMainLooper())
  private val engineHeartbeatRunnable = object : Runnable {
    override fun run() {
      getSharedPreferences("service_state", Context.MODE_PRIVATE)
        .edit()
        .putLong("engine_heartbeat", System.currentTimeMillis())
        .apply()
      engineHeartbeatHandler.postDelayed(this, 10_000L)
    }
  }
  private val serviceEventReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
      val action = intent?.getStringExtra(KeepAliveService.EXTRA_COMMAND) ?: return
      val script = when (action) {
        KeepAliveService.COMMAND_NETWORK_CHANGED -> "window.__nativeNetworkChanged?.('Android NetworkCallback')"
        KeepAliveService.COMMAND_CHECK -> "window.__nativeNotificationAction?.('check')"
        KeepAliveService.COMMAND_PAUSE -> "window.__nativeNotificationAction?.('pause')"
        KeepAliveService.COMMAND_RESUME -> "window.__nativeNotificationAction?.('resume')"
        else -> null
      } ?: return
      appWebView?.post { appWebView?.evaluateJavascript(script, null) }
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    KeepAliveJournal.recordPreviousProcessExit(this)
    super.onCreate(savedInstanceState)
    engineHeartbeatHandler.post(engineHeartbeatRunnable)
    ContextCompat.registerReceiver(
      this,
      serviceEventReceiver,
      IntentFilter(KeepAliveService.ACTION_SERVICE_EVENT),
      ContextCompat.RECEIVER_NOT_EXPORTED
    )
  }

  override fun onDestroy() {
    engineHeartbeatHandler.removeCallbacks(engineHeartbeatRunnable)
    getSharedPreferences("service_state", Context.MODE_PRIVATE)
      .edit()
      .putLong("engine_heartbeat", 0L)
      .apply()
    try { unregisterReceiver(serviceEventReceiver) } catch (_: Exception) {}
    super.onDestroy()
  }

  override fun onResume() {
    super.onResume()
    UpdateHelper.resumePendingInstall(this)
    if (resumedFromBackground) {
      appWebView?.post {
        appWebView?.evaluateJavascript("window.__showResumeMask?.()", null)
      }
      resumedFromBackground = false
    }
  }

  override fun onPause() {
    resumedFromBackground = true
    super.onPause()
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    appWebView = webView
    webView.setBackgroundColor(android.graphics.Color.rgb(15, 23, 42))
    // Register JavaScript interface so frontend can call Android native methods directly
    webView.addJavascriptInterface(AndroidBridge(this), "AndroidBridge")
  }

  private fun requestForegroundPermissionsInternal() {
    val permissions = mutableListOf<String>()
    
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        permissions.add(android.Manifest.permission.NEARBY_WIFI_DEVICES)
        permissions.add(android.Manifest.permission.POST_NOTIFICATIONS)
    }
    
    permissions.add(android.Manifest.permission.ACCESS_FINE_LOCATION)
    permissions.add(android.Manifest.permission.ACCESS_COARSE_LOCATION)
    
    val toRequest = permissions.filter {
        androidx.core.content.ContextCompat.checkSelfPermission(this, it) != android.content.pm.PackageManager.PERMISSION_GRANTED
    }
    
    if (toRequest.isNotEmpty()) {
        androidx.core.app.ActivityCompat.requestPermissions(this, toRequest.toTypedArray(), 101)
    }
  }

  private fun requestBackgroundPermissionsInternal() {
    val permissions = mutableListOf<String>()
    
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        permissions.add(android.Manifest.permission.POST_NOTIFICATIONS)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        permissions.add(android.Manifest.permission.ACCESS_BACKGROUND_LOCATION)
    }
    
    val toRequest = permissions.filter {
        androidx.core.content.ContextCompat.checkSelfPermission(this, it) != android.content.pm.PackageManager.PERMISSION_GRANTED
    }
    
    if (toRequest.isNotEmpty()) {
        androidx.core.app.ActivityCompat.requestPermissions(this, toRequest.toTypedArray(), 102)
    }
  }

  private fun startKeepAliveServiceInternal() {
    try {
        getSharedPreferences("service_state", Context.MODE_PRIVATE)
          .edit()
          .putBoolean("auto_login_enabled", true)
          .apply()
        KeepAliveRestartScheduler.cancel(this)
        val serviceIntent = Intent(this, KeepAliveService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
    } catch (e: Exception) {
        e.printStackTrace()
    }
  }

  private fun stopKeepAliveServiceInternal() {
    try {
        getSharedPreferences("service_state", Context.MODE_PRIVATE)
          .edit()
          .putBoolean("auto_login_enabled", false)
          .putLong("paused_until", 0L)
          .apply()
        KeepAliveRestartScheduler.cancel(this)
        val serviceIntent = Intent(this, KeepAliveService::class.java)
        stopService(serviceIntent)
    } catch (e: Exception) {
        e.printStackTrace()
    }
  }

  private fun requestBatteryOptimizationsInternal() {
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
          val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
          intent.data = Uri.parse("package:$packageName")
          startActivity(intent)
      }
    } catch (e: Exception) {
      e.printStackTrace()
    }
  }

  private fun permissionHealthJson(): String {
    fun granted(permission: String): Boolean =
      ContextCompat.checkSelfPermission(this, permission) == android.content.pm.PackageManager.PERMISSION_GRANTED
    val notifications = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || granted(android.Manifest.permission.POST_NOTIFICATIONS)
    val nearbyWifi = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || granted(android.Manifest.permission.NEARBY_WIFI_DEVICES)
    val foregroundLocation = granted(android.Manifest.permission.ACCESS_FINE_LOCATION) || granted(android.Manifest.permission.ACCESS_COARSE_LOCATION)
    val backgroundLocation = Build.VERSION.SDK_INT < Build.VERSION_CODES.Q || granted(android.Manifest.permission.ACCESS_BACKGROUND_LOCATION)
    val battery = Build.VERSION.SDK_INT < Build.VERSION_CODES.M ||
      (getSystemService(Context.POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(packageName)
    val installPackages = Build.VERSION.SDK_INT < Build.VERSION_CODES.O || packageManager.canRequestPackageInstalls()
    val servicePreferences = getSharedPreferences("service_state", Context.MODE_PRIVATE)
    val keepAliveEnabled = servicePreferences.getBoolean("auto_login_enabled", false)
    val serviceHeartbeat = servicePreferences.getLong("service_heartbeat", 0L)
    val serviceHealthy = serviceHeartbeat > 0L && System.currentTimeMillis() - serviceHeartbeat < 90_000L
    val serviceDetail = when {
      !keepAliveEnabled -> "后台自动登录未开启，无需常驻检测引擎"
      serviceHealthy -> "前台服务心跳正常"
      else -> "未收到前台服务心跳；请检查系统后台限制"
    }
    return JSONArray()
      .put(JSONObject().put("id", "notifications").put("label", "通知权限").put("granted", notifications).put("required", true))
      .put(JSONObject().put("id", "nearbyWifi").put("label", "附近 Wi-Fi 设备").put("granted", nearbyWifi).put("required", true))
      .put(JSONObject().put("id", "foregroundLocation").put("label", "前台位置权限").put("granted", foregroundLocation).put("required", true))
      .put(JSONObject().put("id", "backgroundLocation").put("label", "后台位置权限").put("granted", backgroundLocation).put("required", false))
      .put(JSONObject().put("id", "batteryOptimization").put("label", "忽略电池优化").put("granted", battery).put("required", false))
      .put(JSONObject().put("id", "installPackages").put("label", "安装应用更新").put("granted", installPackages).put("required", true))
      .put(JSONObject().put("id", "keepAliveEngine").put("label", "后台检测引擎").put("granted", !keepAliveEnabled || serviceHealthy).put("required", keepAliveEnabled).put("detail", serviceDetail))
      .toString()
  }

  private fun openPermissionSettingsInternal(kind: String) {
    when (kind) {
      "notifications" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, packageName))
      } else startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
      "batteryOptimization" -> requestBatteryOptimizationsInternal()
      "installPackages" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
      }
      "backgroundLocation" -> requestBackgroundPermissionsInternal()
      "nearbyWifi", "foregroundLocation" -> requestForegroundPermissionsInternal()
      "keepAliveEngine" -> if (
        getSharedPreferences("service_state", Context.MODE_PRIVATE)
          .getBoolean("auto_login_enabled", false)
      ) startKeepAliveServiceInternal()
      else -> startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
    }
  }

  /**
   * JavaScript bridge exposed to the WebView as window.AndroidBridge
   * All methods annotated with @JavascriptInterface are callable from JS.
   * JS bridge methods run on a WebView background thread, so we use runOnUiThread
   * for any Activity or UI operations.
   */
  inner class AndroidBridge(private val activity: MainActivity) {
    @JavascriptInterface
    fun requestForegroundPermissions() {
      activity.runOnUiThread {
        activity.requestForegroundPermissionsInternal()
      }
    }

    @JavascriptInterface
    fun requestBackgroundPermissions() {
      activity.runOnUiThread {
        activity.requestBackgroundPermissionsInternal()
      }
    }

    @JavascriptInterface
    fun startKeepAliveService() {
      activity.runOnUiThread {
        activity.startKeepAliveServiceInternal()
      }
    }

    @JavascriptInterface
    fun stopKeepAliveService() {
      activity.runOnUiThread {
        activity.stopKeepAliveServiceInternal()
      }
    }

    @JavascriptInterface
    fun requestBatteryOptimizations() {
      activity.runOnUiThread {
        activity.requestBatteryOptimizationsInternal()
      }
    }

    @JavascriptInterface
    fun getPermissionHealth(): String = activity.permissionHealthJson()

    @JavascriptInterface
    fun getAutoLoginPausedUntil(): Long = activity
      .getSharedPreferences("service_state", Context.MODE_PRIVATE)
      .getLong("paused_until", 0L)

    @JavascriptInterface
    fun openPermissionSettings(kind: String) {
      activity.runOnUiThread { activity.openPermissionSettingsInternal(kind) }
    }

    @JavascriptInterface
    fun getClipboardText(): String {
      var text = ""
      try {
        val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        if (clipboard.hasPrimaryClip()) {
            val clipData = clipboard.primaryClip
            if (clipData != null && clipData.itemCount > 0) {
                val itemText = clipData.getItemAt(0).text
                if (itemText != null) {
                    text = itemText.toString()
                }
            }
        }
      } catch (e: Exception) {
        e.printStackTrace()
      }
      return text
    }

    @JavascriptInterface
    fun setClipboardText(text: String): Boolean {
      val completed = java.util.concurrent.CountDownLatch(1)
      var success = false
      activity.runOnUiThread {
        try {
          val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
          val clip = android.content.ClipData.newPlainText("bjut_al_config", text)
          clipboard.setPrimaryClip(clip)
          success = true
        } catch (e: Exception) {
          e.printStackTrace()
        } finally {
          completed.countDown()
        }
      }
      return try {
        completed.await(2, java.util.concurrent.TimeUnit.SECONDS) && success
      } catch (e: InterruptedException) {
        Thread.currentThread().interrupt()
        false
      }
    }

    @JavascriptInterface
    fun exportLogs(): Boolean {
      return try {
        // JavascriptInterface methods already run away from the Android main
        // thread. Keep potentially large, full-history file copies there.
        val source = File(activity.filesDir, "app.log")
        val serviceLogs = activity.filesDir.listFiles()
          ?.filter {
            it.isFile && (
              it.name == "keepalive-journal.log" ||
                (it.name.startsWith("keepalive-journal.importing") && it.name.endsWith(".log"))
            ) && it.length() > 0L
          }
          ?.sortedBy { it.name }
          .orEmpty()
        if ((!source.isFile || source.length() == 0L) && serviceLogs.isEmpty()) return false

        val exportDirectory = File(activity.cacheDir, "exports").apply { mkdirs() }
        val timestamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val destination = File(exportDirectory, "BJUT-AL-logs-$timestamp.log")
        destination.outputStream().buffered().use { output ->
          if (source.isFile && source.length() > 0L) {
            source.inputStream().buffered().use { it.copyTo(output) }
          }
          // The foreground service keeps writing a separate journal while the
          // WebView/Rust process is absent. Include current and crash-recovery
          // import files so the shared export is never limited to the 5000 UI rows.
          for (serviceLog in serviceLogs) {
            serviceLog.inputStream().buffered().use { it.copyTo(output) }
          }
        }
        val uri = FileProvider.getUriForFile(
          activity,
          "${activity.packageName}.fileprovider",
          destination
        )
        val shareIntent = Intent(Intent.ACTION_SEND).apply {
          type = "text/plain"
          putExtra(Intent.EXTRA_STREAM, uri)
          putExtra(Intent.EXTRA_SUBJECT, "BJUT-AL 完整运行日志")
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val completed = java.util.concurrent.CountDownLatch(1)
        var launched = false
        activity.runOnUiThread {
          try {
            activity.startActivity(Intent.createChooser(shareIntent, "导出完整日志"))
            launched = true
          } catch (error: Exception) {
            KeepAliveJournal.append(activity, "启动日志分享窗口失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
          } finally {
            completed.countDown()
          }
        }
        completed.await(3, java.util.concurrent.TimeUnit.SECONDS) && launched
      } catch (error: InterruptedException) {
        Thread.currentThread().interrupt()
        false
      } catch (error: Exception) {
        KeepAliveJournal.append(activity, "导出日志失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
        false
      }
    }
  }
}
