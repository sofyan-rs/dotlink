package com.dotlink.link

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.Socket
import java.nio.ByteBuffer

/** Finds the paired PC (or a paired phone) on the LAN through mDNS, matching its id. */
object Discovery {
    private const val SERVICE = "_dotlink._tcp."

    /** How long one address gets to accept a connection in [scan]. */
    private const val SCAN_CONNECT_MS = 300

    /** Addresses tried at once in [scan]. */
    private const val SCAN_PARALLEL = 64

    /**
     * Addresses on the phone's own IPv4 networks (its Wi-Fi, the hotspot it shares) that accept
     * a connection on [port], for where mDNS doesn't reach: Android browses only the Wi-Fi the
     * phone is on, not its own hotspot, and some networks drop multicast. Only /24 networks and
     * smaller are tried (254 addresses each, a second or two). Callers check each address with
     * the pinned certificate before using it.
     */
    suspend fun scan(port: Int): List<String> =
        withContext(Dispatchers.IO) {
            val targets = localNetworks().flatMap { (own, base) -> (1..254).map { base or it }.filter { it != own } }
            targets
                .chunked(SCAN_PARALLEL)
                .flatMap { chunk -> chunk.map { ip -> async { ip.takeIf { accepts(it, port) } } }.awaitAll() }
                .filterNotNull()
                .map { InetAddress.getByAddress(ByteBuffer.allocate(4).putInt(it).array()).hostAddress!! }
        }

    /** (this phone's address, the /24 it is in) for each private IPv4 network it is on. */
    private fun localNetworks(): List<Pair<Int, Int>> =
        NetworkInterface
            .getNetworkInterfaces()
            ?.toList()
            .orEmpty()
            // Mobile data and VPNs: not where a PC on the same network can be.
            .filter { nic ->
                nic.isUp && !nic.isLoopback && listOf("rmnet", "ccmni", "tun").none { nic.name.startsWith(it) }
            }
            .flatMap { it.interfaceAddresses }
            .mapNotNull { a ->
                val ip = a.address as? Inet4Address
                if (ip == null || !ip.isSiteLocalAddress || a.networkPrefixLength < 24) return@mapNotNull null
                val own = ByteBuffer.wrap(ip.address).int
                own to (own and 0xFFFFFF00.toInt())
            }.distinctBy { it.second }

    private fun accepts(
        ip: Int,
        port: Int,
    ): Boolean =
        try {
            val address = InetAddress.getByAddress(ByteBuffer.allocate(4).putInt(ip).array())
            Socket().use { it.connect(InetSocketAddress(address, port), SCAN_CONNECT_MS) }
            true
        } catch (_: Exception) {
            false
        }

    /** The PC's address (`_dotlink._tcp`, its server id). */
    suspend fun find(
        context: Context,
        serverId: String,
        timeoutMs: Long = 4000,
    ): String? = findService(context, SERVICE, serverId, timeoutMs)?.first

    /** Host and port of the [service] instance whose TXT `id` is [serverId]. */
    @Suppress("DEPRECATION") // resolveService: the replacement needs API 34.
    suspend fun findService(
        context: Context,
        service: String,
        serverId: String,
        timeoutMs: Long = 4000,
    ): Pair<String, Int>? {
        if (serverId.isEmpty()) return null
        val nsd = context.getSystemService(NsdManager::class.java)
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
        val lock = wifi.createMulticastLock("dotlink-discovery").apply { setReferenceCounted(false) }
        val result = CompletableDeferred<Pair<String, Int>?>()
        val listener =
            object : NsdManager.DiscoveryListener {
                override fun onServiceFound(info: NsdServiceInfo) {
                    nsd.resolveService(
                        info,
                        object : NsdManager.ResolveListener {
                            override fun onResolveFailed(
                                info: NsdServiceInfo,
                                errorCode: Int,
                            ) = Unit

                            override fun onServiceResolved(info: NsdServiceInfo) {
                                val id = info.attributes["id"]?.decodeToString()
                                val host = info.host?.hostAddress
                                if (id == serverId && host != null) result.complete(host to info.port)
                            }
                        },
                    )
                }

                override fun onStartDiscoveryFailed(
                    serviceType: String,
                    errorCode: Int,
                ) {
                    result.complete(null)
                }

                override fun onServiceLost(info: NsdServiceInfo) = Unit

                override fun onDiscoveryStarted(serviceType: String) = Unit

                override fun onDiscoveryStopped(serviceType: String) = Unit

                override fun onStopDiscoveryFailed(
                    serviceType: String,
                    errorCode: Int,
                ) = Unit
            }
        lock.acquire()
        return try {
            nsd.discoverServices(service, NsdManager.PROTOCOL_DNS_SD, listener)
            withTimeoutOrNull(timeoutMs) { result.await() }
        } finally {
            runCatching { nsd.stopServiceDiscovery(listener) }
            lock.release()
        }
    }
}
