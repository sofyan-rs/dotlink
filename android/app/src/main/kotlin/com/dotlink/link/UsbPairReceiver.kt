package com.dotlink.link

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.dotlink.DotlinkApp

/**
 * The pairing link the PC on the USB cable sends back after the pair screen asked for one
 * ([Link.requestUsbPairing]), as `adb shell am broadcast`. A broadcast, not an Activity link:
 * the PC sends it to every phone on the cable, and only the one that asked (its nonce in the
 * link) does anything; the others stay as they are.
 */
class UsbPairReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        val link = intent.getStringExtra(EXTRA_LINK) ?: return
        DotlinkApp.link.offerUsbPairLink(link)
    }

    companion object {
        const val ACTION = "com.dotlink.android.USB_PAIR"
        const val EXTRA_LINK = "link"
    }
}
