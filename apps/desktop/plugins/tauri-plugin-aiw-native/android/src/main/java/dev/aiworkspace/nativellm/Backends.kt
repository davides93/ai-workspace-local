package dev.aiworkspace.nativellm

import android.content.Context
import android.util.Log
import com.google.ai.edge.litertlm.Backend
import com.google.ai.edge.litertlm.Content
import com.google.ai.edge.litertlm.Engine
import com.google.ai.edge.litertlm.EngineConfig
import com.google.ai.edge.litertlm.Message
import com.google.mlkit.genai.common.FeatureStatus
import com.google.mlkit.genai.prompt.Generation
import com.google.mlkit.genai.prompt.GenerativeModel
import com.google.mlkit.genai.prompt.ModelPreference
import com.google.mlkit.genai.prompt.ModelReleaseStage
import com.google.mlkit.genai.prompt.SystemInstruction
import com.google.mlkit.genai.prompt.TextPart
import com.google.mlkit.genai.prompt.generateContentRequest
import com.google.mlkit.genai.prompt.generationConfig
import com.google.mlkit.genai.prompt.modelConfig
import java.io.File

/**
 * Gemini Nano through the ML Kit GenAI Prompt API (com.google.mlkit:genai-prompt 1.0.0-beta5).
 * The model lives in AICore and is shared by every app; inference is allowed only while the app
 * is in the foreground and is rate limited per app (BUSY → the agent loop retries).
 *
 * AICore serves several variants of the model, selected with a `ModelConfig` (the API cannot list
 * them: each one is asked for and its `checkStatus()` says whether this device has it):
 *  - Gemini Nano 4 (preview stage), FAST and FULL: the new model, on the TPU of Pixel phones;
 *  - Gemini Nano, stable stage: the default model of consumer devices.
 * Only the default model used to be asked for, so a phone that had Nano 4 but not yet the stable
 * model was reported as "downloading".
 */
class GeminiNanoBackend(@Suppress("unused") private val context: Context) {
    private companion object { const val TAG = "AIWNano" }

    /** `id` is the catalog model id (the profile's model); `preference` null = the default config. */
    class Variant(val id: String, val label: String, val stage: Int, val preference: Int?)

    val variants = listOf(
        Variant("gemini-nano-4-fast-system", "Gemini Nano 4 · Fast (TPU)", ModelReleaseStage.PREVIEW, ModelPreference.FAST),
        Variant("gemini-nano-4-full-system", "Gemini Nano 4 · Full", ModelReleaseStage.PREVIEW, ModelPreference.FULL),
        Variant("gemini-nano-system", "Gemini Nano", ModelReleaseStage.STABLE, null),
    )

    private val clients = java.util.concurrent.ConcurrentHashMap<String, GenerativeModel>()

    private fun client(v: Variant): GenerativeModel = clients.getOrPut(v.id) {
        val pref = v.preference
        if (pref == null) Generation.getClient()
        else Generation.getClient(generationConfig { modelConfig = modelConfig { releaseStage = v.stage; preference = pref } })
    }

    /** `FeatureStatus` of a variant on this device, or null when the question itself fails. */
    suspend fun statusOf(v: Variant): Int? = try {
        client(v).checkStatus().also { Log.i(TAG, "${v.id}: checkStatus=$it") }
    } catch (e: Exception) {
        Log.w(TAG, "${v.id}: checkStatus failed: $e")
        null
    }

    /** Variants this device can run right now, in order of preference. */
    suspend fun available(): List<Variant> = variants.filter { statusOf(it) == FeatureStatus.AVAILABLE }

    /** Context window (tokens) of a variant, when AICore says. */
    suspend fun tokenLimit(v: Variant): Int? = try {
        client(v).getTokenLimit().takeIf { it > 0 }
    } catch (e: Exception) {
        null
    }

    /** null when at least one variant is available, otherwise a human readable reason. */
    suspend fun status(): String? {
        val states = variants.map { it to statusOf(it) }
        if (states.any { it.second == FeatureStatus.AVAILABLE }) return null
        val coming = states.filter { it.second == FeatureStatus.DOWNLOADABLE || it.second == FeatureStatus.DOWNLOADING }
        if (coming.isNotEmpty()) return "Gemini Nano in download tramite AICore (${coming.joinToString { it.first.label }})"
        if (states.all { it.second == null }) return "ML Kit GenAI non disponibile su questo dispositivo"
        return "Gemini Nano non disponibile su questo dispositivo"
    }

    /**
     * Streams the answer of the variant named by `model` (any id of [variants]); with none, or when
     * that one is not available, the best available one. A variant that fails before saying anything
     * (some preview builds report AVAILABLE and then fail) hands over to the next.
     */
    suspend fun generate(model: String?, system: String, prompt: String, temperature: Double?, maxTokens: Int?, send: (String) -> Unit) {
        val ready = available()
        if (ready.isEmpty()) throw IllegalStateException(status() ?: "Gemini Nano non disponibile")
        val first = ready.firstOrNull { it.id == model }
        val order = if (first != null) listOf(first) + ready.filter { it !== first } else ready
        val failures = mutableListOf<String>()
        for (v in order) {
            var sent = false
            try {
                run(v, system, prompt, temperature, maxTokens) { sent = true; send(it) }
                return
            } catch (e: Exception) {
                Log.w(TAG, "${v.id}: generation failed (text already sent: $sent): $e")
                if (sent) throw e
                failures.add("${v.label}: ${e.message ?: e}")
            }
        }
        val hint = if (failures.isNotEmpty() && failures.all { it.contains("ErrorCode 8") }) {
            "\n\nAICore segnala il modello come disponibile ma rifiuta l'inferenza (NOT_AVAILABLE): di solito non ha ancora " +
                "scaricato la configurazione per le app di terze parti. Riavvia il telefono e riprova tra qualche minuto."
        } else ""
        throw IllegalStateException(failures.joinToString("\n").ifEmpty { "Gemini Nano non ha risposto" } + hint)
    }

    private suspend fun run(v: Variant, system: String, prompt: String, temperature: Double?, maxTokens: Int?, send: (String) -> Unit) {
        val model = client(v)
        val systemSupported = runCatching { model.isSystemPromptAvailable() }.getOrDefault(false)
        val configure: com.google.mlkit.genai.prompt.GenerateContentRequest.Builder.() -> Unit = {
            temperature?.let { this.temperature = it.toFloat() }
            maxTokens?.let { this.maxOutputTokens = it }
        }
        val request = if (systemSupported && system.isNotBlank()) {
            generateContentRequest(SystemInstruction(system), TextPart(prompt), configure)
        } else {
            generateContentRequest(TextPart(if (system.isBlank()) prompt else "$system\n\n$prompt"), configure)
        }
        // Each emission carries the newly generated text.
        model.generateContentStream(request).collect { chunk ->
            chunk.candidates.firstOrNull()?.text?.let { if (it.isNotEmpty()) send(it) }
        }
    }
}

/** LiteRT-LM engine (litertlm-android 0.19.0) for `.litertlm` models, GPU first with CPU fallback. */
class LiteRtLmBackend(private val context: Context) {
    private var engine: Engine? = null
    private var loaded: String? = null

    @Synchronized
    private fun engineFor(file: File): Engine {
        val current = engine
        if (current != null && loaded == file.absolutePath) return current
        current?.close()
        val cache = context.cacheDir.absolutePath
        val e = try {
            Engine(EngineConfig(modelPath = file.absolutePath, backend = Backend.GPU(), cacheDir = cache)).also { it.initialize() }
        } catch (gpuError: Exception) {
            Engine(EngineConfig(modelPath = file.absolutePath, backend = Backend.CPU(), cacheDir = cache)).also { it.initialize() }
        }
        engine = e
        loaded = file.absolutePath
        return e
    }

    suspend fun generate(file: File, system: String, prompt: String, send: (String) -> Unit) {
        val conversation = engineFor(file).createConversation()
        try {
            conversation.sendMessageAsync(Message.user(if (system.isBlank()) prompt else "$system\n\n$prompt")).collect { msg ->
                val text = msg.contents.contents.filterIsInstance<Content.Text>().joinToString("") { it.text }
                if (text.isNotEmpty()) send(text)
            }
        } finally {
            conversation.close()
        }
    }
}
