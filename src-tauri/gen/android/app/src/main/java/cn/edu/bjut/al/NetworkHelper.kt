package cn.edu.bjut.al

import android.content.Context
import android.net.wifi.WifiManager
import java.net.NetworkInterface
import java.net.Inet4Address

class NetworkHelper {
    companion object {
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
                while (interfaces.hasMoreElements()) {
                    val iface = interfaces.nextElement()
                    if (iface.isLoopback || !iface.isUp) continue
                    val addresses = iface.inetAddresses
                    while (addresses.hasMoreElements()) {
                        val addr = addresses.nextElement()
                        if (addr is Inet4Address) {
                            val ip = addr.hostAddress ?: ""
                            if (ip.isNotEmpty() && !ip.startsWith("127.") && !ip.startsWith("198.18.")) {
                                return ip
                            }
                        }
                    }
                }
            } catch (e: Exception) {}
            return ""
        }
    }
}
