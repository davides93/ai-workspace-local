// aiw-apple-fm: helper that exposes Apple Foundation Models (the ~3B on-device model behind
// Apple Intelligence, macOS 26+) to the AIW desktop app over stdin/stdout.
//
//   aiw-apple-fm availability
//       prints one JSON line: {"available": bool, "detail": string?}
//
//   aiw-apple-fm generate
//       reads ONE JSON line from stdin: {"system", "prompt", "temperature"?, "maxTokens"?}
//       and streams one JSON object per line on stdout:
//         {"type":"text","delta":"..."}  ...  {"type":"done"}  |  {"type":"error","message":"..."}
//       Stdin stays open while generating; when it reaches EOF (the app cancelled the request or
//       exited) the helper stops and exits.
//
// Built by build.rs with swiftc against the macOS 26 SDK. No third-party dependencies.

import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

// A closed stdout must not kill us with SIGPIPE: the write below fails and we exit cleanly.
signal(SIGPIPE, SIG_IGN)

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: []) else { return }
    var line = data
    line.append(0x0A)
    do {
        try FileHandle.standardOutput.write(contentsOf: line)
    } catch {
        exit(0)  // the reader is gone
    }
}

func fail(_ message: String) -> Never {
    emit(["type": "error", "message": message])
    exit(1)
}

struct Request: Decodable {
    let system: String
    let prompt: String
    let temperature: Double?
    let maxTokens: Int?
}

/// Human message for a failure, from the error's description (the case names are stable even
/// when their payloads change between SDKs).
func describe(_ error: Error) -> String {
    let d = "\(error)"
    if d.contains("exceededContextWindowSize") {
        return "La conversazione supera la finestra di contesto del modello di sistema Apple (circa 4000 token). Inizia una nuova conversazione o usa un altro runtime."
    }
    if d.contains("guardrailViolation") {
        return "Apple Intelligence ha rifiutato la richiesta (filtri di sicurezza del modello di sistema)."
    }
    if d.contains("unsupportedLanguageOrLocale") {
        return "Apple Intelligence non supporta la lingua di questa richiesta."
    }
    if d.contains("assetsUnavailable") {
        return "Il modello di Apple Intelligence non è ancora scaricato: aprilo da Impostazioni di Sistema → Apple Intelligence e Siri."
    }
    if d.contains("rateLimited") || d.contains("concurrentRequests") {
        return "Il modello di sistema Apple è occupato: riprova tra poco."
    }
    return d
}

func unavailableMessage(_ reason: String) -> String {
    if reason.contains("deviceNotEligible") { return "Questo Mac non supporta Apple Intelligence." }
    if reason.contains("appleIntelligenceNotEnabled") {
        return "Apple Intelligence è disattivata: attivala in Impostazioni di Sistema → Apple Intelligence e Siri."
    }
    if reason.contains("modelNotReady") { return "Il modello di Apple Intelligence è ancora in download." }
    return reason
}

func availability() {
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        switch SystemLanguageModel.default.availability {
        case .available:
            emit(["available": true])
        case .unavailable(let reason):
            emit(["available": false, "detail": unavailableMessage("\(reason)")])
        }
        return
    }
    emit(["available": false, "detail": "Richiede macOS 26 o successivo"])
    #else
    emit(["available": false, "detail": "Helper compilato senza l'SDK FoundationModels"])
    #endif
}

/// New text since `emitted`, or nil when `text` does not extend it (snapshots can be revised;
/// a delta cannot be taken back, so we wait until the text extends what was already sent).
/// Compared by unicode scalars: counting Characters breaks when a combining mark merges into
/// the previous grapheme cluster.
func suffix(of text: String, after emitted: String) -> String? {
    let old = Array(emitted.unicodeScalars)
    let new = Array(text.unicodeScalars)
    guard new.count > old.count, Array(new[0..<old.count]) == old else { return nil }
    var view = String.UnicodeScalarView()
    view.append(contentsOf: new[old.count...])
    return String(view)
}

#if canImport(FoundationModels)
@available(macOS 26.0, *)
func generate(_ request: Request) async {
    do {
        let session = LanguageModelSession(instructions: request.system)
        var options = GenerationOptions()
        if let t = request.temperature { options.temperature = t }
        if let m = request.maxTokens { options.maximumResponseTokens = m }
        var emitted = ""
        for try await snapshot in session.streamResponse(to: request.prompt, options: options) {
            let text = snapshot.content
            if let delta = suffix(of: text, after: emitted) {
                emitted = text
                emit(["type": "text", "delta": delta])
            }
        }
        emit(["type": "done"])
    } catch {
        emit(["type": "error", "message": describe(error)])
    }
}
#endif

func runGenerate() {
    guard let line = readLine(strippingNewline: true), let data = line.data(using: .utf8),
          let request = try? JSONDecoder().decode(Request.self, from: data)
    else { fail("Richiesta non valida") }

    // The app keeps our stdin open for the whole generation: EOF means "stop".
    Thread.detachNewThread {
        while FileHandle.standardInput.availableData.count > 0 {}
        exit(0)
    }

    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        Task {
            await generate(request)
            exit(0)
        }
        dispatchMain()
    }
    #endif
    fail("Apple Foundation Models non è disponibile su questo Mac")
}

switch CommandLine.arguments.dropFirst().first {
case "availability": availability()
case "generate": runGenerate()
default:
    FileHandle.standardError.write(Data("uso: aiw-apple-fm availability | generate\n".utf8))
    exit(2)
}
