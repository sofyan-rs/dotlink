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
    suspend fun findService(
        context: Context,
        service: String,
        serverId: String,
        timeoutMs: Long = 4000,
    ): Pair<String, Int>? {
        if (serverId.isEmpty()) return null
        return findService(context, service, timeoutMs) { it.attributes["id"]?.decodeToString() == serverId }
    }

    /**
     * The port of this phone's own Wireless debugging [service] (`_adb-tls-connect._tcp.` or
     * `_adb-tls-pairing._tcp.`), for casting. The PC browses for it too, but some networks
     * (a phone's hotspot) lose most multicast between their clients; here it never leaves the
     * phone.
     */
    suspend fun findOwnAdb(
        context: Context,
        service: String,
        timeoutMs: Long = 4000,
    ): Int? {
        val own =
            NetworkInterface
                .getNetworkInterfaces()
                ?.toList()
                .orEmpty()
                .flatMap { it.inetAddresses.toList() }
                .mapNotNull { it.hostAddress?.substringBefore('%') }
                .toSet()
        return findService(context, service, timeoutMs) { it.host?.hostAddress?.substringBefore('%') in own }?.second
    }

    /** Host and port of the first resolved [service] instance that [accept]s. */
    @Suppress("DEPRECATION") // resolveService: the replacement needs API 34.
    private suspend fun findService(
        context: Context,
        service: String,
        timeoutMs: Long,
        accept: (NsdServiceInfo) -> Boolean,
    ): Pair<String, Int>? {
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
                                val host = info.host?.hostAddress
                                if (host != null && accept(info)) result.complete(host to info.port)
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
