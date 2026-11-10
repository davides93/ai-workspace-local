import AppKit
import Foundation
import Network
import WebKit

// A plain WKWebView: remote pages have no Tauri IPC or access to the application UI.
// The C boundary always replies asynchronously on the main queue.
private typealias Reply = (Result<Any, Error>) -> Void
private func failure(_ message: String) -> NSError {
    NSError(domain: "AiwBrowser", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
}

@available(macOS 14.0, *)
private final class Browser: NSObject, WKNavigationDelegate, WKUIDelegate {
    static var shared: Browser?
    let window: NSWindow
    let web: WKWebView
    private var pending: Reply?
    private var timeout: DispatchWorkItem?
    private var activeNavigation: WKNavigation?

    init(port: UInt16) {
        let config = WKWebViewConfiguration()
        // Persistent cookies, in a data store separate from the app UI.
        let key = "aiw.browser.store"
        let identifier = UserDefaults.standard.string(forKey: key).flatMap(UUID.init(uuidString:)) ?? UUID()
        UserDefaults.standard.set(identifier.uuidString, forKey: key)
        let store = WKWebsiteDataStore(forIdentifier: identifier)
        let endpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
        store.proxyConfigurations = [ProxyConfiguration(httpCONNECTProxy: endpoint, tlsOptions: nil)]
        config.websiteDataStore = store
        config.userContentController.addUserScript(WKUserScript(
            source: "for (const k of ['RTCPeerConnection','webkitRTCPeerConnection','RTCDataChannel']) { try { Object.defineProperty(window, k, { value: undefined }); } catch (e) {} }",
            injectionTime: .atDocumentStart, forMainFrameOnly: false))
        web = WKWebView(frame: .zero, configuration: config)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 800),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init()
        window.title = "Browser dell’agente — AI Workspace"
        window.isReleasedWhenClosed = false
        window.contentView = web
        window.center()
        web.navigationDelegate = self
        web.uiDelegate = self
        web.allowsBackForwardNavigationGestures = true
    }

    func show() {
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func finish(_ result: Result<Any, Error>) {
        let reply = pending
        pending = nil
        timeout?.cancel()
        timeout = nil
        reply?(result)
    }

    func navigate(_ url: URL, reply: @escaping Reply) {
        finish(.failure(failure("Navigazione sostituita")))
        web.stopLoading()
        pending = reply
        let task = DispatchWorkItem { [weak self] in
            self?.finish(.failure(failure("La pagina non ha finito di caricarsi in tempo")))
            self?.web.stopLoading()
        }
        timeout = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 35, execute: task)
        show()
        activeNavigation = web.load(URLRequest(url: url))
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard navigation === activeNavigation else { return }
        finish(.success(["url": web.url?.absoluteString ?? "about:blank"]))
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        guard navigation === activeNavigation else { return }
        finish(.failure(error))
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        guard navigation === activeNavigation else { return }
        finish(.failure(error))
    }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        finish(.failure(failure("Il processo del browser interno si è chiuso")))
    }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let url = navigationAction.request.url
        let allowed = url?.scheme?.lowercased() == "https" || url?.absoluteString == "about:blank"
        // No downloads, external apps, file URLs or unproxied HTTP navigations.
        decisionHandler(allowed && !navigationAction.shouldPerformDownload ? .allow : .cancel)
        if !allowed { finish(.failure(failure("Il browser interno apre solo pagine https"))) }
    }
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if navigationAction.request.url?.scheme?.lowercased() == "https" {
            web.load(navigationAction.request)
        }
        return nil
    }

    func call(_ method: String, _ payload: [String: Any], reply: @escaping Reply) {
        switch method {
        case "browserNavigate":
            guard let text = payload["url"] as? String, let url = URL(string: text),
                  url.scheme?.lowercased() == "https" || text == "about:blank" else {
                reply(.failure(failure("URL del browser non valido"))); return
            }
            navigate(url, reply: reply)
        case "browserEval":
            guard let js = payload["js"] as? String else { reply(.failure(failure("Script mancante"))); return }
            // Isolated content world: DOM is shared, page-defined JS globals are not.
            web.evaluateJavaScript(js, in: nil, in: .defaultClient) { result in
                reply(result.map { ["value": $0] })
            }
        case "browserScreenshot":
            let config = WKSnapshotConfiguration()
            config.snapshotWidth = 1280
            web.takeSnapshot(with: config) { image, error in
                if let error = error { reply(.failure(error)); return }
                guard let data = image?.tiffRepresentation, let bitmap = NSBitmapImageRep(data: data),
                      let png = bitmap.representation(using: .png, properties: [:]) else {
                    reply(.failure(failure("Impossibile acquisire la pagina"))); return
                }
                reply(.success(["dataUrl": "data:image/png;base64," + png.base64EncodedString()]))
            }
        case "browserShow": show(); reply(.success([:]))
        case "browserClose":
            finish(.failure(failure("Browser chiuso")))
            activeNavigation = nil
            web.stopLoading()
            web.load(URLRequest(url: URL(string: "about:blank")!))
            window.orderOut(nil)
            reply(.success([:]))
        default: reply(.failure(failure("Operazione browser sconosciuta")))
        }
    }
}

@_cdecl("aiw_browser_call")
public func aiwBrowserCall(_ id: UInt64, _ request: UnsafePointer<CChar>,
                          _ callback: @escaping @convention(c) (UInt64, UnsafePointer<CChar>) -> Void) {
    // Copy before returning to Rust, which owns the request buffer.
    let data = Data(String(cString: request).utf8)
    DispatchQueue.main.async {
        let reply: Reply = { result in
            let envelope: [String: Any]
            switch result {
            case .success(let value): envelope = ["result": value]
            case .failure(let error): envelope = ["error": error.localizedDescription]
            }
            let encoded = (try? JSONSerialization.data(withJSONObject: envelope, options: [.fragmentsAllowed]))
                ?? Data("{\"error\":\"Risposta del browser non serializzabile\"}".utf8)
            String(decoding: encoded, as: UTF8.self).withCString { callback(id, $0) }
        }
        guard #available(macOS 14.0, *) else {
            reply(.failure(failure("Il browser interno richiede macOS 14 o successivo"))); return
        }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let method = object["method"] as? String else {
            reply(.failure(failure("Richiesta browser non valida"))); return
        }
        let payload = object["payload"] as? [String: Any] ?? [:]
        if method == "browserConfigure" {
            guard let port = payload["proxyPort"] as? Int, port > 0, port <= 65535 else {
                reply(.failure(failure("Porta proxy non valida"))); return
            }
            Browser.shared = Browser(port: UInt16(port))
            Browser.shared?.show()
            reply(.success([:]))
        } else if let browser = Browser.shared {
            browser.call(method, payload, reply: reply)
        } else {
            reply(.failure(failure("Browser interno non configurato")))
        }
    }
}
