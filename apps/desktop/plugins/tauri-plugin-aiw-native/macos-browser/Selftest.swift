// Compile with Browser.swift and run as a signed sandboxed .app (see selftest.sh).
import AppKit
import Foundation
import Network

private var callbacks: [UInt64: ([String: Any]) -> Void] = [:]
private var sequence: UInt64 = 0
private func response(_ id: UInt64, _ text: UnsafePointer<CChar>) {
    let value = try! JSONSerialization.jsonObject(with: Data(String(cString: text).utf8)) as! [String: Any]
    callbacks.removeValue(forKey: id)?(value)
}
private func call(_ method: String, _ payload: [String: Any] = [:], _ done: @escaping ([String: Any]) -> Void) {
    sequence += 1
    callbacks[sequence] = done
    let data = try! JSONSerialization.data(withJSONObject: ["method": method, "payload": payload])
    String(decoding: data, as: UTF8.self).withCString { aiwBrowserCall(sequence, $0, response) }
}
private func check(_ yes: Bool, _ message: String) {
    if !yes { fputs("FAIL: \(message)\n", stderr); exit(1) }
    print("PASS: \(message)")
}

private func pipe(_ source: NWConnection, _ destination: NWConnection) {
    source.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, complete, error in
        if let data = data, !data.isEmpty {
            destination.send(content: data, completion: .contentProcessed { _ in
                if complete || error != nil { source.cancel(); destination.cancel() }
                else { pipe(source, destination) }
            })
        } else { source.cancel(); destination.cancel() }
    }
}

@main
struct Selftest {
    static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        // A local refusing CONNECT proxy proves that WebKit uses the configured policy proxy.
        let listener = try NWListener(using: .tcp, on: .any)
        let online = CommandLine.arguments.contains("--online")
        var proxyHit = false
        listener.newConnectionHandler = { connection in
            connection.start(queue: .main)
            connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { data, _, _, _ in
                let text = String(decoding: data ?? Data(), as: UTF8.self)
                proxyHit = proxyHit || text.contains("CONNECT example.com:443")
                if online && text.contains("CONNECT example.com:443") {
                    let upstream = NWConnection(host: "example.com", port: 443, using: .tcp)
                    upstream.stateUpdateHandler = { state in
                        if case .ready = state {
                            connection.send(content: Data("HTTP/1.1 200 Connection established\r\n\r\n".utf8), completion: .contentProcessed { _ in
                                pipe(connection, upstream)
                                pipe(upstream, connection)
                            })
                        } else if case .failed = state { connection.cancel(); upstream.cancel() }
                    }
                    upstream.start(queue: .main)
                    return
                }
                connection.send(content: Data("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".utf8), completion: .contentProcessed { _ in connection.cancel() })
            }
        }
        listener.stateUpdateHandler = { state in
            guard case .ready = state else { return }
            call("browserConfigure", ["proxyPort": Int(listener.port!.rawValue)]) { value in
                check(value["error"] == nil, "configure in App Sandbox")
                call("browserNavigate", ["url": "about:blank"]) { value in
                    check(value["error"] == nil, "navigate about:blank")
                    call("browserEval", ["js": "document.body.innerHTML = '<input id=\"q\"><button onclick=\"document.body.dataset.clicked=1\">Vai</button>'; document.title = 'Prova'; document.title"]) { value in
                        check((value["result"] as? [String: Any])?["value"] as? String == "Prova", "evaluate and edit DOM")
                        call("browserScreenshot") { value in
                            let shot = (value["result"] as? [String: Any])?["dataUrl"] as? String ?? ""
                            check(shot.hasPrefix("data:image/png;base64,") && shot.count > 100, "screenshot")
                            call("browserNavigate", ["url": "file:///etc/passwd"]) { value in
                                check(value["error"] != nil, "refuse file navigation")
                                call("browserNavigate", ["url": "https://example.com"]) { value in
                                    check(proxyHit, "HTTPS connection passes through configured proxy")
                                    if online { check(value["error"] == nil, "public HTTPS page loads in sandbox") }
                                    call("browserClose") { value in
                                        check(value["error"] == nil, "close")
                                        call("browserShow") { value in
                                            check(value["error"] == nil, "reopen")
                                            call("browserNavigate", ["url": "about:blank"]) { value in
                                                check(value["error"] == nil, "navigate after reopen")
                                                print("All sandbox browser checks passed")
                                                listener.cancel()
                                                exit(0)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        listener.start(queue: .main)
        DispatchQueue.main.asyncAfter(deadline: .now() + 65) { check(false, "selftest timed out") }
        app.run()
    }
}
