package dev.aiworkspace.nativellm

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File

@InvokeArg
class BrowserConfigureArgs {
    var proxyPort: Int = 0
}

@InvokeArg
class BrowserNavigateArgs {
    lateinit var url: String
}

@InvokeArg
class BrowserEvalArgs {
    lateinit var js: String
}

@InvokeArg
class GenerateArgs {
    lateinit var runtime: String
    var model: String? = null
    var system: String = ""
    lateinit var prompt: String
    var temperature: Double? = null
    var maxTokens: Int? = null
    lateinit var onEvent: Channel
}

/**
 * On-device inference for Android:
 *  - `gemini-nano`: ML Kit GenAI Prompt API on AICore (shared system model; foreground only).
 *  - `litert-lm`:   LiteRT-LM engine with `.litertlm` files placed in the app's models folder.
 * Tokens are streamed to Rust through the `onEvent` channel as {type:"text", delta}.
 */
@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")])
class NativeLlmPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val gemini by lazy { GeminiNanoBackend(activity) }
    private val litert by lazy { LiteRtLmBackend(activity) }
    private val browser by lazy { AgentBrowser(activity) }

    private var webView: WebView? = null

    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
        // Cold start from a notification: the page needs a moment before it can be told where to go.
        routeFrom(activity.intent, 1500)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        routeFrom(intent, 0)
    }

    /** A notification tap carries `aiw_route` (`#/inbox`): the app is a hash router, so setting the hash opens the page. */
    private fun routeFrom(intent: Intent?, delayMs: Long) {
        val route = intent?.getStringExtra("aiw_route") ?: return
        intent.removeExtra("aiw_route")
        val js = "window.location.hash = " + org.json.JSONObject.quote(route) + ";"
        Handler(Looper.getMainLooper()).postDelayed({ webView?.evaluateJavascript(js, null) }, delayMs)
    }

    private fun modelsDir(): File = File(activity.filesDir, "models").apply { mkdirs() }

    @Command
    fun availability(invoke: Invoke) {
        scope.launch {
            val runtimes = JSArray()
            val geminiReady = gemini.available()
            val geminiInfo = runtimeInfo("gemini-nano", gemini.status(), geminiReady.map { it.id })
            geminiReady.firstOrNull()?.let { v -> gemini.tokenLimit(v)?.let { geminiInfo.put("contextTokens", it) } }
            runtimes.put(geminiInfo)
            val files = modelsDir().listFiles { f -> f.name.endsWith(".litertlm") }?.map { it.name } ?: emptyList()
            runtimes.put(runtimeInfo("litert-lm", if (files.isEmpty()) "Nessun file .litertlm in ${modelsDir()}" else null, files))
            invoke.resolve(JSObject().put("runtimes", runtimes))
        }
    }

    private fun runtimeInfo(id: String, unavailableReason: String?, models: List<String>): JSObject {
        val o = JSObject()
        o.put("id", id)
        o.put("available", unavailableReason == null)
        if (unavailableReason != null) o.put("detail", unavailableReason)
        o.put("models", JSArray(models))
        return o
    }

    @Command
    fun generate(invoke: Invoke) {
        val args = invoke.parseArgs(GenerateArgs::class.java)
        scope.launch {
            val send: (String) -> Unit = { delta -> args.onEvent.send(JSObject().put("type", "text").put("delta", delta)) }
            try {
                when (args.runtime) {
                    "gemini-nano" -> gemini.generate(args.model, args.system, args.prompt, args.temperature, args.maxTokens, send)
                    "litert-lm" -> {
                        val file = args.model?.let { File(modelsDir(), it) }?.takeIf { it.exists() }
                            ?: modelsDir().listFiles { f -> f.name.endsWith(".litertlm") }?.firstOrNull()
                            ?: throw IllegalStateException("Nessun modello .litertlm disponibile")
                        litert.generate(file, args.system, args.prompt, send)
                    }
                    else -> throw IllegalArgumentException("Runtime nativo sconosciuto: ${args.runtime}")
                }
                args.onEvent.send(JSObject().put("type", "done"))
                invoke.resolve()
            } catch (e: Exception) {
                args.onEvent.send(JSObject().put("type", "error").put("message", e.message ?: e.toString()))
                invoke.reject(e.message ?: "errore di inferenza")
            }
        }
    }

    // In-app browser of the agent tools (see AgentBrowser); called from Rust, which blocks on each.
    @Command
    fun browserConfigure(invoke: Invoke) {
        val args = invoke.parseArgs(BrowserConfigureArgs::class.java)
        browser.configure(args.proxyPort) { r -> r.fold({ invoke.resolve() }, { invoke.reject(it.message ?: "browser non disponibile") }) }
    }

    @Command
    fun browserNavigate(invoke: Invoke) {
        val args = invoke.parseArgs(BrowserNavigateArgs::class.java)
        browser.navigate(args.url) { r -> r.fold({ invoke.resolve(JSObject().put("url", it)) }, { invoke.reject(it.message ?: "navigazione fallita") }) }
    }

    @Command
    fun browserEval(invoke: Invoke) {
        val args = invoke.parseArgs(BrowserEvalArgs::class.java)
        browser.evaluate(args.js) { r -> r.fold({ invoke.resolve(JSObject().put("value", it)) }, { invoke.reject(it.message ?: "script fallito") }) }
    }

    @Command
    fun browserScreenshot(invoke: Invoke) {
        browser.screenshot { r -> r.fold({ invoke.resolve(JSObject().put("dataUrl", it)) }, { invoke.reject(it.message ?: "screenshot fallito") }) }
    }

    @Command
    fun browserShow(invoke: Invoke) {
        browser.show { r -> r.fold({ invoke.resolve() }, { invoke.reject(it.message ?: "browser non disponibile") }) }
    }

    @Command
    fun browserClose(invoke: Invoke) {
        browser.hide { invoke.resolve() }
    }

    // Background checks (see Background.kt); called from Rust through `aiw_background`.
    private fun notificationsState(): String = when {
        ProactiveNotifier.permissionGranted(activity) -> "granted"
        BackgroundScheduler.asked(activity) -> "denied"
        else -> "not_determined"
    }

    private fun backgroundAnswer(invoke: Invoke) {
        scope.launch {
            invoke.resolve(
                JSObject()
                    .put("platform", "android")
                    .put("supported", true)
                    .put("notifications", notificationsState())
                    .put("scheduled", BackgroundScheduler.isScheduled(activity))
                    .put("detail", "Android decide quando far partire i controlli: di solito ogni 15-30 minuti, meno a batteria scarica."),
            )
        }
    }

    @Command
    fun backgroundState(invoke: Invoke) = backgroundAnswer(invoke)

    /** The payload is the shell's schedule (`schedule_payload`): keep it for the workers and (re)schedule them. */
    @Command
    fun backgroundSchedule(invoke: Invoke) {
        val data = invoke.getArgs()
        BackgroundScheduler.save(activity, data)
        BackgroundScheduler.apply(activity, BackgroundConfig.from(data))
        backgroundAnswer(invoke)
    }

    @Command
    fun backgroundCheckNow(invoke: Invoke) {
        BackgroundScheduler.checkNow(activity)
        backgroundAnswer(invoke)
    }

    /** Asked only when the person turns the assistant on (or presses the button): Android 13+ shows its own dialog. */
    @Command
    fun backgroundRequestNotifications(invoke: Invoke) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && getPermissionState("notifications") != PermissionState.GRANTED) {
            BackgroundScheduler.markAsked(activity)
            requestPermissionForAlias("notifications", invoke, "notificationsResult")
        } else {
            backgroundAnswer(invoke)
        }
    }

    @PermissionCallback
    fun notificationsResult(invoke: Invoke) = backgroundAnswer(invoke)
}
