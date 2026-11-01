package dev.aiworkspace.nativellm

import android.content.Context
import com.google.mlkit.genai.common.FeatureStatus
import com.google.mlkit.genai.prompt.Generation
import com.google.mlkit.genai.prompt.GenerativeModel
import com.google.mlkit.genai.prompt.generateContentRequest
import com.google.mlkit.genai.prompt.TextPart
import com.google.ai.edge.litertlm.Backend
import com.google.ai.edge.litertlm.Engine
import com.google.ai.edge.litertlm.EngineConfig
import com.google.ai.edge.litertlm.Message
import java.io.File

/**
 * Gemini Nano through the ML Kit GenAI Prompt API. The model lives in AICore and is shared by
 * every app; inference is only allowed while the app is in the foreground (BACKGROUND_USE_BLOCKED)
 * and is rate limited per app (BUSY → retry with backoff, handled by the agent loop).
 * API names follow the ML Kit GenAI docs (2026); verify against the SDK version you pin.
 */
class GeminiNanoBackend(private val context: Context) {
    private val model: GenerativeModel by lazy { Generation.getClient() }

    /** null when available, otherwise a human readable reason. */
    suspend fun status(): String? = try {
        when (model.checkStatus()) {
            FeatureStatus.AVAILABLE -> null
            FeatureStatus.DOWNLOADABLE, FeatureStatus.DOWNLOADING -> "Gemini Nano in download tramite AICore"
            else -> "Gemini Nano non disponibile su questo dispositivo"
        }
    } catch (e: Exception) {
        "ML Kit GenAI non disponibile: ${e.message}"
    }

    suspend fun generate(system: String, prompt: String, temperature: Double?, maxTokens: Int?, send: (String) -> Unit) {
        val request = generateContentRequest(TextPart("$system\n\n$prompt")) {
            temperature?.let { this.temperature = it.toFloat() }
            maxTokens?.let { this.maxOutputTokens = it }
        }
        model.generateContentStream(request).collect { chunk ->
            chunk.candidates.firstOrNull()?.text?.let { if (it.isNotEmpty()) send(it) }
        }
    }
}

/** LiteRT-LM engine for `.litertlm` models (Gemma 3n/4…), GPU first with CPU fallback. */
class LiteRtLmBackend(private val context: Context) {
    private var engine: Engine? = null
    private var loaded: String? = null

    @Synchronized
    private fun engineFor(file: File): Engine {
        if (loaded == file.absolutePath) return engine!!
        engine?.close()
        val e = try {
            Engine(EngineConfig(modelPath = file.absolutePath, backend = Backend.GPU, cacheDir = context.cacheDir.absolutePath)).also { it.initialize() }
        } catch (gpuError: Exception) {
            Engine(EngineConfig(modelPath = file.absolutePath, backend = Backend.CPU, cacheDir = context.cacheDir.absolutePath)).also { it.initialize() }
        }
        engine = e
        loaded = file.absolutePath
        return e
    }

    suspend fun generate(file: File, system: String, prompt: String, send: (String) -> Unit) {
        val conversation = engineFor(file).createConversation()
        try {
            conversation.sendMessageAsync(Message.of("$system\n\n$prompt")).collect { msg ->
                val text = msg.toString()
                if (text.isNotEmpty()) send(text)
            }
        } finally {
            conversation.close()
        }
    }
}
