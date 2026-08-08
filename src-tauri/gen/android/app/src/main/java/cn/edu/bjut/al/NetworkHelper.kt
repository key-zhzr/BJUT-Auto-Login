package cn.edu.bjut.al

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.wifi.WifiInfo
import android.os.Build
import android.os.SystemClock
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import java.net.Inet4Address
import java.io.File
import java.security.KeyStore
import java.util.LinkedHashMap
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.concurrent.withLock
import org.json.JSONObject

private data class PhysicalNetworkSnapshot(
    val network: Network,
    val capabilities: NetworkCapabilities,
    val linkProperties: LinkProperties?
)

/**
 * Process network binding is global Android process state, so independent Rust
 * commands and the headless service must share leases instead of pairing raw
 * bindProcessToNetwork()/bindProcessToNetwork(null) calls. Leases targeting the
 * same physical network may coexist; the process is only unbound after the
 * final lease is released. A network handover waits briefly for the previous
 * users to finish.
 */
private object ProcessNetworkBindingCoordinator {
    private const val HANDOVER_TIMEOUT_MS = 5_000L
    private val lock = ReentrantLock(true)
    private val leasesChanged = lock.newCondition()
    private val leases = LinkedHashMap<String, Network>()
    private var boundNetwork: Network? = null

    fun acquire(manager: ConnectivityManager, network: Network): String {
        val deadline = SystemClock.elapsedRealtime() + HANDOVER_TIMEOUT_MS
        lock.withLock {
            while (boundNetwork != null && boundNetwork != network) {
                val remaining = deadline - SystemClock.elapsedRealtime()
                if (remaining <= 0L) return ""
                try {
                    leasesChanged.await(remaining, TimeUnit.MILLISECONDS)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    return ""
                }
            }
            if (boundNetwork == null) {
                if (!manager.bindProcessToNetwork(network)) return ""
                boundNetwork = network
            }
            return UUID.randomUUID().toString().also { token -> leases[token] = network }
        }
    }

    fun release(manager: ConnectivityManager, token: String): Boolean {
        if (token.isBlank()) return false
        lock.withLock {
            if (leases.remove(token) == null) return false
            if (leases.isNotEmpty()) return true
            val cleared = manager.bindProcessToNetwork(null)
            // Keep the logical state aligned with Android's process binding.
            // If unbinding fails, a later acquisition for another Network must
            // not assume that the process has already returned to its default
            // route. A same-Network lease may still be acquired safely and a
            // later waitAndClear() can retry the unbind.
            if (cleared) {
                boundNetwork = null
                leasesChanged.signalAll()
            }
            return cleared
        }
    }

    fun waitAndClear(manager: ConnectivityManager): Boolean {
        val deadline = SystemClock.elapsedRealtime() + HANDOVER_TIMEOUT_MS
        lock.withLock {
            while (leases.isNotEmpty()) {
                val remaining = deadline - SystemClock.elapsedRealtime()
                if (remaining <= 0L) return false
                try {
                    leasesChanged.await(remaining, TimeUnit.MILLISECONDS)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    return false
                }
            }
            val cleared = manager.bindProcessToNetwork(null)
            if (cleared) {
                boundNetwork = null
                leasesChanged.signalAll()
            }
            return cleared
        }
    }
}

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

        private fun usableIpv4(linkProperties: LinkProperties?): String = linkProperties
            ?.linkAddresses
            ?.asSequence()
            ?.map { it.address }
            ?.filterIsInstance<Inet4Address>()
            ?.firstOrNull { address ->
                val octets = address.address
                val fakeIp = octets.size == 4
                    && (octets[0].toInt() and 0xff) == 198
                    && (octets[1].toInt() and 0xff) in 18..19
                !address.isAnyLocalAddress
                    && !address.isLoopbackAddress
                    && !address.isLinkLocalAddress
                    && !address.isMulticastAddress
                    && !fakeIp
            }
            ?.hostAddress
            .orEmpty()

        private fun isCampusWiredIpv4(value: String): Boolean {
            val octets = value.split('.').mapNotNull { it.toIntOrNull() }
            if (octets.size != 4 || octets.any { it !in 0..255 }) return false
            // The wired lgn network and its gateways use 172.30/16. Keeping
            // this narrow avoids preferring an unrelated home/USB Ethernet
            // adapter merely because it has a common 10/8 or 172.16/12 IP.
            return octets[0] == 172 && octets[1] == 30
        }

        private fun physicalSnapshots(manager: ConnectivityManager): List<PhysicalNetworkSnapshot> =
            manager.allNetworks.mapNotNull { network ->
                val capabilities = manager.getNetworkCapabilities(network) ?: return@mapNotNull null
                if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                    return@mapNotNull null
                }
                PhysicalNetworkSnapshot(
                    network,
                    capabilities,
                    manager.getLinkProperties(network)
                )
            }

        private fun transportCandidateScore(
            snapshot: PhysicalNetworkSnapshot,
            activeNetwork: Network?
        ): Int {
            var score = 0
            if (snapshot.network == activeNetwork) score += 100
            if (snapshot.capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
                score += 20
            }
            if (snapshot.capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL)) {
                score += 10
            }
            if (snapshot.capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
                score += 5
            }
            if (usableIpv4(snapshot.linkProperties).isNotEmpty()) score += 2
            return score
        }

        private fun selectTransportSnapshot(
            snapshots: List<PhysicalNetworkSnapshot>,
            activeNetwork: Network?,
            transport: Int
        ): PhysicalNetworkSnapshot? = snapshots
            .asSequence()
            .filter { it.capabilities.hasTransport(transport) }
            .sortedWith(
                compareByDescending<PhysicalNetworkSnapshot> {
                    transportCandidateScore(it, activeNetwork)
                }.thenBy { it.network.toString() }
            )
            .firstOrNull()

        private fun selectPhysicalSnapshot(
            manager: ConnectivityManager,
            activeNetwork: Network?
        ): Pair<PhysicalNetworkSnapshot?, PhysicalNetworkSnapshot?> {
            val snapshots = physicalSnapshots(manager)
            val wifi = selectTransportSnapshot(
                snapshots,
                activeNetwork,
                NetworkCapabilities.TRANSPORT_WIFI
            )
            val ethernet = selectTransportSnapshot(
                snapshots,
                activeNetwork,
                NetworkCapabilities.TRANSPORT_ETHERNET
            )
            // A campus-wired link is the only valid transport for Type 3 and
            // should win even while an unrelated Wi-Fi remains associated.
            // Do not prefer an arbitrary Ethernet adapter: it must carry a
            // same-Network IPv4 from a known campus range.
            val campusEthernet = snapshots
                .asSequence()
                .filter {
                    it.capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                        && isCampusWiredIpv4(usableIpv4(it.linkProperties))
                }
                .sortedWith(
                    compareByDescending<PhysicalNetworkSnapshot> {
                        transportCandidateScore(it, activeNetwork)
                    }.thenBy { it.network.toString() }
                )
                .firstOrNull()
            val selected = campusEthernet
                ?: wifi
                ?: ethernet
                ?: selectTransportSnapshot(
                    snapshots,
                    activeNetwork,
                    NetworkCapabilities.TRANSPORT_CELLULAR
                )
                ?: snapshots.firstOrNull { it.network == activeNetwork }
            return selected to wifi
        }

        @Suppress("DEPRECATION")
        private fun wifiInfo(capabilities: NetworkCapabilities?): WifiInfo? {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
            return capabilities?.transportInfo as? WifiInfo
        }

        private fun validSsid(value: String?): String {
            val normalized = value?.removeSurrounding("\"")?.trim().orEmpty()
            return if (
                normalized.isEmpty()
                || normalized.equals("<unknown ssid>", ignoreCase = true)
                || normalized.equals("unknown", ignoreCase = true)
            ) "" else normalized
        }

        private fun validBssid(value: String?): String {
            val normalized = value?.trim()?.lowercase().orEmpty()
            return if (
                normalized.isEmpty()
                || normalized == "00:00:00:00:00:00"
                || normalized == "02:00:00:00:00:00"
            ) "" else normalized
        }

        @JvmStatic
        fun getNetworkInfo(context: Context, includeWifiDetails: Boolean): String {
            try {
                val appContext = context.applicationContext
                val connectivityManager = appContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val activeNetwork = connectivityManager.activeNetwork
                val capabilities = activeNetwork?.let(connectivityManager::getNetworkCapabilities)
                val vpnActive = connectivityManager.allNetworks.any { network ->
                    connectivityManager.getNetworkCapabilities(network)
                        ?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true
                }
                // Android may keep an unvalidated Wi-Fi associated while cellular is
                // the default Internet route. Campus authentication must describe and
                // target that Wi-Fi instead of mixing cellular capabilities with wlan0.
                val (physicalSnapshot, wifiSnapshot) = selectPhysicalSnapshot(
                    connectivityManager,
                    activeNetwork
                )
                val physicalNetwork = physicalSnapshot?.network
                val wifiNetwork = wifiSnapshot?.network
                val physicalCapabilities = physicalSnapshot?.capabilities
                val physicalLinkProperties = physicalSnapshot?.linkProperties
                val transport = when {
                    physicalCapabilities == null -> if (vpnActive) "vpn" else "none"
                    physicalCapabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
                    physicalCapabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
                    physicalCapabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
                    physicalCapabilities.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH) -> "bluetooth"
                    else -> "other"
                }
                val validated = physicalCapabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
                val defaultValidated = capabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
                val captivePortal = physicalCapabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL) == true
                val metered = physicalCapabilities
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) != true
                // A VPN can remain the app's default network while an associated
                // campus Wi-Fi is still captive/unvalidated. In that case every
                // probe and login request must bypass the VPN/mobile default and
                // target the concrete Wi-Fi Network object.
                val authenticationTransport = transport == "wifi" || transport == "ethernet"
                val routeBindingRequired = authenticationTransport
                    && physicalNetwork != null
                    && physicalNetwork != activeNetwork
                    && (!validated || captivePortal)
                var ssid = ""
                var bssid = ""
                var identityFresh = false
                var identityObservedAt = 0L
                // These LinkProperties belong to physicalNetwork, never the VPN.
                // This keeps TUN/Fake-IP addresses out of the campus login context.
                val ipString = usableIpv4(physicalLinkProperties)

                if (includeWifiDetails && transport == "wifi") {
                    // transportInfo and LinkProperties are read from the exact same
                    // NetworkCapabilities/Network pair. The legacy process-wide
                    // Wi-Fi manager API can describe a different primary connection
                    // and therefore can belong to
                    // a different Network during VPN, cellular fallback or Wi-Fi
                    // concurrency, so it must never be used for authentication.
                    val selectedWifiInfo = wifiInfo(physicalCapabilities)
                    try {
                        ssid = validSsid(selectedWifiInfo?.ssid)
                        bssid = validBssid(selectedWifiInfo?.bssid)
                    } catch (_: SecurityException) {
                        // Preserve the coherent Network/IP result while clearly
                        // marking protected Wi-Fi identity as unavailable.
                        ssid = ""
                        bssid = ""
                    }
                    identityFresh = ssid.isNotEmpty() && bssid.isNotEmpty() && ipString.isNotEmpty()
                    if (identityFresh) identityObservedAt = SystemClock.elapsedRealtime()
                }
                return JSONObject()
                    .put("ssid", ssid)
                    .put("bssid", bssid)
                    .put("ip", ipString)
                    .put("interfaceName", physicalLinkProperties?.interfaceName ?: "")
                    .put("identitySource", if (ipString.isEmpty()) "unknown" else "sameInterface")
                    .put("routeIp", ipString)
                    .put("transport", transport)
                    .put("networkId", physicalNetwork?.toString() ?: "")
                    .put("identityNetworkId", physicalNetwork?.toString() ?: "")
                    .put("defaultNetworkId", activeNetwork?.toString() ?: "")
                    .put("wifiNetworkId", wifiNetwork?.toString() ?: "")
                    .put("wifiIsDefault", wifiNetwork != null && wifiNetwork == activeNetwork)
                    .put("routeBindingRequired", routeBindingRequired)
                    .put("identityRequested", includeWifiDetails)
                    .put("identityFresh", identityFresh)
                    .put("identityObservedAt", identityObservedAt)
                    .put("vpnActive", vpnActive)
                    .put("validated", validated)
                    .put("defaultValidated", defaultValidated)
                    .put("captivePortal", captivePortal)
                    .put("metered", metered)
                    .toString()
            } catch (e: Exception) {
                return JSONObject()
                    .put("ssid", "")
                    .put("bssid", "")
                    .put("ip", "")
                    .put("interfaceName", "")
                    .put("identitySource", "unknown")
                    .put("routeIp", "")
                    .put("transport", "unknown")
                    .put("networkId", "")
                    .put("identityNetworkId", "")
                    .put("defaultNetworkId", "")
                    .put("wifiNetworkId", "")
                    .put("wifiIsDefault", false)
                    .put("routeBindingRequired", false)
                    .put("identityRequested", includeWifiDetails)
                    .put("identityFresh", false)
                    .put("identityObservedAt", 0)
                    .put("vpnActive", false)
                    .put("validated", false)
                    .put("defaultValidated", false)
                    .put("captivePortal", false)
                    .put("metered", false)
                    .toString()
            }
        }

        private fun campusAuthenticationNetwork(
            context: Context,
            expectedNetworkId: String
        ): Network? {
            if (expectedNetworkId.isBlank()) return null
            val manager = context.applicationContext
                .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
            return physicalSnapshots(manager).firstOrNull { snapshot ->
                snapshot.network.toString() == expectedNetworkId
                    && (
                        snapshot.capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
                            || snapshot.capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                    )
                    && usableIpv4(snapshot.linkProperties).isNotEmpty()
            }?.network
        }

        /** Refuse to bind if the selected physical network changed after capture. */
        @JvmStatic
        fun acquireCampusWifiBindingForNetwork(context: Context, networkId: String): String {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val physical = campusAuthenticationNetwork(context, networkId) ?: return ""
                ProcessNetworkBindingCoordinator.acquire(manager, physical)
            } catch (_: Exception) {
                ""
            }
        }

        /** Release only the lease represented by [token]. */
        @JvmStatic
        fun releaseProcessNetworkBinding(context: Context, token: String): Boolean {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                ProcessNetworkBindingCoordinator.release(manager, token)
            } catch (_: Exception) {
                false
            }
        }

        /**
         * Wait for campus-network users to leave before restoring Android's default
         * route. Billing/CAS commands should use this instead of releasing a lease
         * that belongs to a concurrent connectivity check.
         */
        @JvmStatic
        fun waitAndClearProcessNetworkBinding(context: Context): Boolean {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                ProcessNetworkBindingCoordinator.waitAndClear(manager)
            } catch (_: Exception) {
                false
            }
        }

        /** Ask Android to revalidate the same Wi-Fi/Ethernet after our own probes succeed. */
        @JvmStatic
        fun reportCampusWifiConnectivity(context: Context): Boolean {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val selected = selectPhysicalSnapshot(manager, manager.activeNetwork).first
                    ?: return false
                reportCampusWifiConnectivityForNetwork(context, selected.network.toString())
            } catch (_: Exception) {
                false
            }
        }

        @JvmStatic
        fun reportCampusWifiConnectivityForNetwork(
            context: Context,
            networkId: String
        ): Boolean {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val physical = campusAuthenticationNetwork(context, networkId) ?: return false
                manager.reportNetworkConnectivity(physical, true)
                true
            } catch (_: Exception) {
                false
            }
        }

        /** IPv4 of the same selected physical Network used by getNetworkInfo(). */
        @JvmStatic
        fun getPhysicalNetworkIp(context: Context): String {
            return try {
                val manager = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                val selected = selectPhysicalSnapshot(manager, manager.activeNetwork).first
                usableIpv4(selected?.linkProperties)
            } catch (_: Exception) {
                ""
            }
        }

        @JvmStatic
        fun getAccountHealth(context: Context): String {
            return try {
                val file = File(context.applicationContext.filesDir, "account-health.json")
                if (file.isFile) file.readText(Charsets.UTF_8) else "{}"
            } catch (_: Exception) {
                "{}"
            }
        }

        @JvmStatic
        fun setAccountHealth(context: Context, value: String): Boolean {
            return try {
                // Validate before replacing the shared file so a partial JNI value
                // cannot erase the foreground engine's cooldown history.
                JSONObject(value)
                val target = File(context.applicationContext.filesDir, "account-health.json")
                val temporary = File(target.parentFile, "${target.name}.tmp")
                temporary.writeText(value, Charsets.UTF_8)
                if (!temporary.renameTo(target)) {
                    target.writeText(value, Charsets.UTF_8)
                    temporary.delete()
                }
                true
            } catch (_: Exception) {
                false
            }
        }

    }
}
