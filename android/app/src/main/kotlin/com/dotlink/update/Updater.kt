/**
 * Updates from the GitHub releases: finds a newer release, downloads its APK and hands it to the
 * system installer. Android only installs it over this app when both are signed with the same
 * key, so this works for the release-signed APKs the release workflow builds.
 */
package com.dotlink.update

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONArray
import java.io.File
import java.util.concurrent.TimeUnit

data class Release(
    val version: String,
    /** The notes from the release page, without the download table. */
    val notes: String,
    val prerelease: Boolean,
    val pageUrl: String,
    val apkUrl: String,
    val apkName: String,
    val apkSize: Long,
)

sealed interface UpdateState {
    data object Idle : UpdateState

    data object Checking : UpdateState

    data object UpToDate : UpdateState

    data class Available(val release: Release) : UpdateState

    data class Downloading(val release: Release, val progress: Float) : UpdateState

    data class Failed(val message: String) : UpdateState
}

object Updater {
    private const val RELEASES = "https://api.github.com/repos/sofyan-rs/dotlink/releases?per_page=20"

    /** The release workflow appends the downloads from this heading on; the app hides them. */
    private const val DOWNLOADS_HEADING = "## Downloads"

    val state = MutableStateFlow<UpdateState>(UpdateState.Idle)

    /** The release the update dialog shows; null when it's closed. */
    val prompt = MutableStateFlow<Release?>(null)

    private var checkedOnStart = false

    private val http by lazy {
        OkHttpClient.Builder().connectTimeout(15, TimeUnit.SECONDS).readTimeout(60, TimeUnit.SECONDS).build()
    }

    /** Once per process, quietly: opens the update dialog only when there is an update. */
    suspend fun checkOnStart(context: Context) {
        if (checkedOnStart) return
        checkedOnStart = true
        check(context, prompt = true)
    }

    suspend fun check(
        context: Context,
        prompt: Boolean,
    ) {
        if (state.value is UpdateState.Checking || state.value is UpdateState.Downloading) return
        state.value = UpdateState.Checking
        state.value =
            try {
                val found = withContext(Dispatchers.IO) { latest(installedVersion(context)) }
                if (found == null) {
                    UpdateState.UpToDate
                } else {
                    if (prompt) this.prompt.value = found
                    UpdateState.Available(found)
                }
            } catch (e: Exception) {
                UpdateState.Failed(e.message ?: "Couldn't check for updates")
            }
    }

    fun installedVersion(context: Context): String = context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "0"

    /**
     * The newest release above [current]. Pre-releases count only when [current] is one too, so
     * stable installs stay on stable releases.
     */
    private fun latest(current: String): Release? {
        val request = Request.Builder().url(RELEASES).header("Accept", "application/vnd.github+json").build()
        val body =
            http.newCall(request).execute().use { response ->
                if (!response.isSuccessful) error("GitHub answered ${response.code}")
                response.body?.string() ?: error("empty answer")
            }
        val withPre = Version.parse(current)?.pre?.isNotEmpty() == true
        val releases = JSONArray(body)
        var best: Release? = null
        var bestVersion: Version? = null
        for (i in 0 until releases.length()) {
            val r = releases.getJSONObject(i)
            if (r.optBoolean("draft")) continue
            val prerelease = r.optBoolean("prerelease")
            if (prerelease && !withPre) continue
            val tag = r.optString("tag_name").removePrefix("v")
            val version = Version.parse(tag) ?: continue
            if (bestVersion != null && version <= bestVersion) continue
            val assets = r.optJSONArray("assets") ?: continue
            val apk =
                (0 until assets.length()).map { assets.getJSONObject(it) }.firstOrNull { it.optString("name").endsWith(".apk") }
                    ?: continue
            best =
                Release(
                    version = tag,
                    notes = r.optString("body").substringBefore(DOWNLOADS_HEADING).trim(),
                    prerelease = prerelease,
                    pageUrl = r.optString("html_url"),
                    apkUrl = apk.optString("browser_download_url"),
                    apkName = apk.optString("name"),
                    apkSize = apk.optLong("size"),
                )
            bestVersion = version
        }
        val installed = Version.parse(current) ?: return best
        return best?.takeIf { bestVersion!! > installed }
    }

    /** Downloads [release]'s APK (unless it's already here) and opens the system installer. */
    suspend fun update(
        context: Context,
        release: Release,
    ) {
        val app = context.applicationContext
        // Without "Install unknown apps" for Dotlink the installer refuses; send the user there
        // first, then they tap Update again.
        if (!app.packageManager.canRequestPackageInstalls()) {
            context.startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${app.packageName}"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
            return
        }
        state.value = UpdateState.Downloading(release, 0f)
        try {
            val apk = withContext(Dispatchers.IO) { download(app, release) }
            state.value = UpdateState.Available(release)
            val uri = FileProvider.getUriForFile(app, "${app.packageName}.updates", apk)
            context.startActivity(
                Intent(Intent.ACTION_VIEW)
                    .setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (e: Exception) {
            state.value = UpdateState.Failed(e.message ?: "Download failed")
        }
    }

    private fun download(
        context: Context,
        release: Release,
    ): File {
        val dir = File(context.cacheDir, "updates").apply { mkdirs() }
        val apk = File(dir, release.apkName)
        if (apk.length() == release.apkSize && release.apkSize > 0) return apk
        // Older downloads are no use any more.
        dir.listFiles()?.forEach { it.delete() }
        val part = File(dir, "${release.apkName}.part")
        http.newCall(Request.Builder().url(release.apkUrl).build()).execute().use { response ->
            if (!response.isSuccessful) error("Download failed (${response.code})")
            val body = response.body ?: error("Download failed")
            val total = body.contentLength().takeIf { it > 0 } ?: release.apkSize
            body.byteStream().use { input ->
                part.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    var done = 0L
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        output.write(buffer, 0, n)
                        done += n
                        if (total > 0) state.value = UpdateState.Downloading(release, done.toFloat() / total)
                    }
                }
            }
        }
        if (!part.renameTo(apk)) error("Couldn't save the download")
        return apk
    }
}

/** A semantic version: 1.2.3, or 1.2.3-beta.1 (a pre-release, older than 1.2.3). */
data class Version(
    val core: List<Int>,
    val pre: List<String>,
) : Comparable<Version> {
    override fun compareTo(other: Version): Int {
        for (i in 0 until 3) {
            val c = core[i].compareTo(other.core[i])
            if (c != 0) return c
        }
        if (pre.isEmpty() || other.pre.isEmpty()) return other.pre.size.coerceAtMost(1) - pre.size.coerceAtMost(1)
        for (i in 0 until minOf(pre.size, other.pre.size)) {
            val a = pre[i]
            val b = other.pre[i]
            val an = a.toIntOrNull()
            val bn = b.toIntOrNull()
            val c =
                when {
                    an != null && bn != null -> an.compareTo(bn)
                    an != null -> -1
                    bn != null -> 1
                    else -> a.compareTo(b)
                }
            if (c != 0) return c
        }
        return pre.size.compareTo(other.pre.size)
    }

    companion object {
        fun parse(text: String): Version? {
            val main = text.trim().removePrefix("v").substringBefore('+')
            val core = main.substringBefore('-').split('.').map { it.toIntOrNull() ?: return null }
            if (core.size != 3) return null
            val pre = if ('-' in main) main.substringAfter('-').split('.') else emptyList()
            return Version(core, pre)
        }
    }
}
