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
            "dex_apps" -> JSONObject().put("apps", dexApps(args.optString("key"))).toString()
            "installed" -> JSONObject().put("installed", installed(args.getJSONArray("ids"))).toString()
            "clip_set" -> { clipSet(args.getJSONArray("paths")); "{}" }
            "clip_import" -> clipImport()
            "drag_out" -> { dragOut(args.getJSONArray("paths")); "{}" }
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

    /**
     * The Dex apps: activities that answer [DEX_OPEN] and list the extensions they open in a
     * `<meta-data>` (docs/apps-dex.md). DexFiles makes them the default apps for those.
     */
    private fun dexApps(key: String): JSONArray {
        val probe = Intent(DEX_OPEN)
        val flags = PackageManager.GET_META_DATA
        val found = if (Build.VERSION.SDK_INT >= 33) {
            packageManager.queryIntentActivities(probe, PackageManager.ResolveInfoFlags.of(flags.toLong()))
        } else {
            @Suppress("DEPRECATION") packageManager.queryIntentActivities(probe, flags)
        }
        val out = JSONArray()
        for (info in found) {
            val ai = info.activityInfo ?: continue
            if (ai.packageName == packageName) continue
            val exts = ai.metaData?.getString(key) ?: ai.applicationInfo?.metaData?.getString(key) ?: continue
            out.put(
                JSONObject()
                    .put("id", "${ai.packageName}/${ai.name}")
                    .put("label", info.loadLabel(packageManager).toString())
                    .put("extensions", exts)
            )
        }
        return out
    }

    /** Which of these activities (`package/activity`) are installed. */
    private fun installed(ids: JSONArray): JSONArray {
        val out = JSONArray()
        for (i in 0 until ids.length()) {
            val id = ids.getString(i)
            val pkg = id.substringBefore('/')
            val cls = id.substringAfter('/', "").let { if (it.startsWith(".")) pkg + it else it }
            val found = runCatching {
                if (cls.isEmpty()) {
                    @Suppress("DEPRECATION") packageManager.getPackageInfo(pkg, 0)
                } else {
                    @Suppress("DEPRECATION") packageManager.getActivityInfo(android.content.ComponentName(pkg, cls), 0)
                }
            }.isSuccess
            if (found) out.put(id)
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

    private val clipboard by lazy { getSystemService(CLIPBOARD_SERVICE) as android.content.ClipboardManager }

    /**
     * Copied files go to the system clipboard as content URIs (each with its path as text), so
     * other apps can paste them (a mail, a note, a chat), and DexFiles knows them for its own.
     */
    private fun clipSet(paths: JSONArray) {
        val files = (0 until paths.length()).map { File(paths.getString(it)) }.filter { it.exists() }
        if (files.isEmpty()) return
        val items = files.mapNotNull { f -> runCatching { ClipData.Item(f.absolutePath, null, uriFor(f)) }.getOrNull() }
        if (items.isEmpty()) return
        val mimes = files.map { mimeFor(it) }.distinct().toTypedArray()
        val clip = ClipData(android.content.ClipDescription(CLIP_LABEL, mimes), items.first())
        items.drop(1).forEach { clip.addItem(it) }
        runOnUiThread { runCatching { clipboard.setPrimaryClip(clip) } }
    }

    /**
     * Files another app put on the clipboard, copied into the cache to paste:
     * `{"ours": true}` when the clipboard holds DexFiles' own copy, `{"paths": [...]}` otherwise.
     */
    private fun clipImport(): String {
        val out = JSONObject()
        val clip = clipboard.primaryClip ?: return out.toString()
        if (clip.description.label?.toString() == CLIP_LABEL) return out.put("ours", true).toString()
        val uris = (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }.filter { it.scheme == "content" || it.scheme == "file" }
        if (uris.isEmpty()) return out.toString()
        val dir = File(cacheDir, "clipboard-import/${System.currentTimeMillis()}").apply { mkdirs() }
        val paths = JSONArray()
        for ((i, uri) in uris.withIndex()) {
            runCatching {
                val name = displayName(uri)?.replace('/', '_')?.takeIf { it.isNotBlank() } ?: "file-${i + 1}"
                var target = File(dir, name)
                var n = 2
                while (target.exists()) target = File(dir, "${target.nameWithoutExtension} ($n).${target.extension}".removeSuffix(".")).also { n++ }
                contentResolver.openInputStream(uri)?.use { input -> target.outputStream().use { input.copyTo(it) } }
                if (target.exists()) paths.put(target.absolutePath)
            }.onFailure { Log.w(TAG, "clipboard import: $uri: $it") }
        }
        return out.put("paths", paths).toString()
    }

    private fun displayName(uri: Uri): String? = runCatching {
        contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/')

    /**
     * A drag of files that leaves the window becomes a system drag, which other apps (My Files,
     * a mail, a chat in split screen or in another DeX window) can take.
     */
    private fun dragOut(paths: JSONArray) {
        val files = (0 until paths.length()).map { File(paths.getString(it)) }.filter { it.exists() }
        if (files.isEmpty()) return
        val items = files.mapNotNull { f -> runCatching { ClipData.Item(f.absolutePath, null, uriFor(f)) }.getOrNull() }
        if (items.isEmpty()) return
        val mimes = files.map { mimeFor(it) }.distinct().toTypedArray()
        val clip = ClipData(android.content.ClipDescription(DRAG_LABEL, mimes), items.first())
        items.drop(1).forEach { clip.addItem(it) }
        val label = if (files.size == 1) files[0].name else "${files.size} × ${files[0].name}…"
        runOnUiThread {
            val view = window.decorView
            val flags = android.view.View.DRAG_FLAG_GLOBAL or android.view.View.DRAG_FLAG_GLOBAL_URI_READ
            val started = runCatching { view.startDragAndDrop(clip, LabelShadow(view, label), DRAG_LABEL, flags) }.getOrDefault(false)
            Log.i(TAG, "drag out of ${files.size} file(s): started=$started")
        }
    }

    /** The drag's picture: a small rounded label with the file name (or how many). */
    private class LabelShadow(view: android.view.View, private val label: String) : android.view.View.DragShadowBuilder(view) {
        private val density = view.resources.displayMetrics.density
        private val paint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            textSize = 14f * density
            color = android.graphics.Color.WHITE
        }
        private val bg = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply { color = 0xE0_0A_5C_C8.toInt() }
        private val text = if (label.length > 40) label.take(39) + "…" else label
        private val w = (paint.measureText(text) + 24 * density).toInt()
        private val h = (32 * density).toInt()

        override fun onProvideShadowMetrics(size: android.graphics.Point, touch: android.graphics.Point) {
            size.set(w, h)
            touch.set(w / 2, h / 2)
        }

        override fun onDrawShadow(canvas: android.graphics.Canvas) {
            canvas.drawRoundRect(0f, 0f, w.toFloat(), h.toFloat(), 8 * density, 8 * density, bg)
            canvas.drawText(text, 12 * density, h / 2f - (paint.descent() + paint.ascent()) / 2, paint)
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
        private const val CLIP_LABEL = "DexFiles files"
        /** The action a Dex app's activity answers to be found (it opens files with VIEW). */
        const val DEX_OPEN = "io.github.dexfiles.action.OPEN"
        private const val DRAG_LABEL = "DexFiles drag"
    }
}
