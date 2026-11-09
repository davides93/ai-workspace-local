package dev.aiworkspace.nativellm

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.webkit.ProxyConfig
import androidx.webkit.ProxyController
import androidx.webkit.WebViewFeature
import org.json.JSONTokener
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executor

/**
 * In-app browser for the agent tools (`browser_*`).
 *
 * A plain `android.webkit.WebView` (never the Tauri one: pages must not reach the app's IPC) in a
 * panel over the lower part of the screen that the user can see, close and use (logins). All its
 * traffic goes through the policy proxy that the Rust side runs on 127.0.0.1 (`ProxyController`),
 * which decides every HTTPS connection: the agent's browser cannot reach the LAN or loopback.
 * Without `PROXY_OVERRIDE` support the tools stay unavailable. Everything runs on the main thread.
 *
 * Limit: the cookie jar is the app-wide one (the Profile API is not used yet).
 */
class AgentBrowser(private val activity: Activity) {
    private val main = Handler(Looper.getMainLooper())
    private var proxyPort = 0
    private var panel: FrameLayout? = null
    private var webView: WebView? = null
    private var pending: ((Result<String>) -> Unit)? = null
    private var timeout: Runnable? = null
    private var address: TextView? = null
    private var backButton: Button? = null
    private var forwardButton: Button? = null

    /** Routes the WebView through the proxy; `done` gets an error when the system cannot. */
    fun configure(port: Int, done: (Result<Unit>) -> Unit) {
        main.post {
            if (!WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)) {
                done(Result.failure(IllegalStateException("Il WebView di questo dispositivo non supporta il proxy: browser non disponibile")))
                return@post
            }
            proxyPort = port
            val config = ProxyConfig.Builder().removeImplicitRules().addProxyRule("127.0.0.1:$port").build()
            ProxyController.getInstance().setProxyOverride(config, Executor { it.run() }) { done(Result.success(Unit)) }
        }
    }

    private fun ensurePanel(): WebView {
        webView?.let {
            if (panel?.parent != null) {
                panel?.visibility = View.VISIBLE // closed by the user with "Chiudi": same page, shown again
                return it
            }
        }
        val wv = WebView(activity)
        wv.settings.javaScriptEnabled = true
        wv.settings.domStorageEnabled = true
        wv.settings.allowFileAccess = false
        wv.settings.allowContentAccess = false
        wv.settings.setSupportMultipleWindows(false)
        wv.settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
        wv.webViewClient = object : WebViewClient() {
            // Only https loads; no app or intent schemes.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                request.url.scheme?.lowercase() != "https"

            override fun onPageFinished(view: WebView, url: String?) {
                refreshBar(view)
                finish(Result.success(url ?: view.url ?: ""))
            }

            override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) = refreshBar(view)

            override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                if (request.isForMainFrame) finish(Result.failure(IllegalStateException("${error.description}")))
            }
        }
        fun navButton(label: String, description: String, action: () -> Unit) = Button(activity).apply {
            text = label
            contentDescription = description
            minWidth = 0
            minimumWidth = 0
            setOnClickListener { action() }
        }
        val back = navButton("‹", "Indietro") { if (wv.canGoBack()) wv.goBack() }
        val forward = navButton("›", "Avanti") { if (wv.canGoForward()) wv.goForward() }
        val reload = navButton("⟳", "Ricarica") { wv.reload() }
        val title = TextView(activity).apply {
            text = "Browser dell'agente"
            setTextColor(Color.GRAY)
            textSize = 13f
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.MIDDLE
        }
        // "Chiudi" hides the panel and keeps the page: the chat's browser button brings it back.
        val close = Button(activity).apply {
            text = "Chiudi"
            setOnClickListener { panel?.visibility = View.GONE }
        }
        val bar = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(Color.WHITE)
            setPadding(24, 8, 24, 8)
            addView(back)
            addView(forward)
            addView(reload)
            addView(title, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = 16; marginEnd = 16 })
            addView(close)
        }
        address = title
        backButton = back
        forwardButton = forward
        refreshBar(wv)
        val column = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            addView(bar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(wv, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        val frame = FrameLayout(activity).apply {
            setBackgroundColor(Color.WHITE)
            elevation = 24f
            addView(column, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        val height = (activity.resources.displayMetrics.heightPixels * 0.6).toInt()
        activity.addContentView(frame, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, height, Gravity.BOTTOM))
        panel = frame
        webView = wv
        return wv
    }

    /** Back/forward and the address follow the page. */
    private fun refreshBar(view: WebView) {
        backButton?.isEnabled = view.canGoBack()
        forwardButton?.isEnabled = view.canGoForward()
        address?.text = view.url?.let { runCatching { android.net.Uri.parse(it).host }.getOrNull() ?: it } ?: "Browser dell'agente"
    }

    /** The chat's browser button: shows the panel on the page it has (empty the first time). */
    fun show(done: (Result<Unit>) -> Unit) = main.post {
        if (proxyPort == 0) {
            done(Result.failure(IllegalStateException("Browser non configurato")))
            return@post
        }
        ensurePanel()
        done(Result.success(Unit))
    }

    fun navigate(url: String, done: (Result<String>) -> Unit) = main.post {
        finish(Result.failure(IllegalStateException("Navigazione sostituita da una nuova")))
        if (proxyPort == 0) {
            done(Result.failure(IllegalStateException("Browser non configurato")))
            return@post
        }
        val wv = ensurePanel()
        pending = done
        val t = Runnable { finish(Result.success(wv.url ?: url)) } // slow page: answer with what is there
        timeout = t
        main.postDelayed(t, 30_000)
        wv.loadUrl(url)
    }

    fun evaluate(js: String, done: (Result<String>) -> Unit) = main.post {
        val wv = webView ?: return@post done(Result.failure(IllegalStateException("Nessuna pagina aperta")))
        wv.evaluateJavascript(js) { raw ->
            // The result comes back JSON-encoded: a string literal for our scripts.
            val value = try {
                (JSONTokener(raw ?: "null").nextValue() as? String) ?: ""
            } catch (e: Exception) {
                ""
            }
            done(Result.success(value))
        }
    }

    fun screenshot(done: (Result<String>) -> Unit) = main.post {
        val wv = webView ?: return@post done(Result.failure(IllegalStateException("Nessuna pagina aperta")))
        try {
            val w = wv.width.coerceAtLeast(1)
            val h = wv.height.coerceAtLeast(1)
            val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
            wv.draw(Canvas(bitmap))
            val scaled = if (w > 1280) Bitmap.createScaledBitmap(bitmap, 1280, (h * 1280L / w).toInt().coerceAtLeast(1), true) else bitmap
            val out = ByteArrayOutputStream()
            scaled.compress(Bitmap.CompressFormat.JPEG, 70, out)
            done(Result.success("data:image/jpeg;base64," + Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)))
        } catch (e: Exception) {
            done(Result.failure(e))
        }
    }

    fun hide(done: (() -> Unit)? = null) {
        main.post {
            finish(Result.failure(IllegalStateException("Browser chiuso")))
            (panel?.parent as? ViewGroup)?.removeView(panel)
            webView?.destroy()
            panel = null
            webView = null
            address = null
            backButton = null
            forwardButton = null
            done?.invoke()
        }
    }

    private fun finish(result: Result<String>) {
        timeout?.let { main.removeCallbacks(it) }
        timeout = null
        val done = pending
        pending = null
        done?.invoke(result)
    }
}
