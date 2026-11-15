// Compile with Folders.swift and run as a signed sandboxed .app (see selftest.sh).
//
//   (no argument)      checks that need no person: the sandbox, the bookmark API, errors
//   --save             also writes a permission for a folder of the container to `bookmark.txt`
//   --load             reopens the permission saved by `--save` in ANOTHER launch (what the app does at start)
//   --pick             opens the real macOS panel: choose a folder (try Downloads), then the app lists it from
//                      inside the sandbox, saves its permission and (with --load later) reopens it
import AppKit
import Foundation

private var pending: [UInt64: ([String: Any]) -> Void] = [:]
private var sequence: UInt64 = 0
private func response(_ id: UInt64, _ text: UnsafePointer<CChar>) {
    let value = try! JSONSerialization.jsonObject(with: Data(String(cString: text).utf8)) as! [String: Any]
    pending.removeValue(forKey: id)?(value)
}
/// The C ABI the Rust side calls.
private func call(_ method: String, _ payload: [String: Any] = [:], _ done: @escaping ([String: Any]) -> Void) {
    sequence += 1
    pending[sequence] = done
    let data = try! JSONSerialization.data(withJSONObject: ["method": method, "payload": payload])
    String(decoding: data, as: UTF8.self).withCString { aiwFoldersCall(sequence, $0, response) }
}
private func callNow(_ method: String, _ payload: [String: Any] = [:]) -> [String: Any] {
    var out: [String: Any] = [:]
    call(method, payload) { out = $0 }
    return out
}
private func check(_ yes: Bool, _ message: String) {
    if !yes { fputs("FAIL: \(message)\n", stderr); exit(1) }
    print("PASS: \(message)")
}
private let bookmarkFile = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("bookmark.txt")
private let home = URL(fileURLWithPath: NSHomeDirectory())

private func list(_ path: String) -> [String]? {
    try? FileManager.default.contentsOfDirectory(atPath: path)
}

@main
struct FoldersSelftest {
    static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let args = CommandLine.arguments

        check(ProcessInfo.processInfo.environment["APP_SANDBOX_CONTAINER_ID"] != nil, "the process runs in the App Sandbox")
        let realHome = callNow("realHome")["result"].flatMap { ($0 as? [String: Any])?["path"] as? String }
        check(realHome != nil && realHome != NSHomeDirectory(), "the real home (\(realHome ?? "?")) is not the home of the container")

        if args.contains("--load") {
            // Another launch: nothing was picked here, the saved permission has to be enough.
            let bookmark = try String(contentsOf: bookmarkFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
            let r = callNow("open", ["bookmark": bookmark])
            check(r["error"] == nil, "reopening the saved permission works in a new launch: \(r["error"] ?? "ok")")
            let path = (r["result"] as! [String: Any])["path"] as! String
            check(FolderGrants.isHeld(path: path), "its access is held until it is released")
            let entries = list(path)
            check(entries != nil, "the folder can be listed in a new launch: \(entries?.count ?? -1) entries")
            if args.contains("--pick") { print("LISTED \(path): \(entries!.prefix(5))") }
            _ = callNow("release", ["path": path])
            check(!FolderGrants.isHeld(path: path), "released")
            print("DONE")
            return
        }

        if args.contains("--pick") {
            print("Choose a folder in the panel (for example Downloads)…")
            var picked: [String: Any]?
            call("pick", ["message": "AI Workspace: prova delle cartelle consentite", "prompt": "Consenti"]) { picked = $0 }
            while picked == nil { RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.2)) }
            check(picked!["error"] == nil, "the panel answered: \(picked!["error"] ?? "ok")")
            let folders = (picked!["result"] as! [String: Any])["folders"] as! [[String: String]]
            check(!folders.isEmpty, "a folder was chosen (the panel was not cancelled)")
            let f = folders[0]
            let entries = list(f["path"]!)
            check(entries != nil, "the chosen folder can be listed from inside the sandbox: \(entries?.count ?? -1) entries")
            try f["bookmark"]!.write(to: bookmarkFile, atomically: true, encoding: .utf8)
            print("SAVED \(f["path"]!); now run again with --load --pick to see that it is reopened without the panel")
            return
        }

        // 1. The sandbox refuses what the app was not given: the whole reason for this feature.
        let downloads = URL(fileURLWithPath: realHome!).appendingPathComponent("Downloads").path
        check(list(downloads) == nil, "the sandbox refuses the real Downloads folder before a folder is granted")

        // 2. A folder the app owns (its container) stands in for a picked one: the bookmark API is the same.
        let dir = home.appendingPathComponent("granted-\(UUID().uuidString.prefix(8))")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try "ciao".write(to: dir.appendingPathComponent("a.txt"), atomically: true, encoding: .utf8)
        let bookmark = try FolderGrants.makeBookmark(dir)
        check(Data(base64Encoded: bookmark) != nil && !bookmark.isEmpty, "a security-scoped bookmark can be created (entitlement app-scope)")

        let opened = callNow("open", ["bookmark": bookmark])
        check(opened["error"] == nil, "the ABI reopens it: \(opened["error"] ?? "ok")")
        let path = (opened["result"] as! [String: Any])["path"] as! String
        check(path == FolderGrants.canonical(dir), "it reopens at the canonical path")
        check((opened["result"] as! [String: Any])["bookmark"] == nil, "a fresh bookmark is not stale: nothing to renew")
        check(FolderGrants.isHeld(path: path), "its access is held")
        check(list(path) == ["a.txt"], "the folder can be read through it")
        // A second open of the same folder does not start the access twice, and one release gives it up.
        _ = callNow("open", ["bookmark": bookmark])
        _ = callNow("release", ["path": path])
        check(!FolderGrants.isHeld(path: path), "one release gives up the access")
        _ = callNow("release", ["path": "/nowhere"])
        check(true, "releasing a path that is not held is harmless")
        check(callNow("open", ["bookmark": bookmark])["error"] == nil, "it can be reopened after a release")
        _ = callNow("release", ["path": path])

        if args.contains("--save") {
            let saved = home.appendingPathComponent("saved-folder")
            try? FileManager.default.removeItem(at: saved)
            try FileManager.default.createDirectory(at: saved, withIntermediateDirectories: true)
            try "dentro".write(to: saved.appendingPathComponent("b.txt"), atomically: true, encoding: .utf8)
            try FolderGrants.makeBookmark(saved).write(to: bookmarkFile, atomically: true, encoding: .utf8)
            print("SAVED permission for \(saved.path)")
        }

        // 3. What goes wrong says why, in Italian, and nothing keeps the access open.
        check((callNow("open", ["bookmark": "%%%non-base64"])["error"] as? String)?.contains("non è leggibile") == true, "garbage is not a permission")
        let valid = Data("not a bookmark".utf8).base64EncodedString()
        check(callNow("open", ["bookmark": valid])["error"] != nil, "base64 that is not a bookmark is refused")
        try FileManager.default.removeItem(at: dir)
        let gone = callNow("open", ["bookmark": bookmark])["error"] as? String
        check(gone?.contains("non c'è più") == true, "a folder that was deleted says so: \(gone ?? "no error")")
        check(!FolderGrants.isHeld(path: path), "a failed open holds nothing")
        check(callNow("bogus")["error"] != nil, "an unknown operation is an error")
        print("ALL PASSED")
    }
}
