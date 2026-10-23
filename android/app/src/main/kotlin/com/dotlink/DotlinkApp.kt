package com.dotlink

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import com.dotlink.link.Link

class DotlinkApp : Application() {
    override fun onCreate() {
        super.onCreate()
        link = Link(this)
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL_LINK, "Connection", NotificationManager.IMPORTANCE_MIN))
        nm.createNotificationChannel(NotificationChannel(CHANNEL_FILES, "Received files", NotificationManager.IMPORTANCE_DEFAULT))
    }

    companion object {
        const val CHANNEL_LINK = "link"
        const val CHANNEL_FILES = "files"

        /** The one connection to the paired PC. */
        lateinit var link: Link
            private set
    }
}
