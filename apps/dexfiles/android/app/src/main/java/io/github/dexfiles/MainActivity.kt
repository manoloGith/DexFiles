package io.github.dexfiles

import android.Manifest
import android.content.ClipData
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.storage.StorageManager
import android.os.storage.StorageVolume
import android.provider.Settings
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import io.github.craftandroid.CraftActivity
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * DexFiles' activity. Mouse, keyboard, lifecycle, clipboard and diagnostics are craft-android's
 * [CraftActivity]; the library it loads (`libdexfiles.so`) draws the whole window.
 *
 * What a file manager needs besides is here, called by the library through [files]
 * (`src/android/mod.rs`): storage volumes, the "All files access" permission, opening a file
 * with another app, sharing and mailing files, and telling the media index about new files.
 */
class MainActivity : CraftActivity() {
    private val player by lazy { Player(this) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (!isFinishing) nativeRegisterFiles(this)
    }

    /** Files shared to the app are copied into the open folder (craft-android delivers them). */
    override fun droppableMimeTypes(): List<String> = listOf("*/*")

    /** Called by the library (any thread) with an operation and JSON; answers JSON. Never throws. */
    @Suppress("unused") // called over JNI
    fun files(op: String, json: String): String = try {
        val args = if (json.isEmpty()) JSONObject() else JSONObject(json)
        when (op) {
            "storage" -> JSONObject().put("granted", hasStorageAccess()).toString()
            "request_storage" -> { runOnUiThread { requestStorageAccess() }; "{}" }
            "volumes" -> JSONObject().put("volumes", volumes()).toString()
            "open" -> open(File(args.getString("path")), args.optBoolean("chooser"), args.optString("app"))
            "apps_for" -> JSONObject().put("apps", appsFor(args.getString("ext"), args.getString("mime"))).toString()
            "share" -> share(args.getJSONArray("paths"), args.optBoolean("email"))
            "media_scan" -> { mediaScan(args.getJSONArray("paths")); "{}" }
            "video_frame" -> videoFrame(File(args.getString("path")), args.optInt("max", 512))
            else -> if (op.startsWith("player_")) player.handle(op.removePrefix("player_"), args) else { Log.w(TAG, "files: unknown operation $op"); "{}" }
        }
    } catch (e: Exception) {
        Log.w(TAG, "files: $op failed", e)
        JSONObject().put("error", e.message ?: e.javaClass.simpleName).toString()
    }

    private fun hasStorageAccess(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
        }

    private fun requestStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val forApp = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
            try {
                startActivity(forApp)
            } catch (e: Exception) {
                startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
            }
        } else {
            requestPermissions(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE), REQUEST_STORAGE)
        }
    }

    /** Mounted volumes: internal storage, SD cards, USB drives. */
    private fun volumes(): JSONArray {
        val out = JSONArray()
        val manager = getSystemService(StorageManager::class.java) ?: return out
        for (v in manager.storageVolumes) {
            if (v.state != Environment.MEDIA_MOUNTED && v.state != Environment.MEDIA_MOUNTED_READ_ONLY) continue
            val path = volumePath(v) ?: continue
            out.put(
                JSONObject()
                    .put("path", path)
                    .put("label", v.getDescription(this) ?: "")
                    .put("removable", v.isRemovable)
                    .put("primary", v.isPrimary),
            )
        }
        return out
    }

    private fun volumePath(v: StorageVolume): String? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            v.directory?.absolutePath
        } else {
            // Android 10 has no public getter; StorageVolume.getPath has been there since 4.4.
            runCatching { v.javaClass.getMethod("getPath").invoke(v) as? String }.getOrNull()
        }

    private fun uriFor(file: File): Uri = FileProvider.getUriForFile(this, "$packageName.dexfiles", file)

    private fun mimeFor(file: File): String {
        val ext = file.extension.lowercase()
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext)
            ?: when (ext) {
                "md", "log", "ini", "conf", "rs", "kt", "py", "sh", "toml", "yaml", "yml" -> "text/plain"
                "7z" -> "application/x-7z-compressed"
                else -> "*/*"
            }
    }

    private fun open(file: File, chooser: Boolean, app: String): String {
        val uri = uriFor(file)
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, mimeFor(file))
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        // DexFiles' own default app for this kind of file (Settings ▸ Default apps).
        if (app.contains('/')) {
            val (pkg, cls) = app.split('/', limit = 2)
            intent.setClassName(pkg, cls)
            grantUriPermission(pkg, uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            return try {
                startActivity(intent)
                "{}"
            } catch (e: Exception) {
                JSONObject().put("error", "${e.javaClass.simpleName}: ${e.message}").toString()
            }
        }
        val launch = if (chooser) Intent.createChooser(intent, getString(R.string.open_with)) else intent
        runOnUiThread {
            try {
                startActivity(launch)
            } catch (e: Exception) {
                Log.w(TAG, "no app for ${file.extension}: $e")
                CraftActivity.toast(getString(R.string.no_app))
            }
        }
        return "{}"
    }

    /** The apps that open this type of file, for DexFiles' own app picker. */
    private fun appsFor(ext: String, mime: String): JSONArray {
        val probe = Intent(Intent.ACTION_VIEW).setDataAndType(Uri.parse("content://$packageName.dexfiles/root/probe.$ext"), mime)
        val flags = PackageManager.MATCH_ALL
        val found = if (Build.VERSION.SDK_INT >= 33) {
            packageManager.queryIntentActivities(probe, PackageManager.ResolveInfoFlags.of(flags.toLong()))
        } else {
            @Suppress("DEPRECATION") packageManager.queryIntentActivities(probe, flags)
        }
        val out = JSONArray()
        for (info in found.distinctBy { it.activityInfo.packageName + "/" + it.activityInfo.name }) {
            val ai = info.activityInfo ?: continue
            if (ai.packageName == packageName) continue
            val item = JSONObject()
                .put("id", "${ai.packageName}/${ai.name}")
                .put("label", info.loadLabel(packageManager).toString())
            iconPng(info.loadIcon(packageManager))?.let { item.put("icon", it) }
            out.put(item)
        }
        return out
    }

    /** An app's icon as a small PNG, base64. */
    private fun iconPng(d: android.graphics.drawable.Drawable): String? = runCatching {
        val size = 96
        val bmp = android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ARGB_8888)
        val canvas = android.graphics.Canvas(bmp)
        d.setBounds(0, 0, size, size)
        d.draw(canvas)
        val bytes = java.io.ByteArrayOutputStream()
        bmp.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, bytes)
        android.util.Base64.encodeToString(bytes.toByteArray(), android.util.Base64.NO_WRAP)
    }.getOrNull()

    private fun share(paths: JSONArray, email: Boolean): String {
        val files = (0 until paths.length()).map { File(paths.getString(it)) }.filter { it.isFile }
        if (files.isEmpty()) return JSONObject().put("error", "nothing to share").toString()
        val uris = ArrayList(files.map { uriFor(it) })
        val mimes = files.map { mimeFor(it) }.distinct()
        val type = mimes.singleOrNull()?.takeIf { it != "*/*" } ?: mimes.map { it.substringBefore('/') }.distinct().singleOrNull()?.let { "$it/*" } ?: "*/*"
        val intent = if (uris.size == 1) {
            Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
        } else {
            Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
        }
        intent.type = type
        // The receiving app gets read access through the clip.
        val clip = ClipData.newUri(contentResolver, files[0].name, uris[0])
        for (u in uris.drop(1)) clip.addItem(ClipData.Item(u))
        intent.clipData = clip
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        if (email) {
            // Only mail apps, with the files attached.
            intent.selector = Intent(Intent.ACTION_SENDTO, Uri.parse("mailto:"))
            intent.putExtra(Intent.EXTRA_SUBJECT, files.joinToString(", ") { it.name })
        }
        val title = getString(if (email) R.string.send_email else R.string.share_with)
        runOnUiThread {
            try {
                startActivity(Intent.createChooser(intent, title).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            } catch (e: Exception) {
                Log.w(TAG, "share failed: $e")
                CraftActivity.toast(getString(R.string.no_app))
            }
        }
        return "{}"
    }

    /** A video's frame (a tenth in, scaled to [max] pixels) and details, for the preview. */
    private fun videoFrame(file: File, max: Int): String {
        val r = android.media.MediaMetadataRetriever()
        try {
            r.setDataSource(file.absolutePath)
            val out = JSONObject()
            val duration = r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()
            duration?.let { out.put("duration_ms", it) }
            r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull()?.let { out.put("width", it) }
            r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull()?.let { out.put("height", it) }
            r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_TITLE)?.let { out.put("title", it) }
            r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_ARTIST)?.let { out.put("artist", it) }
            val at = (duration ?: 0L) * 100 // microseconds: a tenth of the way in
            val frame = r.getScaledFrameAtTime(at, android.media.MediaMetadataRetriever.OPTION_CLOSEST_SYNC, max, max)
            if (frame != null) {
                val bytes = java.io.ByteArrayOutputStream()
                frame.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, bytes)
                out.put("frame", android.util.Base64.encodeToString(bytes.toByteArray(), android.util.Base64.NO_WRAP))
            }
            return out.toString()
        } finally {
            runCatching { r.release() }
        }
    }

    private fun mediaScan(paths: JSONArray) {
        val list = Array(paths.length()) { paths.getString(it) }
        MediaScannerConnection.scanFile(applicationContext, list, null, null)
    }

    override fun onDestroy() {
        player.release()
        super.onDestroy()
    }

    /** This activity, for the library's file operations (a global reference on the Rust side). */
    private external fun nativeRegisterFiles(activity: MainActivity)

    companion object {
        private const val TAG = "dexfiles"
        private const val REQUEST_STORAGE = 0x5100
    }
}
