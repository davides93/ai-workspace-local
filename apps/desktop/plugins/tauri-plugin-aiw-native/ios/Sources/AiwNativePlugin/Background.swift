import Foundation
import UserNotifications
#if os(iOS)
import BackgroundTasks
import UIKit
#endif
#if canImport(AppIntents)
import AppIntents
#endif

// Background checks of the proactive assistant on iOS. No server and no push: the system decides when it lets the
// app run (BGAppRefreshTask for short checks, BGProcessingTask for longer ones, usually at night on the charger;
// both are only a request and iOS may run them rarely or never), plus
//   - a local notification scheduled every day at `proactive.briefing_time`, with a generic invitation to open the app;
//     actual briefing text is delivered only by a fresh engine check;
//   - two App Intents, «Controlla adesso» and «Riepilogo di oggi», that the person can put in a timed Shortcuts
//     automation (the only way to get a check at a precise time on iOS).
// Each check is one call into the Rust library (`aiw_background_tick`, hard budget of 25 s): the decisions are in the
// engine, nothing here talks to the network.
//
// NOT COMPILED OR RUN in the repository's CI: the iOS build only happens in the store workflow.

@_silgen_name("aiw_background_tick")
private func aiw_background_tick(_ request: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?

@_silgen_name("aiw_background_free")
private func aiw_background_free(_ ptr: UnsafeMutablePointer<CChar>?)

/// What the Rust shell last handed over (`schedule_payload` in `background.rs`), kept for the background tasks.
struct BackgroundConfig: Codable {
    var enabled: Bool?
    var intervalMinutes: Int?
    var briefingTime: String?
    var onlyOnPower: Bool?
    var notifications: Bool?
    var dataDir: String?
    var workspaceRoot: String?

    var isOn: Bool { enabled ?? false }
    var every: Int { max(15, intervalMinutes ?? 30) }
    var wantsNotifications: Bool { notifications ?? true }
    /// (hour, minute) of `briefingTime`, 08:00 when it cannot be read.
    var briefing: (Int, Int) {
        let p = (briefingTime ?? "08:00").split(separator: ":").map { Int($0.trimmingCharacters(in: .whitespaces)) }
        let h = (p.first ?? 8) ?? 8
        let m = (p.count > 1 ? p[1] : 0) ?? 0
        return (min(max(h, 0), 23), min(max(m, 0), 59))
    }
}

enum AiwBackground {
    /// Both identifiers are listed in Info.plist under `BGTaskSchedulerPermittedIdentifiers`.
    static let refreshId = "dev.aiworkspace.local.proactive.refresh"
    static let processingId = "dev.aiworkspace.local.proactive.processing"
    static let dailyNoticeId = "dev.aiworkspace.local.proactive.daily"
    private static let configKey = "aiw.background.config"

    static func save(_ cfg: BackgroundConfig) {
        if let data = try? JSONEncoder().encode(cfg) { UserDefaults.standard.set(data, forKey: configKey) }
    }

    static func load() -> BackgroundConfig {
        guard let data = UserDefaults.standard.data(forKey: configKey), let cfg = try? JSONDecoder().decode(BackgroundConfig.self, from: data) else {
            return BackgroundConfig()
        }
        return cfg
    }

    // MARK: the check itself

    /// One check, blocking (call it off the main thread). Returns the engine's summary, or `["error": …]`.
    static func runTick(reason: String) -> [String: Any] {
        let cfg = load()
        var req: [String: Any] = ["reason": reason, "budgetS": 25]
        if let d = cfg.dataDir { req["dataDir"] = d }
        if let w = cfg.workspaceRoot { req["workspaceRoot"] = w }
        #if os(iOS)
        UIDevice.current.isBatteryMonitoringEnabled = true
        let state = UIDevice.current.batteryState
        if state != .unknown { req["onPower"] = (state == .charging || state == .full) }
        #endif
        guard let body = try? JSONSerialization.data(withJSONObject: req), let text = String(data: body, encoding: .utf8) else {
            return ["error": "richiesta non valida"]
        }
        guard let raw = aiw_background_tick(text) else { return ["error": "nessuna risposta"] }
        defer { aiw_background_free(raw) }
        let answer = String(cString: raw)
        let parsed = (try? JSONSerialization.jsonObject(with: Data(answer.utf8))) as? [String: Any]
        return parsed ?? ["error": "risposta non valida"]
    }

    /// Shows a fresh engine notification and keeps the generic daily reminder scheduled.
    static func deliver(_ summary: [String: Any]) {
        let cfg = load()
        if cfg.isOn { scheduleDailyNotice(cfg) }
        guard cfg.wantsNotifications, let n = summary["notification"] as? [String: Any],
              let title = n["title"] as? String, let body = n["body"] as? String else { return }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default
        content.userInfo = ["route": "#/inbox"]
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "dev.aiworkspace.local.proactive.now", content: content, trigger: nil))
    }

    /// Daily reminder at `proactive.briefing_time`. A repeating notification cannot guarantee
    /// fresh briefing data, so its text must never include a cached summary.
    static func scheduleDailyNotice(_ cfg: BackgroundConfig) {
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: [dailyNoticeId])
        guard cfg.isOn, cfg.wantsNotifications else { return }
        let content = UNMutableNotificationContent()
        content.title = "Il riepilogo di oggi"
        content.body = "Apri AI Workspace per vedere il riepilogo aggiornato."
        content.sound = .default
        content.userInfo = ["route": "#/inbox"]
        var when = DateComponents()
        (when.hour, when.minute) = cfg.briefing
        center.add(UNNotificationRequest(identifier: dailyNoticeId, content: content, trigger: UNCalendarNotificationTrigger(dateMatching: when, repeats: true)))
    }

    /// The sentence a Shortcut says or shows.
    static func spoken(_ summary: [String: Any], briefing: Bool) -> String {
        if let why = summary["error"] as? String { return "Non sono riuscito a controllare: \(why)" }
        if briefing, let b = summary["briefing"] as? String, !b.isEmpty { return b }
        if let why = summary["skipped_reason"] as? String { return why }
        let n = (summary["new_proposals"] as? Int) ?? 0
        if n == 1 { return "Ho una proposta per te." }
        if n > 1 { return "Ho \(n) proposte per te." }
        return briefing ? "Niente da segnalare per oggi." : "Niente di nuovo."
    }

    // MARK: system tasks

    #if os(iOS)
    /// Must run before the end of the app launch (BGTaskScheduler refuses later registrations).
    static func register() {
        let scheduler = BGTaskScheduler.shared
        scheduler.register(forTaskWithIdentifier: refreshId, using: nil) { task in handle(task, reason: "interval") }
        scheduler.register(forTaskWithIdentifier: processingId, using: nil) { task in handle(task, reason: "briefing") }
    }

    private static func handle(_ task: BGTask, reason: String) {
        schedule() // the next one first: a task that is cut short must not end the chain
        var finished = false
        let finish: (Bool) -> Void = { ok in
            DispatchQueue.main.async {
                if finished { return }
                finished = true
                task.setTaskCompleted(success: ok)
            }
        }
        // The engine stops itself after its budget; if the system ends the task sooner the call still returns later.
        task.expirationHandler = { finish(false) }
        DispatchQueue.global(qos: .utility).async {
            deliver(runTick(reason: reason))
            finish(true)
        }
    }

    /// Asks for the next refresh (short, every `proactive.interval_minutes` at the earliest) and the next processing
    /// task (shortly before the morning briefing, on the charger when `proactive.only_on_power`). iOS may run them
    /// much later, or not at all.
    static func schedule() {
        let cfg = load()
        let scheduler = BGTaskScheduler.shared
        guard cfg.isOn else {
            scheduler.cancel(taskRequestWithIdentifier: refreshId)
            scheduler.cancel(taskRequestWithIdentifier: processingId)
            return
        }
        let refresh = BGAppRefreshTaskRequest(identifier: refreshId)
        refresh.earliestBeginDate = Date(timeIntervalSinceNow: TimeInterval(cfg.every * 60))
        try? scheduler.submit(refresh)

        let processing = BGProcessingTaskRequest(identifier: processingId)
        processing.requiresExternalPower = cfg.onlyOnPower ?? true
        processing.requiresNetworkConnectivity = false
        var when = DateComponents()
        (when.hour, when.minute) = cfg.briefing
        if let next = Calendar.current.nextDate(after: Date(), matching: when, matchingPolicy: .nextTime) {
            processing.earliestBeginDate = next.addingTimeInterval(-10 * 60)
        }
        try? scheduler.submit(processing)
    }

    /// «Controlla adesso» from the app: a few seconds of background time, so that the check ends if the app is left.
    static func checkNow(done: @escaping ([String: Any]) -> Void) {
        let app = UIApplication.shared
        var id = UIBackgroundTaskIdentifier.invalid
        id = app.beginBackgroundTask(withName: "aiw-check-now") {
            app.endBackgroundTask(id)
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let summary = runTick(reason: "shortcut")
            deliver(summary)
            DispatchQueue.main.async {
                done(summary)
                app.endBackgroundTask(id)
            }
        }
    }
    #endif
}

#if canImport(AppIntents)
// The two actions of the Shortcuts app. `openAppWhenRun` is false: they run in the background and answer in words.
// If Shortcuts does not list them after the first store build, they have to move into the app target (a Swift package
// that is linked statically does not always get its App Intents metadata extracted).

@available(iOS 16.0, *)
struct CheckNowIntent: AppIntent {
    static var title: LocalizedStringResource = "Controlla adesso"
    static var description = IntentDescription("Chiede all'assistente di guardare cosa c'è di nuovo e di preparare le proposte.")
    static var openAppWhenRun: Bool = false

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let summary = await Task.detached { AiwBackground.runTick(reason: "shortcut") }.value
        AiwBackground.deliver(summary)
        return .result(dialog: IntentDialog(stringLiteral: AiwBackground.spoken(summary, briefing: false)))
    }
}

@available(iOS 16.0, *)
struct TodaySummaryIntent: AppIntent {
    static var title: LocalizedStringResource = "Riepilogo di oggi"
    static var description = IntentDescription("Dice il riepilogo del giorno: promemoria, proposte, agenda e posta.")
    static var openAppWhenRun: Bool = false

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog {
        let summary = await Task.detached { AiwBackground.runTick(reason: "briefing") }.value
        AiwBackground.deliver(summary)
        let text = AiwBackground.spoken(summary, briefing: true)
        return .result(value: text, dialog: IntentDialog(stringLiteral: text))
    }
}

@available(iOS 16.4, *)
struct AiwShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(intent: CheckNowIntent(), phrases: ["Controlla adesso con \(.applicationName)"], shortTitle: "Controlla adesso", systemImageName: "arrow.clockwise")
        AppShortcut(intent: TodaySummaryIntent(), phrases: ["Riepilogo di oggi con \(.applicationName)"], shortTitle: "Riepilogo di oggi", systemImageName: "sun.max")
    }
}
#endif
