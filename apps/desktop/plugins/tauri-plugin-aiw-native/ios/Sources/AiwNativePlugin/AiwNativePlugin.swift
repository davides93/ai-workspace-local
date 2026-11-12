import Foundation
import SwiftRs
import Tauri
import UIKit
import Speech
import UserNotifications
import WebKit
#if os(iOS)
import BackgroundTasks
#endif
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
class AiwNativePlugin: Plugin, UNUserNotificationCenterDelegate {
    private var webview: WKWebView?

    override init() {
        super.init()
        #if os(iOS)
        // BGTaskScheduler wants its handlers before the app has finished launching; the plugin is created during setup.
        AiwBackground.register()
        UNUserNotificationCenter.current().delegate = self
        #endif
    }

    @objc public override func load(webview: WKWebView) {
        self.webview = webview
    }

    /// A tap on one of our notifications opens the page it names (`#/inbox`): the app is a hash router.
    public func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        if let route = response.notification.request.content.userInfo["route"] as? String, let data = try? JSONSerialization.data(withJSONObject: [route]),
           let arr = String(data: data, encoding: .utf8) {
            DispatchQueue.main.async { self.webview?.evaluateJavaScript("window.location.hash = \(arr)[0];", completionHandler: nil) }
        }
        completionHandler()
    }

    // MARK: background checks (see Background.swift); called from Rust through `aiw_background`

    private func backgroundAnswer(_ invoke: Invoke) {
        UNUserNotificationCenter.current().getNotificationSettings { settings in
            let state: String
            switch settings.authorizationStatus {
            case .authorized, .provisional, .ephemeral: state = "granted"
            case .denied: state = "denied"
            default: state = "not_determined"
            }
            #if os(iOS)
            BGTaskScheduler.shared.getPendingTaskRequests { requests in
                let scheduled = requests.contains { $0.identifier == AiwBackground.refreshId || $0.identifier == AiwBackground.processingId }
                invoke.resolve([
                    "platform": "ios", "supported": true, "notifications": state, "scheduled": scheduled,
                    "detail": "iOS decide quando far partire i controlli: può farlo di rado. Per un orario preciso usa un'automazione dei Comandi rapidi.",
                ])
            }
            #else
            invoke.resolve(["platform": "ios", "supported": false, "notifications": state, "scheduled": false])
            #endif
        }
    }

    @objc public func backgroundState(_ invoke: Invoke) {
        backgroundAnswer(invoke)
    }

    /// The payload is the shell's schedule: keep it for the background tasks, schedule them and the daily notification.
    @objc public func backgroundSchedule(_ invoke: Invoke) throws {
        let cfg = try invoke.parseArgs(BackgroundConfig.self)
        AiwBackground.save(cfg)
        #if os(iOS)
        AiwBackground.schedule()
        #endif
        AiwBackground.scheduleDailyNotice(cfg)
        backgroundAnswer(invoke)
    }

    @objc public func backgroundCheckNow(_ invoke: Invoke) {
        #if os(iOS)
        AiwBackground.checkNow { _ in self.backgroundAnswer(invoke) }
        #else
        backgroundAnswer(invoke)
        #endif
    }

    /// Asked only when the person turns the assistant on (or presses the button); iOS shows its own dialog once.
    @objc public func backgroundRequestNotifications(_ invoke: Invoke) {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in
            self.backgroundAnswer(invoke)
        }
    }

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
