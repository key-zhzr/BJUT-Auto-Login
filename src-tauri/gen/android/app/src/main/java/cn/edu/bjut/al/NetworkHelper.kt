package cn.edu.bjut.al

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import java.net.NetworkInterface
import java.net.Inet4Address
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

class NetworkHelper {
    companion object {
        private const val SECURE_PREFS = "bjut_al_secure_config"
        private const val SECURE_CONFIG_KEY = "config"
        private const val SECURE_PREFS_V2 = "bjut_al_secure_config_v2"
        private const val SECURE_CONFIG_KEY_V2 = "encrypted_config"
        private const val KEY_ALIAS_V2 = "bjut_al_config_key_v2"
        private const val CONFIG_AAD = "cn.edu.bjut.al/config/v2"

        private fun securePreferences(context: Context) = EncryptedSharedPreferences.create(
            context,
            SECURE_PREFS,
            MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build(),
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )

        private fun getOrCreateV2Key(): SecretKey {
            val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            (keyStore.getKey(KEY_ALIAS_V2, null) as? SecretKey)?.let { return it }

            val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            generator.init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS_V2,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setKeySize(256)
                    .build()
            )
            return generator.generateKey()
        }

        private fun writeSecureConfigV2(context: Context, value: String): Boolean {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, getOrCreateV2Key())
            cipher.updateAAD(CONFIG_AAD.toByteArray(Charsets.UTF_8))
            val ciphertext = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            val payload = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) + "." +
                Base64.encodeToString(ciphertext, Base64.NO_WRAP)
            return context.applicationContext
                .getSharedPreferences(SECURE_PREFS_V2, Context.MODE_PRIVATE)
                .edit()
                .putString(SECURE_CONFIG_KEY_V2, payload)
                .commit()
        }

        private fun readSecureConfigV2(context: Context): String {
            val preferences = context.applicationContext
                .getSharedPreferences(SECURE_PREFS_V2, Context.MODE_PRIVATE)
            val payload = preferences.getString(SECURE_CONFIG_KEY_V2, "") ?: ""
            if (payload.isEmpty()) return ""
            val parts = payload.split('.', limit = 2)
            require(parts.size == 2) { "Invalid secure configuration payload" }
            val iv = Base64.decode(parts[0], Base64.NO_WRAP)
            val ciphertext = Base64.decode(parts[1], Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, getOrCreateV2Key(), GCMParameterSpec(128, iv))
            cipher.updateAAD(CONFIG_AAD.toByteArray(Charsets.UTF_8))
            return String(cipher.doFinal(ciphertext), Charsets.UTF_8)
        }

        private fun legacyPreferencesExist(context: Context): Boolean {
            val preferencesDir = File(context.applicationInfo.dataDir, "shared_prefs")
            return File(preferencesDir, "$SECURE_PREFS.xml").isFile
        }

        @JvmStatic
        fun getSecureConfig(context: Context): String {
            val v2Preferences = context.applicationContext
                .getSharedPreferences(SECURE_PREFS_V2, Context.MODE_PRIVATE)
            val hasV2Payload = !v2Preferences
                .getString(SECURE_CONFIG_KEY_V2, "")
                .isNullOrEmpty()
            // Once v2 exists it is authoritative. A decrypt/Keystore failure
            // must propagate to Rust; falling back would restore a stale legacy
            // password over a newer v2 value.
            if (hasV2Payload) return readSecureConfigV2(context)

            // One-time compatibility path for releases using
            // EncryptedSharedPreferences/Tink.
            if (legacyPreferencesExist(context)) {
                try {
                    val legacy = securePreferences(context.applicationContext)
                        .getString(SECURE_CONFIG_KEY, "") ?: ""
                    if (legacy.isNotEmpty()) {
                        check(writeSecureConfigV2(context, legacy)) {
                            "Unable to persist migrated secure configuration"
                        }
                        context.applicationContext.deleteSharedPreferences(SECURE_PREFS)
                        return legacy
                    }
                } catch (legacyError: Exception) {
                    throw IllegalStateException(
                        "Unable to read the legacy secure configuration",
                        legacyError
                    )
                }
            }
            return ""
        }

        @JvmStatic
        fun setSecureConfig(context: Context, value: String): Boolean {
            return try {
                writeSecureConfigV2(context, value)
            } catch (error: Exception) {
                false
            }
        }

        @JvmStatic
        fun getNetworkInfo(context: Context, includeWifiDetails: Boolean): String {
            try {
                val appContext = context.applicationContext
                val connectivityManager = appContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val activeNetwork = connectivityManager.activeNetwork
                val capabilities = activeNetwork?.let(connectivityManager::getNetworkCapabilities)
                val transport = when {
                    capabilities == null -> "none"
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> "vpn"
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH) -> "bluetooth"
                    else -> "other"
                }
                val validated = capabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
                val captivePortal = capabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL) == true
                val metered = connectivityManager.isActiveNetworkMetered
                var ssid = ""
                var bssid = ""
                // LinkProperties belongs to the active routed network. When a VPN is
                // enabled it may expose the TUN/Fake-IP address instead of the Wi-Fi
                // or cellular interface address. Reuse the physical-interface lookup
                // used by the change detector so identity checks see the real LAN IP.
                var ipString = getLocalIpAddress()

                if (includeWifiDetails && transport == "wifi") {
                    val wifiManager = appContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                    val wifiInfo = wifiManager.connectionInfo
                    ssid = wifiInfo.ssid?.removeSurrounding("\"") ?: "Unknown"
                    bssid = wifiInfo.bssid ?: "00:00:00:00:00:00"
                    // WifiInfo reports the address assigned to the physical Wi-Fi
                    // interface and is therefore preferred over any routed/VPN IP.
                    val ipAddress = wifiInfo.ipAddress
                    if (ipAddress != 0) {
                        ipString = String.format(
                            "%d.%d.%d.%d",
                            ipAddress and 0xff,
                            ipAddress shr 8 and 0xff,
                            ipAddress shr 16 and 0xff,
                            ipAddress shr 24 and 0xff
                        )
                    }
                }
                return JSONObject()
                    .put("ssid", ssid)
                    .put("bssid", bssid)
                    .put("ip", ipString)
                    .put("transport", transport)
                    .put("validated", validated)
                    .put("captivePortal", captivePortal)
                    .put("metered", metered)
                    .toString()
            } catch (e: Exception) {
                return JSONObject()
                    .put("ssid", "")
                    .put("bssid", "")
                    .put("ip", "")
                    .put("transport", "unknown")
                    .put("validated", false)
                    .put("captivePortal", false)
                    .put("metered", false)
                    .toString()
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
