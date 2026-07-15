package cn.edu.bjut.al

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max

class KeepAliveService : Service() {
    companion object {
        const val ACTION_SERVICE_EVENT = "cn.edu.bjut.al.SERVICE_EVENT"
        const val EXTRA_COMMAND = "command"
        const val COMMAND_NETWORK_CHANGED = "network_changed"
        const val COMMAND_CHECK = "check"
        const val COMMAND_PAUSE = "pause"
        const val COMMAND_RESUME = "resume"
        private const val ACTION_CHECK = "cn.edu.bjut.al.action.CHECK"
        private const val ACTION_PAUSE = "cn.edu.bjut.al.action.PAUSE"
        private const val ACTION_RESUME = "cn.edu.bjut.al.action.RESUME"
        private const val CHANNEL_ID = "keep_alive_channel"
        private const val NOTIFICATION_ID = 1
        private const val ENGINE_HEARTBEAT_MAX_AGE = 35_000L
        private const val IP_POLL_INTERVAL = 3_000L
        private const val SERVICE_HEARTBEAT_INTERVAL = 30_000L
        private const val MIN_CHECK_INTERVAL_SECONDS = 5L
        private const val CELLULAR_CHECK_INTERVAL_SECONDS = 300L
    }

    private val handler = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val checkInProgress = AtomicBoolean(false)
    private var connectivityManager: ConnectivityManager? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private var pausedUntil = 0L
    private var lastNetworkSignature = ""
    private var lastNetworkEventAt = 0L
    private var lastLocalIp = ""
    @Volatile private var destroyed = false
    @Volatile private var statusText = "正在初始化后台检测引擎"

    private val preferences by lazy { getSharedPreferences("service_state", Context.MODE_PRIVATE) }

    private val periodicCheckRunnable = object : Runnable {
        override fun run() {
            if (destroyed) return
            if (!isPaused() && !engineIsAlive()) {
                performHeadlessCheck("后台定时检测", false)
            }
            schedulePeriodicCheck()
        }
    }

    private val ipPollRunnable = object : Runnable {
        override fun run() {
            if (destroyed) return
            val currentIp = NetworkHelper.getLocalIpAddress()
            if (currentIp != lastLocalIp) {
                val previousIp = lastLocalIp
                lastLocalIp = currentIp
                preferences.edit().putString("last_physical_ip", currentIp).apply()
                if (previousIp.isNotEmpty() || currentIp.isNotEmpty()) {
                    KeepAliveJournal.append(
                        this@KeepAliveService,
                        "物理网络 IPv4 发生变化：${previousIp.ifEmpty { "未分配" }} -> ${currentIp.ifEmpty { "未分配" }}，触发完整检测"
                    )
                    dispatchNetworkChange("IPv4变化", true)
                }
            }
            handler.postDelayed(this, IP_POLL_INTERVAL)
        }
    }

    private val serviceHeartbeatRunnable = object : Runnable {
        override fun run() {
            if (destroyed) return
            preferences.edit().putLong("service_heartbeat", System.currentTimeMillis()).apply()
            handler.postDelayed(this, SERVICE_HEARTBEAT_INTERVAL)
        }
    }

    private val automaticResumeRunnable = Runnable {
        if (destroyed) return@Runnable
        if (pausedUntil != 0L && pausedUntil <= System.currentTimeMillis()) {
            pausedUntil = 0L
            persistPauseState()
            statusText = "自动登录已恢复"
            if (engineIsAlive()) sendCommand(COMMAND_RESUME) else performHeadlessCheck("暂停到期自动恢复", true)
            updateNotification()
            schedulePeriodicCheck(5_000L)
        }
    }

    override fun onCreate() {
        super.onCreate()
        destroyed = false
        pausedUntil = preferences.getLong("paused_until", 0L)
        preferences.edit()
            .putLong("service_heartbeat", System.currentTimeMillis())
            .putLong("service_started_at", System.currentTimeMillis())
            .apply()
        KeepAliveRestartScheduler.cancel(this)
        KeepAliveRestartScheduler.scheduleWatchdog(this)
        KeepAliveJournal.append(this, "Android 前台保活服务已创建，Rust 无界面检测核心待命")
        createNotificationChannel()
        startAsForeground()
        lastLocalIp = NetworkHelper.getLocalIpAddress()
        preferences.edit().putString("last_physical_ip", lastLocalIp).apply()
        registerNetworkCallback()
        scheduleAutomaticResume()
        schedulePeriodicCheck(12_000L)
        handler.postDelayed(ipPollRunnable, IP_POLL_INTERVAL)
        handler.post(serviceHeartbeatRunnable)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        KeepAliveRestartScheduler.scheduleWatchdog(this)
        if (intent == null) {
            KeepAliveJournal.append(this, "系统以 START_STICKY 方式重新创建了保活服务；界面心跳失效后将切换至 Rust 无界面核心", "info")
        }
        when (intent?.action) {
            ACTION_CHECK -> {
                if (engineIsAlive()) sendCommand(COMMAND_CHECK) else performHeadlessCheck("通知栏立即检测", true)
            }
            ACTION_PAUSE -> {
                pausedUntil = System.currentTimeMillis() + 60 * 60 * 1000L
                persistPauseState()
                statusText = "自动登录已暂停一小时"
                if (engineIsAlive()) sendCommand(COMMAND_PAUSE)
                KeepAliveJournal.append(this, "用户通过常驻通知暂停自动登录一小时")
                updateNotification()
                scheduleAutomaticResume()
            }
            ACTION_RESUME -> {
                pausedUntil = 0L
                persistPauseState()
                statusText = "自动登录已恢复"
                if (engineIsAlive()) sendCommand(COMMAND_RESUME) else performHeadlessCheck("通知栏恢复", true)
                KeepAliveJournal.append(this, "用户通过常驻通知恢复自动登录")
                updateNotification()
                scheduleAutomaticResume()
                schedulePeriodicCheck(5_000L)
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onTaskRemoved(rootIntent: Intent?) {
        KeepAliveJournal.append(this, "应用任务已从最近任务中移除，保活服务仍将尝试恢复", "error")
        KeepAliveRestartScheduler.schedule(this, 10_000L, "task_removed")
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        destroyed = true
        networkCallback?.let { callback ->
            try { connectivityManager?.unregisterNetworkCallback(callback) } catch (_: Exception) {}
        }
        networkCallback = null
        handler.removeCallbacks(periodicCheckRunnable)
        handler.removeCallbacks(ipPollRunnable)
        handler.removeCallbacks(serviceHeartbeatRunnable)
        handler.removeCallbacks(automaticResumeRunnable)
        preferences.edit().putLong("service_heartbeat", 0L).apply()
        KeepAliveJournal.append(this, "Android 前台保活服务 onDestroy；若并非用户关闭，将安排恢复", "error")
        KeepAliveRestartScheduler.schedule(this, 15_000L, "service_destroyed")
        worker.shutdownNow()
        super.onDestroy()
    }

    private fun startAsForeground() {
        val notification = buildNotification()
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (error: Exception) {
            KeepAliveJournal.append(this, "启动前台服务失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}", "error")
            stopSelf()
        }
    }

    private fun buildNotification(): Notification {
        val paused = isPaused()
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPendingIntent = PendingIntent.getActivity(
            this,
            10,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("BJUT Auto Login")
            .setContentText(if (paused) "自动登录已暂停" else statusText)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(openPendingIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, "立即检测", servicePendingIntent(11, ACTION_CHECK))
            .addAction(0, if (paused) "恢复" else "暂停一小时", servicePendingIntent(12, if (paused) ACTION_RESUME else ACTION_PAUSE))
            .build()
    }

    private fun servicePendingIntent(requestCode: Int, action: String): PendingIntent =
        PendingIntent.getService(
            this,
            requestCode,
            Intent(this, KeepAliveService::class.java).setAction(action),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

    private fun updateNotification() {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    private fun isPaused(): Boolean = pausedUntil > System.currentTimeMillis()

    private fun persistPauseState() {
        preferences.edit().putLong("paused_until", pausedUntil).apply()
    }

    private fun scheduleAutomaticResume() {
        handler.removeCallbacks(automaticResumeRunnable)
        val delay = pausedUntil - System.currentTimeMillis()
        if (delay > 0) handler.postDelayed(automaticResumeRunnable, delay)
        else if (pausedUntil != 0L) automaticResumeRunnable.run()
    }

    private fun engineIsAlive(): Boolean {
        val heartbeat = preferences.getLong("engine_heartbeat", 0L)
        return heartbeat > 0L && System.currentTimeMillis() - heartbeat <= ENGINE_HEARTBEAT_MAX_AGE
    }

    private fun sendCommand(command: String) {
        sendBroadcast(
            Intent(ACTION_SERVICE_EVENT)
                .setPackage(packageName)
                .putExtra(EXTRA_COMMAND, command)
        )
    }

    @Synchronized
    private fun notifyNetworkChanged(signature: String) {
        val now = SystemClock.elapsedRealtime()
        if (signature == lastNetworkSignature || now - lastNetworkEventAt < 1_500L) return
        lastNetworkSignature = signature
        lastNetworkEventAt = now
        dispatchNetworkChange("系统网络事件", false)
    }

    private fun dispatchNetworkChange(reason: String, fullDetails: Boolean) {
        if (isPaused()) return
        if (engineIsAlive()) {
            sendCommand(COMMAND_NETWORK_CHANGED)
        } else {
            performHeadlessCheck(reason, fullDetails)
        }
    }

    private fun schedulePeriodicCheck(initialDelay: Long? = null) {
        if (destroyed) return
        handler.removeCallbacks(periodicCheckRunnable)
        val delay = initialDelay ?: backgroundCheckIntervalMillis()
        handler.postDelayed(periodicCheckRunnable, delay.coerceAtLeast(5_000L))
    }

    private fun backgroundCheckIntervalMillis(): Long {
        return try {
            val config = JSONObject(NetworkHelper.getSecureConfig(this))
            val configured = max(MIN_CHECK_INTERVAL_SECONDS, config.optLong("check_interval_bg", 60L))
            val network = JSONObject(NetworkHelper.getNetworkInfo(this, false))
            val seconds = if (network.optString("transport") == "cellular") {
                max(configured, CELLULAR_CHECK_INTERVAL_SECONDS)
            } else configured
            seconds * 1000L
        } catch (_: Exception) {
            60_000L
        }
    }

    private fun networkInfoForCheck(fullDetails: Boolean): String {
        val network = JSONObject(NetworkHelper.getNetworkInfo(this, fullDetails))
        val transport = network.optString("transport")
        if (fullDetails && transport == "wifi") {
            val ssid = network.optString("ssid")
            val bssid = network.optString("bssid")
            if (ssid.isNotEmpty() && !ssid.contains("unknown", ignoreCase = true)) {
                preferences.edit().putString("last_ssid", ssid).putString("last_bssid", bssid).apply()
            }
        } else if (!fullDetails && transport == "wifi") {
            network.put("ssid", preferences.getString("last_ssid", "") ?: "")
            network.put("bssid", preferences.getString("last_bssid", "") ?: "")
        }
        val physicalIp = NetworkHelper.getLocalIpAddress()
        if (physicalIp.isNotEmpty()) network.put("ip", physicalIp)
        return network.toString()
    }

    private fun performHeadlessCheck(reason: String, fullDetails: Boolean) {
        if (destroyed || !preferences.getBoolean("auto_login_enabled", false) || isPaused()) return
        if (!checkInProgress.compareAndSet(false, true)) return
        try {
            worker.execute {
                try {
                    KeepAliveJournal.append(this, "Tauri 界面心跳缺失，Rust 无界面核心开始执行：$reason", "debug")
                    val config = NetworkHelper.getSecureConfig(this)
                    if (config.isBlank()) throw IllegalStateException("安全存储中没有应用配置")
                    val networkInfo = networkInfoForCheck(fullDetails)
                    val rawResult = NativeKeepAlive.runHeadlessCheck(config, networkInfo, reason)
                    val result = JSONObject(rawResult)
                    val logs = result.optJSONArray("logs")
                    if (logs != null) {
                        for (index in 0 until logs.length()) {
                            val item = logs.optJSONObject(index) ?: continue
                            KeepAliveJournal.append(
                                this,
                                item.optString("message", "后台检测完成"),
                                item.optString("type", "info"),
                                item.optString("module", "Android后台")
                            )
                        }
                    }
                    statusText = result.optString("notification", "后台检测已完成").take(80)
                    preferences.edit()
                        .putLong("last_headless_check", System.currentTimeMillis())
                        .putString("last_headless_status", result.optString("status", "unknown"))
                        .putString("last_headless_message", statusText)
                        .apply()
                    if (!destroyed) updateNotification()
                } catch (error: Throwable) {
                    statusText = "后台检测失败，打开应用查看日志"
                    preferences.edit()
                        .putLong("last_headless_check", System.currentTimeMillis())
                        .putString("last_headless_status", "error")
                        .putString("last_headless_message", error.message.orEmpty().take(160))
                        .apply()
                    KeepAliveJournal.append(
                        this,
                        "Rust 无界面检测失败：${error.javaClass.simpleName}: ${error.message.orEmpty()}",
                        "error"
                    )
                    if (!destroyed) updateNotification()
                } finally {
                    checkInProgress.set(false)
                    if (!destroyed) handler.post { schedulePeriodicCheck() }
                }
            }
        } catch (_: RejectedExecutionException) {
            checkInProgress.set(false)
        }
    }

    private fun registerNetworkCallback() {
        try {
            val manager = getSystemService(ConnectivityManager::class.java)
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
            manager.registerDefaultNetworkCallback(callback)
        } catch (error: Exception) {
            KeepAliveJournal.append(this, "注册系统网络回调失败：${error.javaClass.simpleName}", "error")
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "校园网后台连接",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "监听网络切换，并在界面进程退出后由 Rust 无界面核心继续检测与登录"
                setShowBadge(false)
            }
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }
}
