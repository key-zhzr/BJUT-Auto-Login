package cn.edu.bjut.al

import android.content.Context
import android.net.wifi.WifiManager

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
    }
}
