package cn.edu.bjut.al

import android.content.Context
import android.net.wifi.WifiManager
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import java.net.NetworkInterface
import java.net.Inet4Address

class NetworkHelper {
    companion object {
        private const val SECURE_PREFS = "bjut_al_secure_config"
        private const val SECURE_CONFIG_KEY = "config"

        private fun securePreferences(context: Context) = EncryptedSharedPreferences.create(
            context,
            SECURE_PREFS,
            MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build(),
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )

        @JvmStatic
        fun getSecureConfig(context: Context): String {
            return try {
                securePreferences(context.applicationContext).getString(SECURE_CONFIG_KEY, "") ?: ""
            } catch (e: Exception) {
                ""
            }
        }

        @JvmStatic
        fun setSecureConfig(context: Context, value: String): Boolean {
            return try {
                securePreferences(context.applicationContext).edit().putString(SECURE_CONFIG_KEY, value).commit()
            } catch (e: Exception) {
                false
            }
        }

        @JvmStatic
        fun getNetworkInfo(context: Context): String {
            try {
                val wifiManager = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                val wifiInfo = wifiManager.connectionInfo
                val ssid = wifiInfo.ssid?.removeSurrounding("\"") ?: "Unknown"
                val bssid = wifiInfo.bssid ?: "00:00:00:00:00:00"
                val ipAddress = wifiInfo.ipAddress
                val ipString = String.format("%d.%d.%d.%d", ipAddress and 0xff, ipAddress shr 8 and 0xff, ipAddress shr 16 and 0xff, ipAddress shr 24 and 0xff)
                return "{\"ssid\":\"$ssid\",\"bssid\":\"$bssid\",\"ip\":\"$ipString\"}"
            } catch (e: Exception) {
                return "{\"ssid\":\"\",\"bssid\":\"\",\"ip\":\"\"}"
            }
        }

        @JvmStatic
        fun getLocalIpAddress(): String {
            try {
                val interfaces = NetworkInterface.getNetworkInterfaces()
                var bestIp = ""
                while (interfaces.hasMoreElements()) {
                    val iface = interfaces.nextElement()
                    if (iface.isLoopback || !iface.isUp) continue
                    val ifaceName = iface.name.lowercase()
                    if (ifaceName.contains("tun") || ifaceName.contains("tap")) continue
                    
                    val addresses = iface.inetAddresses
                    while (addresses.hasMoreElements()) {
                        val addr = addresses.nextElement()
                        if (addr is Inet4Address) {
                            val ip = addr.hostAddress ?: ""
                            if (ip.isNotEmpty() && !ip.startsWith("127.") && !ip.startsWith("198.18.")) {
                                if (ifaceName.contains("wlan")) {
                                    return ip
                                }
                                if (bestIp.isEmpty()) {
                                    bestIp = ip
                                }
                            }
                        }
                    }
                }
                return bestIp
            } catch (e: Exception) {}
            return ""
        }
    }
}
