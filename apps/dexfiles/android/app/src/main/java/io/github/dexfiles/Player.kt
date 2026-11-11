package io.github.dexfiles

import android.app.Activity
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import org.json.JSONObject

/**
 * The player of the preview panel: the system's MediaPlayer (hardware decoding), and for a
 * video a SurfaceView laid over the window where the library draws the player's picture area
 * (the library sends that rectangle as it moves; it hides the view under menus and dialogs).
 *
 * Called from any thread through [MainActivity.files]; the MediaPlayer is driven on the UI
 * thread, and its state is read under a lock.
 */
class Player(private val activity: Activity) {
    private val lock = Any()
    private var mp: MediaPlayer? = null
    private var prepared = false
    private var ended = false
    private var error: String? = null
    private var wantVideo = false
    private var surface: SurfaceView? = null
    private var surfaceReady = false

    fun handle(op: String, args: JSONObject): String = when (op) {
        "open" -> { activity.runOnUiThread { open(args.getString("path"), args.optBoolean("video")) }; "{}" }
        "play" -> { activity.runOnUiThread { synchronized(lock) { if (prepared) { ended = false; mp?.start() } } }; "{}" }
        "pause" -> { activity.runOnUiThread { synchronized(lock) { if (prepared) mp?.pause() } }; "{}" }
        "seek" -> { activity.runOnUiThread { synchronized(lock) { if (prepared) { ended = false; mp?.seekTo(args.getLong("ms"), MediaPlayer.SEEK_PREVIOUS_SYNC) } } }; "{}" }
        "stop" -> { activity.runOnUiThread { release() }; "{}" }
        "rect" -> { activity.runOnUiThread { place(args) }; "{}" }
        "state" -> state()
        else -> "{}"
    }

    private fun open(path: String, video: Boolean) {
        release()
        wantVideo = video
        if (video) ensureSurface()
        val p = MediaPlayer()
        synchronized(lock) {
            mp = p
            prepared = false
            ended = false
            error = null
        }
        try {
            p.setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(if (video) AudioAttributes.CONTENT_TYPE_MOVIE else AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            p.setDataSource(path)
            p.setOnPreparedListener {
                synchronized(lock) { prepared = true }
                it.start()
            }
            p.setOnCompletionListener { synchronized(lock) { ended = true } }
            p.setOnErrorListener { _, what, extra ->
                synchronized(lock) { error = "MediaPlayer error $what/$extra" }
                true
            }
            if (video && surfaceReady) surface?.holder?.let { p.setDisplay(it) }
            p.prepareAsync()
        } catch (e: Exception) {
            synchronized(lock) { error = e.message ?: e.javaClass.simpleName }
        }
    }

    /** The video's view, over the GameActivity surface. */
    private fun ensureSurface() {
        if (surface != null) return
        val view = SurfaceView(activity)
        view.setZOrderMediaOverlay(true)
        view.visibility = View.GONE
        view.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                surfaceReady = true
                synchronized(lock) { if (wantVideo) mp?.setDisplay(holder) }
            }
            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}
            override fun surfaceDestroyed(holder: SurfaceHolder) {
                surfaceReady = false
                synchronized(lock) { runCatching { mp?.setDisplay(null) } }
            }
        })
        activity.addContentView(view, FrameLayout.LayoutParams(1, 1))
        surface = view
    }

    private fun place(args: JSONObject) {
        val view = surface ?: return
        if (!args.optBoolean("visible") || !wantVideo) {
            view.visibility = View.GONE
            return
        }
        val params = (view.layoutParams as? FrameLayout.LayoutParams) ?: FrameLayout.LayoutParams(1, 1)
        params.width = args.getInt("w").coerceAtLeast(1)
        params.height = args.getInt("h").coerceAtLeast(1)
        params.leftMargin = args.getInt("x")
        params.topMargin = args.getInt("y")
        view.layoutParams = params
        view.visibility = View.VISIBLE
    }

    private fun state(): String = synchronized(lock) {
        val out = JSONObject()
        val p = mp
        out.put("ready", prepared)
        out.put("ended", ended)
        error?.let { out.put("error", it) }
        if (p != null && prepared) {
            runCatching {
                out.put("position", p.currentPosition.toLong())
                out.put("duration", p.duration.toLong())
                out.put("playing", p.isPlaying)
                out.put("width", p.videoWidth)
                out.put("height", p.videoHeight)
            }
        }
        out.toString()
    }

    fun release() {
        synchronized(lock) {
            runCatching { mp?.release() }
            mp = null
            prepared = false
            ended = false
        }
        surface?.visibility = View.GONE
    }
}
