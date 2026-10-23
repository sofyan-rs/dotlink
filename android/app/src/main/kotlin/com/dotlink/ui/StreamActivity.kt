package com.dotlink.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Invisible activity behind the "Start" button of the PC's webcam/mic request. Being on screen
 * is what lets Android start the camera or mic, so it stays until the stream is running.
 */
class StreamActivity : ComponentActivity() {
    private val kind get() = intent.getStringExtra(EXTRA_KIND) ?: "camera"

    private val permission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) start() else finish()
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val p = if (kind == "camera") Manifest.permission.CAMERA else Manifest.permission.RECORD_AUDIO
        if (ContextCompat.checkSelfPermission(this, p) == PackageManager.PERMISSION_GRANTED) start() else permission.launch(p)
    }

    private fun start() {
        val link = DotlinkApp.link
        val camera = kind == "camera"
        LinkService.send(this, if (camera) LinkService.ACTION_CAMERA_ON else LinkService.ACTION_MIC_ON)
        lifecycleScope.launch {
            val running = withTimeoutOrNull(5_000) { (if (camera) link.cameraOn else link.micOn).first { it } }
            if (running == null) Toast.makeText(this@StreamActivity, "Couldn't start", Toast.LENGTH_SHORT).show()
            finish()
        }
    }

    companion object {
        private const val EXTRA_KIND = "kind"

        fun intent(
            context: Context,
            kind: String,
        ) = Intent(context, StreamActivity::class.java)
            .putExtra(EXTRA_KIND, kind)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
}
