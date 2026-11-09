import Foundation
import SwiftRs
import Tauri
import UIKit
import Speech
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
    let tools: [AFMToolSpec]?
    let transcript: [AFMTurn]?
    let contextTokens: Int?
    let onEvent: Channel
}

/// On-device inference for Apple platforms through the Foundation Models framework
/// (iOS / iPadOS / macOS 26+): the ~3B system model behind Apple Intelligence.
/// The answer is streamed back to Rust through `onEvent` ({type: "text" | "toolcall" | "done" |
/// "error"}); the tools of the app are offered to the model as native tools and their calls
/// handed back, not run here (see AppleFMCore.swift, shared with the macOS helper).
class AiwNativePlugin: Plugin {
    @objc public func availability(_ invoke: Invoke) {
        var runtimes: [[String: Any]] = []
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *) {
            switch SystemLanguageModel.default.availability {
            case .available:
                runtimes.append(["id": "apple-fm", "available": true, "models": [], "contextTokens": afmDefaultWindow()])
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

    /// Speech to text for a WAV file with SFSpeechRecognizer (on-device when the language supports it).
    @objc public func transcribe(_ invoke: Invoke) throws {
        struct Args: Decodable { let path: String; let language: String? }
        let args = try invoke.parseArgs(Args.self)
        SFSpeechRecognizer.requestAuthorization { status in
            guard status == .authorized else {
                invoke.reject("Riconoscimento vocale non autorizzato: abilitalo in Impostazioni › AI Workspace › Riconoscimento vocale")
                return
            }
            let locale = Locale(identifier: args.language ?? Locale.preferredLanguages.first ?? "it-IT")
            guard let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer(), recognizer.isAvailable else {
                invoke.reject("Riconoscimento vocale non disponibile per la lingua \(locale.identifier)")
                return
            }
            let request = SFSpeechURLRecognitionRequest(url: URL(fileURLWithPath: args.path))
            request.shouldReportPartialResults = false
            if recognizer.supportsOnDeviceRecognition { request.requiresOnDeviceRecognition = true }
            var finished = false
            recognizer.recognitionTask(with: request) { result, error in
                if finished { return }
                if let result = result, result.isFinal {
                    finished = true
                    invoke.resolve(["text": result.bestTranscription.formattedString])
                } else if let error = error {
                    finished = true
                    // "No speech detected" is not a failure: empty text.
                    let ns = error as NSError
                    if ns.domain == "kAFAssistantErrorDomain" && (ns.code == 1110 || ns.code == 1101) {
                        invoke.resolve(["text": ""])
                    } else {
                        invoke.reject("\(error.localizedDescription)")
                    }
                }
            }
        }
    }

    @objc public func generate(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(GenerateArgs.self)
        #if canImport(FoundationModels)
        if #available(iOS 26.0, macOS 26.0, *) {
            let request = AFMRequest(
                system: args.system, prompt: args.prompt, temperature: args.temperature, maxTokens: args.maxTokens,
                tools: args.tools, transcript: args.transcript, contextTokens: args.contextTokens)
            Task {
                // Failures reach Rust as an `error` event; the command itself always completes.
                await afmGenerate(request) { event in args.onEvent.send(event) }
                invoke.resolve()
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
