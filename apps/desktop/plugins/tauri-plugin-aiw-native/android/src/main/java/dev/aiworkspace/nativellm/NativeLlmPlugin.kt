package dev.aiworkspace.nativellm

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
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
@TauriPlugin
class NativeLlmPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val gemini by lazy { GeminiNanoBackend(activity) }
    private val litert by lazy { LiteRtLmBackend(activity) }

    private fun modelsDir(): File = File(activity.filesDir, "models").apply { mkdirs() }

    @Command
    fun availability(invoke: Invoke) {
        scope.launch {
            val runtimes = JSArray()
            runtimes.put(runtimeInfo("gemini-nano", gemini.status(), emptyList()))
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
                    "gemini-nano" -> gemini.generate(args.system, args.prompt, args.temperature, args.maxTokens, send)
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
}
