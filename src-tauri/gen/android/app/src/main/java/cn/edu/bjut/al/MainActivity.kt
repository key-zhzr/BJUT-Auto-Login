package cn.edu.bjut.al

import android.os.Bundle
import android.content.Context
import android.content.Intent
import android.os.Build
import android.net.Uri
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.provider.Settings
import android.net.wifi.WifiManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private var resumedFromBackground = false
  private var connectivityManager: ConnectivityManager? = null
  private var networkCallback: ConnectivityManager.NetworkCallback? = null
  private var lastNetworkEventAt = 0L
  private var lastNetworkSignature = ""

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    registerNetworkCallback()
  }

  override fun onDestroy() {
    networkCallback?.let { callback ->
      try { connectivityManager?.unregisterNetworkCallback(callback) } catch (_: Exception) {}
    }
    networkCallback = null
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

  @Synchronized
  private fun notifyNetworkChanged(signature: String) {
    val now = android.os.SystemClock.elapsedRealtime()
    if (signature == lastNetworkSignature || now - lastNetworkEventAt < 800) return
    lastNetworkSignature = signature
    lastNetworkEventAt = now
    appWebView?.post {
      appWebView?.evaluateJavascript(
        "window.__nativeNetworkChanged?.('Android NetworkCallback')",
        null
      )
    }
  }

  private fun registerNetworkCallback() {
    try {
      val manager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
      connectivityManager = manager
      val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = notifyNetworkChanged("available:${network.hashCode()}")
        override fun onLost(network: Network) = notifyNetworkChanged("lost:${network.hashCode()}")
        override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
          val signature = "cap:${network.hashCode()}:" +
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) + ":" +
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) + ":" +
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) + ":" +
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL)
          notifyNetworkChanged(signature)
        }
      }
      networkCallback = callback
      manager.registerNetworkCallback(
        NetworkRequest.Builder()
          .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
          .build(),
        callback
      )
    } catch (error: Exception) {
      error.printStackTrace()
    }
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
