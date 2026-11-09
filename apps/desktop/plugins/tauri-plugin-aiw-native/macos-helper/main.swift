// aiw-apple-fm: helper that exposes Apple Foundation Models (the ~3B on-device model behind
// Apple Intelligence, macOS 26+) to the AIW desktop app over stdin/stdout.
//
//   aiw-apple-fm availability
//       prints one JSON line: {"available": bool, "detail": string?, "contextTokens": int?}
//
//   aiw-apple-fm generate
//       reads ONE JSON line from stdin:
//         {"system", "prompt", "temperature"?, "maxTokens"?,
//          "tools"?: [{"name","description","parameters"}],          native tool calling
//          "transcript"?: [{"role":"user|assistant|tool", "text", "toolCalls"?, "id"?, "name"?}],
//          "contextTokens"?}
//       and streams one JSON object per line on stdout:
//         {"type":"text","delta":"..."}  ...  {"type":"toolcall","calls":[{"id","name","arguments"}]}?
//         {"type":"done"}  |  {"type":"error","message":"..."}
//       Tools are never run here: the model's calls are handed back to the app, which runs them
//       and asks again with the results (see AppleFMCore.swift). Stdin stays open while
//       generating; when it reaches EOF (the app cancelled the request or exited) the helper stops.
//
//   aiw-apple-fm selftest
//       checks the parts that do not need the model (schemas, transcript, fitting) and prints
//       one JSON line: {"ok": bool, "checks": n, "failures": [..]}
//
// Built by build.rs with swiftc against the macOS 26 SDK, together with
// ios/Sources/AiwNativePlugin/AppleFMCore.swift (shared with the iOS plugin). No dependencies.

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
            emit(["available": true, "contextTokens": afmDefaultWindow()])
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

func runGenerate() {
    guard let line = readLine(strippingNewline: true), let data = line.data(using: .utf8),
          let request = try? JSONDecoder().decode(AFMRequest.self, from: data)
    else { fail("Richiesta non valida") }

    // The app keeps our stdin open for the whole generation: EOF means "stop".
    Thread.detachNewThread {
        while FileHandle.standardInput.availableData.count > 0 {}
        exit(0)
    }

    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        Task {
            await afmGenerate(request, emit: emit)
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
case "selftest": runSelfTest()
default:
    FileHandle.standardError.write(Data("uso: aiw-apple-fm availability | generate | selftest\n".utf8))
    exit(2)
}
