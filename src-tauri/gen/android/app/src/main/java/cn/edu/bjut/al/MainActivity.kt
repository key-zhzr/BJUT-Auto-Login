package cn.edu.bjut.al

import android.os.Bundle
import android.content.Context
import android.content.Intent
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.os.Build
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat
import org.json.JSONArray
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private var resumedFromBackground = false
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
    super.onCreate(savedInstanceState)
    ContextCompat.registerReceiver(
      this,
      serviceEventReceiver,
      IntentFilter(KeepAliveService.ACTION_SERVICE_EVENT),
      ContextCompat.RECEIVER_NOT_EXPORTED
    )
  }

  override fun onDestroy() {
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
    return JSONArray()
      .put(JSONObject().put("id", "notifications").put("label", "通知权限").put("granted", notifications).put("required", true))
      .put(JSONObject().put("id", "nearbyWifi").put("label", "附近 Wi-Fi 设备").put("granted", nearbyWifi).put("required", true))
      .put(JSONObject().put("id", "foregroundLocation").put("label", "前台位置权限").put("granted", foregroundLocation).put("required", true))
      .put(JSONObject().put("id", "backgroundLocation").put("label", "后台位置权限").put("granted", backgroundLocation).put("required", false))
      .put(JSONObject().put("id", "batteryOptimization").put("label", "忽略电池优化").put("granted", battery).put("required", false))
      .put(JSONObject().put("id", "installPackages").put("label", "安装应用更新").put("granted", installPackages).put("required", true))
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
  }
}
