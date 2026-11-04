import Foundation
import SwiftRs
import Tauri
import UIKit
#if canImport(FoundationModels)
import FoundationModels
#endif

class GenerateArgs: Decodable {
    let runtime: String
    let model: String?
    let system: String
    let prompt: String
    let temperature: Double?
    let maxTokens: Int?
    let onEvent: Channel
}

/// On-device inference for Apple platforms through the Foundation Models framework
/// (iOS / iPadOS / macOS 26+): the ~3B system model behind Apple Intelligence.
/// Tokens are streamed back to Rust through `onEvent` as {type: "text", delta}.
class AiwNativePlugin: Plugin {
    @objc public func availability(_ invoke: Invoke) {
        var runtimes: [[String: Any]] = []
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *) {
            switch SystemLanguageModel.default.availability {
            case .available:
                runtimes.append(["id": "apple-fm", "available": true, "models": []])
            case .unavailable(let reason):
                runtimes.append(["id": "apple-fm", "available": false, "detail": "\(reason)", "models": []])
            }
        } else {
            runtimes.append(["id": "apple-fm", "available": false, "detail": "Richiede iOS 26 o successivo", "models": []])
        }
        #else
        runtimes.append(["id": "apple-fm", "available": false, "detail": "SDK senza FoundationModels", "models": []])
        #endif
        invoke.resolve(["runtimes": runtimes])
    }

    @objc public func generate(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(GenerateArgs.self)
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *) {
            Task {
                do {
                    let session = LanguageModelSession(instructions: args.system)
                    var options = GenerationOptions()
                    if let t = args.temperature { options.temperature = t }
                    if let m = args.maxTokens { options.maximumResponseTokens = m }
                    var emitted = ""
                    // Snapshots contain the whole text so far: forward only the new suffix.
                    for try await snapshot in session.streamResponse(to: args.prompt, options: options) {
                        let text = snapshot.content
                        if text.count > emitted.count {
                            let delta = String(text.dropFirst(emitted.count))
                            emitted = text
                            try args.onEvent.send(["type": "text", "delta": delta])
                        }
                    }
                    try args.onEvent.send(["type": "done"])
                    invoke.resolve()
                } catch {
                    try? args.onEvent.send(["type": "error", "message": "\(error)"])
                    invoke.reject("\(error)")
                }
            }
            return
        }
        #endif
        invoke.reject("Apple Foundation Models non disponibile su questo dispositivo")
    }
}

@_cdecl("init_plugin_aiw_native")
func initPlugin() -> Plugin {
    return AiwNativePlugin()
}
