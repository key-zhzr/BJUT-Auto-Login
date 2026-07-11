package cn.edu.bjut.al

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat

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
    }

    private var connectivityManager: ConnectivityManager? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private val handler = Handler(Looper.getMainLooper())
    private var pausedUntil = 0L
    private var lastNetworkSignature = ""
    private var lastNetworkEventAt = 0L

    override fun onCreate() {
        super.onCreate()
        pausedUntil = getSharedPreferences("service_state", MODE_PRIVATE).getLong("paused_until", 0L)
        createNotificationChannel()
        startAsForeground()
        registerNetworkCallback()
        scheduleAutomaticResume()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_CHECK -> sendCommand(COMMAND_CHECK)
            ACTION_PAUSE -> {
                pausedUntil = System.currentTimeMillis() + 60 * 60 * 1000L
                persistPauseState()
                sendCommand(COMMAND_PAUSE)
                updateNotification()
                scheduleAutomaticResume()
            }
            ACTION_RESUME -> {
                pausedUntil = 0L
                persistPauseState()
                sendCommand(COMMAND_RESUME)
                updateNotification()
                handler.removeCallbacksAndMessages(null)
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        networkCallback?.let { callback ->
            try { connectivityManager?.unregisterNetworkCallback(callback) } catch (_: Exception) {}
        }
        networkCallback = null
        handler.removeCallbacksAndMessages(null)
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
            error.printStackTrace()
            stopSelf()
        }
    }

    private fun buildNotification(): Notification {
        val paused = pausedUntil > System.currentTimeMillis()
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPendingIntent = PendingIntent.getActivity(
            this,
            10,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val checkPendingIntent = servicePendingIntent(11, ACTION_CHECK)
        val pausePendingIntent = servicePendingIntent(12, if (paused) ACTION_RESUME else ACTION_PAUSE)
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("BJUT Auto Login")
            .setContentText(if (paused) "自动登录已暂停" else "通过系统网络事件保持校园网连接")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(openPendingIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, "立即检测", checkPendingIntent)
            .addAction(0, if (paused) "恢复" else "暂停一小时", pausePendingIntent)
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
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification())
    }

    private fun persistPauseState() {
        getSharedPreferences("service_state", MODE_PRIVATE)
            .edit()
            .putLong("paused_until", pausedUntil)
            .apply()
    }

    private fun scheduleAutomaticResume() {
        handler.removeCallbacksAndMessages(null)
        val delay = pausedUntil - System.currentTimeMillis()
        if (delay <= 0) {
            if (pausedUntil != 0L) {
                pausedUntil = 0L
                persistPauseState()
                sendCommand(COMMAND_RESUME)
                updateNotification()
            }
            return
        }
        handler.postDelayed({
            pausedUntil = 0L
            persistPauseState()
            sendCommand(COMMAND_RESUME)
            updateNotification()
        }, delay)
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
        val now = android.os.SystemClock.elapsedRealtime()
        if (signature == lastNetworkSignature || now - lastNetworkEventAt < 800) return
        lastNetworkSignature = signature
        lastNetworkEventAt = now
        sendCommand(COMMAND_NETWORK_CHANGED)
    }

    private fun registerNetworkCallback() {
        try {
            val manager = getSystemService(ConnectivityManager::class.java)
            connectivityManager = manager
            val callback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) =
                    notifyNetworkChanged("available:${network.hashCode()}")

                override fun onLost(network: Network) =
                    notifyNetworkChanged("lost:${network.hashCode()}")

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
            error.printStackTrace()
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "校园网后台连接",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "监听网络切换并触发校园网连接检查"
                setShowBadge(false)
            }
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }
}
