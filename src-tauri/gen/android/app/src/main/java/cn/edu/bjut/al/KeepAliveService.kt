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
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max

private data class AndroidNotificationSettings(
    val mode: String = "combined",
    val networkStatus: Boolean = true,
    val loginResults: Boolean = true,
    val backgroundErrors: Boolean = true
) {
    val separated: Boolean get() = mode == "separate"
}

class KeepAliveService : Service() {
    companion object {
        const val ACTION_SERVICE_EVENT = "cn.edu.bjut.al.SERVICE_EVENT"
        const val EXTRA_COMMAND = "command"
        const val COMMAND_NETWORK_CHANGED = "network_changed"
        const val COMMAND_CHECK = "check"
        const val COMMAND_PAUSE = "pause"
        const val COMMAND_RESUME = "resume"
        const val ACTION_UPDATE_STATUS = "cn.edu.bjut.al.action.UPDATE_STATUS"
        const val ACTION_REFRESH_NOTIFICATION_SETTINGS = "cn.edu.bjut.al.action.REFRESH_NOTIFICATION_SETTINGS"
        const val EXTRA_STATUS_TEXT = "status_text"
        private const val ACTION_CHECK = "cn.edu.bjut.al.action.CHECK"
        private const val ACTION_PAUSE = "cn.edu.bjut.al.action.PAUSE"
        private const val ACTION_RESUME = "cn.edu.bjut.al.action.RESUME"
        private const val ACTION_TOGGLE_PAUSE = "cn.edu.bjut.al.action.TOGGLE_PAUSE"
        private const val CHANNEL_ID = "keep_alive_channel"
        private const val NETWORK_CHANNEL_ID = "network_status_channel"
        private const val LOGIN_CHANNEL_ID = "login_result_channel"
        private const val BACKGROUND_CHANNEL_ID = "background_error_channel"
        private const val NOTIFICATION_ID = 1
        private const val NETWORK_NOTIFICATION_ID = 101
        private const val LOGIN_NOTIFICATION_ID = 102
        private const val BACKGROUND_NOTIFICATION_ID = 103
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
    @Volatile private var notificationSettings = AndroidNotificationSettings()
    private val lastEventMessages = ConcurrentHashMap<String, String>()
    @Volatile private var lastStatusCategory = "network"
    @Volatile private var destroyed = false
    @Volatile private var statusText = "正在初始化后台检测引擎"

    private val preferences by lazy { getSharedPreferences("service_state", Context.MODE_PRIVATE) }

    private val periodicCheckRunnable = object : Runnable {
        override fun run() {
            if (destroyed) return
            if (!isPaused() && !interfaceIsForeground()) {
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
            if (interfaceIsForeground()) sendCommand(COMMAND_RESUME) else performHeadlessCheck("暂停到期自动恢复", true)
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
        notificationSettings = loadNotificationSettings()
        createNotificationChannels()
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
            ACTION_UPDATE_STATUS -> {
                val message = intent.getStringExtra(EXTRA_STATUS_TEXT)?.trim().orEmpty().take(80)
                if (message.isNotEmpty()) {
                    publishStatus("network", message)
                }
            }
            ACTION_REFRESH_NOTIFICATION_SETTINGS -> refreshNotificationSettings()
            ACTION_CHECK -> {
                val useInterfaceEngine = interfaceIsForeground()
                KeepAliveJournal.append(
                    this,
                    "用户通过常驻通知触发立即检测；已交给${if (useInterfaceEngine) "界面核心" else "Rust 无界面核心"}"
                )
                if (useInterfaceEngine) sendCommand(COMMAND_CHECK) else performHeadlessCheck("通知栏立即检测", true)
            }
            ACTION_PAUSE -> pauseAutomaticLogin()
            ACTION_RESUME -> resumeAutomaticLogin()
            ACTION_TOGGLE_PAUSE -> if (isPaused()) resumeAutomaticLogin() else pauseAutomaticLogin()
        }
        return START_STICKY
    }

    private fun pauseAutomaticLogin() {
        pausedUntil = System.currentTimeMillis() + 60 * 60 * 1000L
        persistPauseState()
        statusText = "自动登录已暂停一小时"
        if (interfaceIsForeground()) sendCommand(COMMAND_PAUSE)
        KeepAliveJournal.append(this, "用户通过常驻通知暂停自动登录一小时")
        updateNotification()
        scheduleAutomaticResume()
    }

    private fun resumeAutomaticLogin() {
        pausedUntil = 0L
        persistPauseState()
        statusText = "自动登录已恢复"
        if (interfaceIsForeground()) sendCommand(COMMAND_RESUME) else performHeadlessCheck("通知栏恢复", true)
        KeepAliveJournal.append(this, "用户通过常驻通知恢复自动登录")
        updateNotification()
        scheduleAutomaticResume()
        schedulePeriodicCheck(5_000L)
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onTaskRemoved(rootIntent: Intent?) {
        KeepAliveJournal.append(this, "应用任务已从最近任务中移除，保活服务仍将尝试恢复")
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
        val separated = notificationSettings.separated
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
            .setContentText(
                if (separated) "后台自动登录保活服务正在运行"
                else if (paused) "自动登录已暂停"
                else statusText
            )
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(openPendingIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setUsesChronometer(false)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, "立即检测", servicePendingIntent(11, ACTION_CHECK))
            .addAction(
                0,
                if (separated) "暂停/恢复" else if (paused) "恢复" else "暂停一小时",
                servicePendingIntent(12, if (separated) ACTION_TOGGLE_PAUSE else if (paused) ACTION_RESUME else ACTION_PAUSE)
            )
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
        if (notificationSettings.separated) return
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    private fun loadNotificationSettings(): AndroidNotificationSettings {
        return try {
            val config = JSONObject(NetworkHelper.getSecureConfig(this))
            AndroidNotificationSettings(
                mode = if (config.optString("android_notification_mode") == "separate") "separate" else "combined",
                networkStatus = config.optBoolean("android_notify_network_status", true),
                loginResults = config.optBoolean("android_notify_login_results", true),
                backgroundErrors = config.optBoolean("android_notify_background_errors", true)
            )
        } catch (error: Exception) {
            KeepAliveJournal.append(
                this,
                "读取 Android 通知设置失败，沿用兼容模式：${error.javaClass.simpleName}",
                "error"
            )
            AndroidNotificationSettings()
        }
    }

    private fun refreshNotificationSettings() {
        val previous = notificationSettings
        val updated = loadNotificationSettings()
        notificationSettings = updated
        val manager = getSystemService(NotificationManager::class.java)
        if (previous.networkStatus != updated.networkStatus) lastEventMessages.remove("network")
        if (previous.loginResults != updated.loginResults) lastEventMessages.remove("login")
        if (previous.backgroundErrors != updated.backgroundErrors) lastEventMessages.remove("background")
        if (!updated.networkStatus || !updated.separated) manager.cancel(NETWORK_NOTIFICATION_ID)
        if (!updated.loginResults || !updated.separated) manager.cancel(LOGIN_NOTIFICATION_ID)
        if (!updated.backgroundErrors || !updated.separated) manager.cancel(BACKGROUND_NOTIFICATION_ID)
        if (previous.mode != updated.mode) {
            lastEventMessages.clear()
            manager.notify(NOTIFICATION_ID, buildNotification())
            KeepAliveJournal.append(
                this,
                if (updated.separated) "通知方式已切换为保活与信息分离；常驻通知后续不再更新"
                else "通知方式已切换为保活与信息共存"
            )
        } else if (!updated.separated && !notificationCategoryEnabled(lastStatusCategory)) {
            statusText = "后台自动登录保活服务正在运行"
            manager.notify(NOTIFICATION_ID, buildNotification())
        }
    }

    private fun notificationCategoryEnabled(category: String): Boolean = when (category) {
        "login" -> notificationSettings.loginResults
        "background" -> notificationSettings.backgroundErrors
        else -> notificationSettings.networkStatus
    }

    private fun publishStatus(category: String, message: String) {
        lastStatusCategory = category
        statusText = message.take(80)
        if (!notificationCategoryEnabled(category)) return
        if (notificationSettings.separated) showEventNotification(category, statusText)
        else updateNotification()
    }

    private fun showEventNotification(category: String, message: String) {
        if (lastEventMessages[category] == message) return
        val channelId: String
        val notificationId: Int
        val title: String
        val priority: Int
        when (category) {
            "login" -> {
                channelId = LOGIN_CHANNEL_ID
                notificationId = LOGIN_NOTIFICATION_ID
                title = "校园网自动登录"
                priority = NotificationCompat.PRIORITY_DEFAULT
            }
            "background" -> {
                channelId = BACKGROUND_CHANNEL_ID
                notificationId = BACKGROUND_NOTIFICATION_ID
                title = "后台检测异常"
                priority = NotificationCompat.PRIORITY_DEFAULT
            }
            else -> {
                channelId = NETWORK_CHANNEL_ID
                notificationId = NETWORK_NOTIFICATION_ID
                title = "校园网网络状态"
                priority = NotificationCompat.PRIORITY_LOW
            }
        }
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPendingIntent = PendingIntent.getActivity(
            this,
            20 + notificationId,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(this, channelId)
            .setContentTitle(title)
            .setContentText(message)
            .setStyle(NotificationCompat.BigTextStyle().bigText(message))
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(openPendingIntent)
            .setPriority(priority)
            .setAutoCancel(true)
            .setOnlyAlertOnce(false)
            .setCategory(
                if (category == "background") NotificationCompat.CATEGORY_ERROR
                else NotificationCompat.CATEGORY_STATUS
            )
            .build()
        getSystemService(NotificationManager::class.java).notify(notificationId, notification)
        lastEventMessages[category] = message
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

    private fun interfaceIsForeground(): Boolean =
        engineIsAlive() && preferences.getBoolean("engine_foreground", false)

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
        if (interfaceIsForeground()) {
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
                    val engineState = if (engineIsAlive()) "Tauri 界面处于后台" else "Tauri 界面心跳缺失"
                    KeepAliveJournal.append(this, "$engineState，Rust 无界面核心开始执行：$reason", "debug")
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
                    val notificationCategory = result.optString("notification_category", "network")
                    statusText = result.optString("notification", "后台检测已完成").take(80)
                    preferences.edit()
                        .putLong("last_headless_check", System.currentTimeMillis())
                        .putString("last_headless_status", result.optString("status", "unknown"))
                        .putString("last_headless_message", statusText)
                        .apply()
                    if (!destroyed) publishStatus(notificationCategory, statusText)
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
                    if (!destroyed) publishStatus("background", statusText)
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

    private fun createNotificationChannels() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channels = listOf(
                NotificationChannel(
                    CHANNEL_ID,
                    "校园网后台连接",
                    NotificationManager.IMPORTANCE_LOW
                ).apply {
                    description = "维持校园网后台检测服务；分离模式下该通知保持静态"
                    setShowBadge(false)
                },
                NotificationChannel(
                    NETWORK_CHANNEL_ID,
                    "校园网网络状态",
                    NotificationManager.IMPORTANCE_LOW
                ).apply {
                    description = "联网、离线、移动数据及校园网待认证状态"
                    setShowBadge(false)
                },
                NotificationChannel(
                    LOGIN_CHANNEL_ID,
                    "校园网自动登录结果",
                    NotificationManager.IMPORTANCE_DEFAULT
                ).apply {
                    description = "自动登录成功、失败、安全阻止及账号不可用"
                    setShowBadge(false)
                },
                NotificationChannel(
                    BACKGROUND_CHANNEL_ID,
                    "校园网后台异常",
                    NotificationManager.IMPORTANCE_DEFAULT
                ).apply {
                    description = "后台检测核心或保活服务运行异常"
                    setShowBadge(false)
                }
            )
            getSystemService(NotificationManager::class.java).createNotificationChannels(channels)
        }
    }
}
