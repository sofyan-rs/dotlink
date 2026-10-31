package com.dotlink.link

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Environment
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.dotlink.DotlinkApp
import com.dotlink.R
import com.dotlink.ui.MainActivity
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Browse from PC: the paired PC lists, downloads, uploads into, renames and deletes files in
 * this phone's storage (`fs_*` messages, see the PC's `files.rs`). Off by default; it needs
 * "All files access" (Android 11+) or storage permission (Android 10). Every path is resolved
 * under the storage root and `Android/data`, `Android/obb` stay hidden. Only the PC link reaches
 * here: linked phones have no such messages.
 */
object FileBrowser {
    private const val TAG = "Dotlink"
    private const val NOTIFICATION_ID = 41

    private val root: File get() = Environment.getExternalStorageDirectory().canonicalFile

    /** The app can read and write the whole storage. */
    fun hasAccess(context: Context): Boolean =
        if (Build.VERSION.SDK_INT >= 30) {
            Environment.isExternalStorageManager()
        } else {
            ContextCompat.checkSelfPermission(context, Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
        }

    fun allowed(context: Context) = DotlinkApp.link.prefs.browseFromPc && hasAccess(context)

    /** Answers one `fs_*` message from the PC. */
    fun handle(
        context: Context,
        msg: JSONObject,
    ) {
        val link = DotlinkApp.link
        val req = msg.optString("req")
        val t = msg.optString("t")
        fun reply(
            type: String,
            build: JSONObject.() -> Unit = {},
        ) = link.sendControl(JSONObject().put("t", type).put("req", req).apply(build))
        val answerType = if (t == "fs_list") "fs_list" else "fs_ok"
        if (!allowed(context)) {
            askToAllow(context)
            reply(answerType) { put("error", "Browse from PC is off on ${link.deviceName()}: turn it on in Dotlink's Settings") }
            return
        }
        link.scope.launch {
            try {
                val path = resolve(msg.optString("path", "/"))
                when (t) {
                    "fs_list" -> reply("fs_list") { put("entries", list(path)) }
                    "fs_get" -> {
                        require(path.isFile) { "${path.name} is not a file" }
                        link.uploadFile(path, req)
                    }
                    "fs_rm" -> {
                        require(path != root) { "can't delete the storage itself" }
                        require(path.exists()) { "${path.name} doesn't exist" }
                        require(path.deleteRecursively()) { "couldn't delete ${path.name}" }
                        reply("fs_ok")
                    }
                    "fs_mv" -> {
                        val to = resolve(msg.optString("to"))
                        require(path != root) { "can't move the storage itself" }
                        require(!to.exists()) { "${to.name} already exists" }
                        require(path.renameTo(to)) { "couldn't rename ${path.name}" }
                        reply("fs_ok")
                    }
                    "fs_mkdir" -> {
                        require(path.mkdirs() || path.isDirectory) { "couldn't create ${path.name}" }
                        reply("fs_ok")
                    }
                }
            } catch (e: Exception) {
                Log.w(TAG, "$t failed", e)
                reply(answerType) { put("error", e.message ?: e.javaClass.simpleName) }
            }
        }
    }

    /**
     * The file for a path from the PC (`/` is the storage root), canonical and kept under the
     * root; `Android/data` and `Android/obb` are refused.
     */
    fun resolve(path: String): File {
        val file = File(root, path.trimStart('/')).canonicalFile
        require(file == root || file.path.startsWith(root.path + File.separator)) { "outside the storage" }
        val rel = file.path.removePrefix(root.path).trimStart(File.separatorChar).lowercase()
        require(HIDDEN.none { rel == it || rel.startsWith(it + File.separator) }) { "that folder is private to its apps" }
        return file
    }

    private fun list(dir: File): JSONArray {
        require(dir.isDirectory) { "${dir.name} is not a folder" }
        val files = dir.listFiles() ?: error("can't read ${dir.name}")
        val out = JSONArray()
        for (f in files) {
            val rel = f.path.removePrefix(root.path).trimStart(File.separatorChar).lowercase()
            if (HIDDEN.any { rel == it }) continue
            out.put(
                JSONObject()
                    .put("name", f.name)
                    .put("dir", f.isDirectory)
                    .put("size", if (f.isFile) f.length() else 0)
                    .put("mtime", f.lastModified() / 1000),
            )
        }
        return out
    }

    /** The first request while it's off: a notification leading to the setting. */
    @SuppressLint("MissingPermission") // Checked by NotificationManager.areNotificationsEnabled.
    private fun askToAllow(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        if (!nm.areNotificationsEnabled() || nm.activeNotifications.any { it.id == NOTIFICATION_ID }) return
        val open =
            PendingIntent.getActivity(
                context,
                NOTIFICATION_ID,
                Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val n =
            NotificationCompat
                .Builder(context, DotlinkApp.CHANNEL_REQUESTS)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("${DotlinkApp.link.prefs.serverName} wants to browse your files")
                .setContentText("Turn on Browse from PC in Dotlink's Settings")
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
        nm.notify(NOTIFICATION_ID, n)
    }

    private val HIDDEN = listOf("android/data", "android/obb").map { it.replace('/', File.separatorChar) }
}
