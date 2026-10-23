package com.dotlink.link

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeoutOrNull

/** Finds the paired PC on the LAN through mDNS (`_dotlink._tcp`), matching its server id. */
object Discovery {
    private const val SERVICE = "_dotlink._tcp."

    @Suppress("DEPRECATION") // resolveService: the replacement needs API 34.
    suspend fun find(
        context: Context,
        serverId: String,
        timeoutMs: Long = 4000,
    ): String? {
        if (serverId.isEmpty()) return null
        val nsd = context.getSystemService(NsdManager::class.java)
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
        val lock = wifi.createMulticastLock("dotlink-discovery").apply { setReferenceCounted(false) }
        val result = CompletableDeferred<String?>()
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
                                if (id == serverId && host != null) result.complete(host)
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
            nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, listener)
            withTimeoutOrNull(timeoutMs) { result.await() }
        } finally {
            runCatching { nsd.stopServiceDiscovery(listener) }
            lock.release()
        }
    }
}
