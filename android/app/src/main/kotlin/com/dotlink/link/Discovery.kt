package com.dotlink.link

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeoutOrNull

/** Finds the paired PC (or a paired phone) on the LAN through mDNS, matching its id. */
object Discovery {
    private const val SERVICE = "_dotlink._tcp."

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
