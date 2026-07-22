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
import androidx.activity.OnBackPressedCallback
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
    getSharedPreferences("service_state", Context.MODE_PRIVATE)
      .edit()
      .putBoolean("engine_foreground", false)
      .apply()
    super.onCreate(savedInstanceState)
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val webView = appWebView
        if (webView == null) {
          moveTaskToBack(true)
          return
        }
        webView.evaluateJavascript("Boolean(window.__handleAndroidBack?.())") { handled ->
          if (handled != "true") moveTaskToBack(true)
        }
      }
    })
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
      .putBoolean("engine_foreground", false)
      .apply()
    try { unregisterReceiver(serviceEventReceiver) } catch (_: Exception) {}
    super.onDestroy()
  }

  override fun onResume() {
    super.onResume()
    getSharedPreferences("service_state", Context.MODE_PRIVATE)
      .edit()
      .putLong("engine_heartbeat", System.currentTimeMillis())
      .putBoolean("engine_foreground", true)
      .apply()
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
    getSharedPreferences("service_state", Context.MODE_PRIVATE)
      .edit()
      .putBoolean("engine_foreground", false)
      .apply()
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

  private fun updateKeepAliveStatusInternal(status: String): Boolean {
    val message = status.trim().take(80)
    val preferences = getSharedPreferences("service_state", Context.MODE_PRIVATE)
    if (message.isEmpty() || !preferences.getBoolean("auto_login_enabled", false)) return false
    return try {
      val serviceIntent = Intent(this, KeepAliveService::class.java)
        .setAction(KeepAliveService.ACTION_UPDATE_STATUS)
        .putExtra(KeepAliveService.EXTRA_STATUS_TEXT, message)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        startForegroundService(serviceIntent)
      } else {
        startService(serviceIntent)
      }
      true
    } catch (error: Exception) {
      KeepAliveJournal.append(this, "更新通知状态失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
      false
    }
  }

  private fun refreshNotificationSettingsInternal(): Boolean {
    val preferences = getSharedPreferences("service_state", Context.MODE_PRIVATE)
    if (!preferences.getBoolean("auto_login_enabled", false)) return false
    return try {
      val serviceIntent = Intent(this, KeepAliveService::class.java)
        .setAction(KeepAliveService.ACTION_REFRESH_NOTIFICATION_SETTINGS)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        startForegroundService(serviceIntent)
      } else {
        startService(serviceIntent)
      }
      true
    } catch (error: Exception) {
      KeepAliveJournal.append(this, "应用 Android 通知设置失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
      false
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

  private fun openAlipayInternal(rawUrl: String): Boolean {
    return try {
      val paymentUrl = Uri.parse(rawUrl)
      val trusted = paymentUrl.scheme.equals("https", ignoreCase = true) &&
        paymentUrl.host.equals("openapi.alipay.com", ignoreCase = true) &&
        paymentUrl.path == "/gateway.do" &&
        (paymentUrl.port == -1 || paymentUrl.port == 443) &&
        paymentUrl.userInfo.isNullOrEmpty() &&
        paymentUrl.fragment.isNullOrEmpty() &&
        paymentUrl.getQueryParameter("method") == "alipay.trade.wap.pay" &&
        paymentUrl.getQueryParameter("sign_type") == "RSA2"
      if (!trusted) return false

      val alipayUri = Uri.Builder()
        .scheme("alipays")
        .authority("platformapi")
        .appendPath("startapp")
        .appendQueryParameter("appId", "20000067")
        .appendQueryParameter("url", rawUrl)
        .build()
      val intent = Intent(Intent.ACTION_VIEW, alipayUri)
        .setPackage("com.eg.android.AlipayGphone")
        .addCategory(Intent.CATEGORY_BROWSABLE)
      if (intent.resolveActivity(packageManager) == null) return false
      startActivity(intent)
      true
    } catch (error: Exception) {
      KeepAliveJournal.append(this, "直接打开支付宝失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
      false
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
    fun updateKeepAliveStatus(status: String): Boolean =
      activity.updateKeepAliveStatusInternal(status)

    @JavascriptInterface
    fun refreshNotificationSettings(): Boolean =
      activity.refreshNotificationSettingsInternal()

    @JavascriptInterface
    fun requestBatteryOptimizations() {
      activity.runOnUiThread {
        activity.requestBatteryOptimizationsInternal()
      }
    }

    @JavascriptInterface
    fun openAlipay(url: String): Boolean {
      val completed = java.util.concurrent.CountDownLatch(1)
      var success = false
      activity.runOnUiThread {
        try {
          success = activity.openAlipayInternal(url)
        } finally {
          completed.countDown()
        }
      }
      return try {
        completed.await(2, java.util.concurrent.TimeUnit.SECONDS) && success
      } catch (error: InterruptedException) {
        Thread.currentThread().interrupt()
        false
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
        // Tauri's Android app_data_dir() is applicationInfo.dataDir, not filesDir.
        val source = File(activity.applicationInfo.dataDir, "app.log")
        val serviceLogs = KeepAliveJournal.filesForExport(activity)
        if ((!source.isFile || source.length() == 0L) && serviceLogs.isEmpty()) return false

        val exportDirectory = File(activity.cacheDir, "exports").apply { mkdirs() }
        val timestamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val destination = File(exportDirectory, "BJUT-AL-logs-$timestamp.log")
        val timestampPattern = Regex("^\\[(\\d{4}-\\d{2}-\\d{2} \\d{2}:\\d{2}:\\d{2})]")
        val indexedLines = mutableListOf<Pair<String, Int>>()
        val seenLines = HashSet<String>()
        val sources = (listOf(source) + serviceLogs)
          .filter { it.isFile && it.length() > 0L }
          .distinctBy { it.absolutePath }
        var lineIndex = 0
        // The foreground service keeps writing while the WebView/Rust process
        // is absent. Merge every source chronologically and suppress exact
        // duplicates left by a crash during journal import.
        for (logFile in sources) {
          logFile.bufferedReader(Charsets.UTF_8).useLines { lines ->
            lines.forEach { line ->
              if (line.isNotBlank() && seenLines.add(line)) {
                indexedLines.add(line to lineIndex++)
              }
            }
          }
        }
        indexedLines.sortWith(
          compareBy<Pair<String, Int>> {
            timestampPattern.find(it.first)?.groupValues?.get(1) ?: "9999-99-99 99:99:99"
          }.thenBy { it.second }
        )
        destination.bufferedWriter(Charsets.UTF_8).use { writer ->
          indexedLines.forEach { (line, _) -> writer.appendLine(line) }
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

    @JavascriptInterface
    fun shareExportFile(path: String, subject: String): Boolean {
      return try {
        val exportDirectory = File(activity.cacheDir, "exports").canonicalFile
        val destination = File(path).canonicalFile
        if (
          destination.parentFile != exportDirectory ||
          !destination.isFile ||
          !destination.name.endsWith(".csv", ignoreCase = true) ||
          destination.length() <= 0L ||
          destination.length() > 32L * 1024L * 1024L
        ) return false

        val uri = FileProvider.getUriForFile(
          activity,
          "${activity.packageName}.fileprovider",
          destination
        )
        val shareIntent = Intent(Intent.ACTION_SEND).apply {
          type = "text/csv"
          putExtra(Intent.EXTRA_STREAM, uri)
          putExtra(Intent.EXTRA_SUBJECT, subject.take(80))
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val completed = java.util.concurrent.CountDownLatch(1)
        var launched = false
        activity.runOnUiThread {
          try {
            activity.startActivity(Intent.createChooser(shareIntent, "导出账单记录"))
            launched = true
          } catch (error: Exception) {
            KeepAliveJournal.append(activity, "启动账单分享窗口失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
          } finally {
            completed.countDown()
          }
        }
        completed.await(3, java.util.concurrent.TimeUnit.SECONDS) && launched
      } catch (error: InterruptedException) {
        Thread.currentThread().interrupt()
        false
      } catch (error: Exception) {
        KeepAliveJournal.append(activity, "导出账单失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
        false
      }
    }

    @JavascriptInterface
    fun clearServiceLogs() {
      KeepAliveJournal.clear(activity)
    }
  }
}
