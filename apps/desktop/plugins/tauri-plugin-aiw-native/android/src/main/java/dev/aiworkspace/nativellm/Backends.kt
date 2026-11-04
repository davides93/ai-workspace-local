package dev.aiworkspace.nativellm

import android.content.Context
import com.google.ai.edge.litertlm.Backend
import com.google.ai.edge.litertlm.Content
import com.google.ai.edge.litertlm.Engine
import com.google.ai.edge.litertlm.EngineConfig
import com.google.ai.edge.litertlm.Message
import com.google.mlkit.genai.common.FeatureStatus
import com.google.mlkit.genai.prompt.Generation
import com.google.mlkit.genai.prompt.GenerativeModel
import com.google.mlkit.genai.prompt.SystemInstruction
import com.google.mlkit.genai.prompt.TextPart
import com.google.mlkit.genai.prompt.generateContentRequest
import java.io.File

/**
 * Gemini Nano through the ML Kit GenAI Prompt API (com.google.mlkit:genai-prompt 1.0.0-beta5).
 * The model lives in AICore and is shared by every app; inference is allowed only while the app
 * is in the foreground and is rate limited per app (BUSY → the agent loop retries).
 */
class GeminiNanoBackend(@Suppress("unused") private val context: Context) {
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
