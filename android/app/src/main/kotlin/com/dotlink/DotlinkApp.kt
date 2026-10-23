package com.dotlink

import android.app.Activity
import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.Bundle
import com.dotlink.link.Link

class DotlinkApp : Application() {
    /** Activities between onStart and onStop. */
    private var started = 0

    override fun onCreate() {
        super.onCreate()
        link = Link(this)
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL_LINK, "Connection", NotificationManager.IMPORTANCE_MIN))
        nm.createNotificationChannel(NotificationChannel(CHANNEL_FILES, "Received files", NotificationManager.IMPORTANCE_DEFAULT))
        nm.createNotificationChannel(NotificationChannel(CHANNEL_TRANSFERS, "File transfers", NotificationManager.IMPORTANCE_LOW))
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_REQUESTS, "Requests from the PC", NotificationManager.IMPORTANCE_HIGH),
        )
        registerActivityLifecycleCallbacks(
            object : ActivityLifecycleCallbacks {
                override fun onActivityStarted(activity: Activity) {
                    started++
                }

                override fun onActivityStopped(activity: Activity) {
                    started--
                }

                override fun onActivityCreated(
                    activity: Activity,
                    savedInstanceState: Bundle?,
                ) = Unit

                override fun onActivityResumed(activity: Activity) = Unit

                override fun onActivityPaused(activity: Activity) = Unit

                override fun onActivitySaveInstanceState(
                    activity: Activity,
                    outState: Bundle,
                ) = Unit

                override fun onActivityDestroyed(activity: Activity) = Unit
            },
        )
        instance = this
    }

    companion object {
        const val CHANNEL_LINK = "link"
        const val CHANNEL_FILES = "files"

        /** Progress of files going to or coming from the PC. */
        const val CHANNEL_TRANSFERS = "transfers"

        /** "Start the webcam?" when the PC asks while the app is in the background. */
        const val CHANNEL_REQUESTS = "requests"

        private lateinit var instance: DotlinkApp

        /**
         * One of our activities is on screen. Android only lets the app start the camera or mic
         * then (or from a notification tap that opens an activity).
         */
        val inForeground: Boolean get() = instance.started > 0

        /** The one connection to the paired PC. */
        lateinit var link: Link
            private set
    }
}
