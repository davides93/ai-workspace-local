// Folders of the computer the person hands to the agents (Mac App Store build, App Sandbox).
//
// The sandbox lets the app touch only its container and what the person picked in the macOS open panel.
// A pick lasts until the app quits; kept as a security-scoped bookmark and reopened at launch it lasts
// for good. This file is the C ABI the Rust side calls (JSON in, JSON out, like the native browser):
//
//   pick     {message, prompt}  -> {folders: [{path, bookmark}], failed: [name]}   (main queue, waits for the person)
//   open     {bookmark}         -> {path, bookmark?}   reopens the permission; `bookmark` only when it was stale
//   release  {path}             -> {}                  gives up the permission held for the folder
//   realHome {}                 -> {path}              the person's real home (`~` is the container here)
//
// Nothing here logs a path or a bookmark.
import AppKit
import Foundation

enum FolderGrants {
    private static let lock = NSLock()
    /// Canonical path -> the URL whose access is open, and whether `startAccessing…` has to be balanced.
    private static var held: [String: (url: URL, started: Bool)] = [:]

    struct Failure: Error, CustomStringConvertible {
        let description: String
    }

    /// The home folder of the person: in a sandboxed app `NSHomeDirectory()` is the container.
    static func realHome() -> String? {
        guard let pw = getpwuid(getuid()), let dir = pw.pointee.pw_dir else { return nil }
        return String(cString: dir)
    }

    static func canonical(_ url: URL) -> String {
        url.resolvingSymlinksInPath().standardizedFileURL.path
    }

    static func makeBookmark(_ url: URL) throws -> String {
        do {
            return try url.bookmarkData(options: [.withSecurityScope], includingResourceValuesForKeys: nil, relativeTo: nil)
                .base64EncodedString()
        } catch {
            throw Failure(description: "macOS non ha creato il permesso per «\(url.lastPathComponent)» (\(error.localizedDescription))")
        }
    }

    /// Keeps the access to `url` open until `release`. A second hold of the same folder does not start it again.
    @discardableResult
    static func hold(_ url: URL, mustStart: Bool) throws -> String {
        let path = canonical(url)
        lock.lock(); defer { lock.unlock() }
        if held[path] != nil { return path }
        let started = url.startAccessingSecurityScopedResource()
        if mustStart && !started {
            throw Failure(description: "macOS non ha riaperto il permesso su questa cartella: aggiungila di nuovo")
        }
        held[path] = (url, started)
        return path
    }

    static func release(path: String) {
        // The Rust side says the path as `canonicalize` does (`/private/var/…`); the keys are `canonical(_:)` (`/var/…`).
        let key = canonical(URL(fileURLWithPath: path))
        lock.lock(); defer { lock.unlock() }
        if let entry = held.removeValue(forKey: held[path] != nil ? path : key), entry.started {
            entry.url.stopAccessingSecurityScopedResource()
        }
    }

    static func isHeld(path: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return held[path] != nil
    }

    /// Reopens a saved permission. Returns where the folder is now and, when the bookmark had gone stale
    /// (the folder was moved or renamed), a fresh one to store in its place.
    static func open(bookmark: String) throws -> (path: String, renewed: String?) {
        guard let data = Data(base64Encoded: bookmark) else {
            throw Failure(description: "il permesso salvato non è leggibile: aggiungi di nuovo la cartella")
        }
        var stale = false
        let url: URL
        do {
            url = try URL(resolvingBookmarkData: data, options: [.withSecurityScope], relativeTo: nil, bookmarkDataIsStale: &stale)
        } catch let error as NSError {
            // 4 and 260: no such file; 259: what macOS answers for a folder that was deleted (and for text that is
            // not a bookmark): either way the permission is of no use any more.
            if error.domain == NSCocoaErrorDomain && (error.code == 4 || error.code == 259 || error.code == 260) {
                throw Failure(description: "la cartella non c'è più, è stata spostata o il permesso non è più valido: aggiungila di nuovo")
            }
            throw Failure(description: "il permesso su questa cartella non si riapre (\(error.localizedDescription)): aggiungila di nuovo")
        }
        let path = try hold(url, mustStart: true)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue else {
            release(path: path)
            throw Failure(description: "la cartella non c'è più o non è una cartella")
        }
        // A stale bookmark still opens, but would not at some later launch: make a fresh one (needs the access just opened).
        let renewed = stale ? (try? makeBookmark(url)) : nil
        return (path, renewed)
    }

    /// Opens the panel on folders, on the main queue, and hands back what the person chose, each with its
    /// permission. An empty list: the person cancelled.
    static func pick(message: String, prompt: String, done: @escaping (Result<(folders: [[String: String]], failed: [String]), Failure>) -> Void) {
        DispatchQueue.main.async {
            let panel = NSOpenPanel()
            panel.canChooseDirectories = true
            panel.canChooseFiles = false
            panel.allowsMultipleSelection = true
            panel.canCreateDirectories = false
            panel.resolvesAliases = true
            panel.message = message
            panel.prompt = prompt
            if let home = realHome() { panel.directoryURL = URL(fileURLWithPath: home, isDirectory: true) }
            let finish: (NSApplication.ModalResponse) -> Void = { response in
                guard response == .OK else { done(.success(([], []))); return }
                var folders: [[String: String]] = []
                var failed: [String] = []
                for url in panel.urls {
                    do {
                        let bookmark = try makeBookmark(url)
                        // Keeps the folder usable for this launch; the saved bookmark does it for the next ones.
                        let path = try hold(url, mustStart: false)
                        folders.append(["path": path, "bookmark": bookmark])
                    } catch {
                        failed.append(url.lastPathComponent)
                    }
                }
                if folders.isEmpty && !failed.isEmpty {
                    done(.failure(Failure(description: "macOS non ha dato il permesso su «\(failed.joined(separator: "», «"))»")))
                } else {
                    done(.success((folders, failed)))
                }
            }
            NSApp.activate(ignoringOtherApps: true)
            if let window = NSApp.keyWindow ?? NSApp.mainWindow ?? NSApp.windows.first(where: { $0.isVisible }) {
                panel.beginSheetModal(for: window, completionHandler: finish)
            } else {
                panel.begin(completionHandler: finish)
            }
        }
    }
}

private func envelope(_ result: [String: Any]?, _ error: String?) -> String {
    let object: [String: Any] = error.map { ["error": $0] } ?? ["result": result ?? [:]]
    let data = (try? JSONSerialization.data(withJSONObject: object, options: [.fragmentsAllowed]))
        ?? Data("{\"error\":\"Risposta non serializzabile\"}".utf8)
    return String(decoding: data, as: UTF8.self)
}

@_cdecl("aiw_folders_call")
public func aiwFoldersCall(_ id: UInt64, _ request: UnsafePointer<CChar>,
                           _ callback: @escaping @convention(c) (UInt64, UnsafePointer<CChar>) -> Void) {
    // Copy before returning to Rust, which owns the request buffer.
    let data = Data(String(cString: request).utf8)
    let reply: (String) -> Void = { text in text.withCString { callback(id, $0) } }
    guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let method = object["method"] as? String else {
        reply(envelope(nil, "Richiesta non valida")); return
    }
    let payload = object["payload"] as? [String: Any] ?? [:]
    switch method {
    case "pick":
        let message = payload["message"] as? String ?? ""
        let prompt = payload["prompt"] as? String ?? ""
        FolderGrants.pick(message: message, prompt: prompt) { result in
            switch result {
            case .success(let picked): reply(envelope(["folders": picked.folders, "failed": picked.failed], nil))
            case .failure(let failure): reply(envelope(nil, failure.description))
            }
        }
    case "open":
        // Not on the main queue: the shell reopens the folders while it starts, on the main thread.
        do {
            let opened = try FolderGrants.open(bookmark: payload["bookmark"] as? String ?? "")
            var result: [String: Any] = ["path": opened.path]
            if let renewed = opened.renewed { result["bookmark"] = renewed }
            reply(envelope(result, nil))
        } catch {
            reply(envelope(nil, "\(error)"))
        }
    case "release":
        if let path = payload["path"] as? String { FolderGrants.release(path: path) }
        reply(envelope([:], nil))
    case "realHome":
        reply(envelope(FolderGrants.realHome().map { ["path": $0] } ?? [:], nil))
    default:
        reply(envelope(nil, "Operazione sulle cartelle sconosciuta"))
    }
}
