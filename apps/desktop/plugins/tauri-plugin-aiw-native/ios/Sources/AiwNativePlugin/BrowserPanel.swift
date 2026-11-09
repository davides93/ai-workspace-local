import Foundation
import Network
import Tauri
import UIKit
import WebKit

/// In-app browser for the agent tools (`browser_*`) on iPhone and iPad.
///
/// A plain `WKWebView` (never a Tauri webview: pages must not reach the app's IPC) in a sheet the
/// user can see, stop and use (logins). All its traffic goes through the policy proxy that the
/// Rust side runs on 127.0.0.1: the proxy decides every HTTPS connection, so the agent's browser
/// cannot reach the LAN or loopback. Needs iOS 17 (`proxyConfigurations`); older systems get a
/// clear error and the browser tools stay unavailable.
@available(iOS 17.0, *)
final class BrowserPanel: UIViewController, WKNavigationDelegate, WKUIDelegate {
    private var webView: WKWebView!
    private var pending: ((Result<String, Error>) -> Void)?
    private var timeout: DispatchWorkItem?
    private let proxyPort: UInt16

    init(proxyPort: UInt16) {
        self.proxyPort = proxyPort
        super.init(nibName: nil, bundle: nil)
        modalPresentationStyle = .pageSheet
        if let sheet = sheetPresentationController {
            sheet.detents = [.medium(), .large()]
            sheet.prefersGrabberVisible = true
            sheet.largestUndimmedDetentIdentifier = .medium
        }
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        let config = WKWebViewConfiguration()
        let store = WKWebsiteDataStore(forIdentifier: BrowserPanel.storeIdentifier())
        if let port = NWEndpoint.Port(rawValue: proxyPort) {
            let endpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: port)
            store.proxyConfigurations = [ProxyConfiguration(httpCONNECTProxy: endpoint, tlsOptions: nil)]
        }
        config.websiteDataStore = store
        // No WebRTC: it would bypass the proxy.
        let noRtc = WKUserScript(
            source: "for (const k of ['RTCPeerConnection','webkitRTCPeerConnection','RTCDataChannel']) { try { Object.defineProperty(window, k, { value: undefined }); } catch (e) {} }",
            injectionTime: .atDocumentStart, forMainFrameOnly: false)
        config.userContentController.addUserScript(noRtc)
        webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.translatesAutoresizingMaskIntoConstraints = false

        let bar = UIView()
        bar.translatesAutoresizingMaskIntoConstraints = false
        let label = UILabel()
        label.text = "Browser dell'agente"
        label.font = .preferredFont(forTextStyle: .footnote)
        label.textColor = .secondaryLabel
        label.translatesAutoresizingMaskIntoConstraints = false
        let stop = UIButton(type: .system)
        stop.setTitle("Chiudi", for: .normal)
        stop.addAction(UIAction { [weak self] _ in self?.dismiss(animated: true) }, for: .touchUpInside)
        stop.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(label)
        bar.addSubview(stop)
        view.addSubview(bar)
        view.addSubview(webView)
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: view.topAnchor, constant: 8),
            bar.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            bar.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            bar.heightAnchor.constraint(equalToConstant: 36),
            label.leadingAnchor.constraint(equalTo: bar.leadingAnchor),
            label.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
            stop.trailingAnchor.constraint(equalTo: bar.trailingAnchor),
            stop.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
            webView.topAnchor.constraint(equalTo: bar.bottomAnchor),
            webView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            webView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            webView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    /// A fixed data store for the agent's browser, separate from the app's own web data.
    static func storeIdentifier() -> UUID {
        let key = "aiw.browser.store"
        if let s = UserDefaults.standard.string(forKey: key), let id = UUID(uuidString: s) { return id }
        let id = UUID()
        UserDefaults.standard.set(id.uuidString, forKey: key)
        return id
    }

    // MARK: operations (main thread)

    func navigate(_ url: URL, completion: @escaping (Result<String, Error>) -> Void) {
        finish(.failure(NSError(domain: "aiw.browser", code: 1, userInfo: [NSLocalizedDescriptionKey: "Navigazione sostituita da una nuova"])))
        pending = completion
        let work = DispatchWorkItem { [weak self] in
            // Slow page: answer with what is there.
            self?.finish(.success(self?.webView.url?.absoluteString ?? url.absoluteString))
        }
        timeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 30, execute: work)
        webView.load(URLRequest(url: url))
    }

    func evaluate(_ js: String, completion: @escaping (Result<String, Error>) -> Void) {
        // The completion handler can be lost if the page's process dies: never wait forever.
        var done = false
        let finishOnce: (Result<String, Error>) -> Void = { r in
            if done { return }
            done = true
            completion(r)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 15) {
            finishOnce(.failure(NSError(domain: "aiw.browser", code: 3, userInfo: [NSLocalizedDescriptionKey: "La pagina non risponde"])))
        }
        webView.evaluateJavaScript(js) { value, error in
            if let error = error { finishOnce(.failure(error)); return }
            if let s = value as? String { finishOnce(.success(s)) } else { finishOnce(.success("")) }
        }
    }

    func screenshot(completion: @escaping (Result<String, Error>) -> Void) {
        let config = WKSnapshotConfiguration()
        config.snapshotWidth = 1280
        webView.takeSnapshot(with: config) { image, error in
            if let error = error { completion(.failure(error)); return }
            guard let data = image?.jpegData(compressionQuality: 0.7) else {
                completion(.failure(NSError(domain: "aiw.browser", code: 2, userInfo: [NSLocalizedDescriptionKey: "Screenshot non riuscito"])))
                return
            }
            completion(.success("data:image/jpeg;base64," + data.base64EncodedString()))
        }
    }

    private func finish(_ result: Result<String, Error>) {
        timeout?.cancel()
        timeout = nil
        let done = pending
        pending = nil
        done?(result)
    }

    // MARK: WKNavigationDelegate

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        finish(.success(webView.url?.absoluteString ?? ""))
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        finish(.failure(error))
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        finish(.failure(error))
    }

    /// Second line of defence after the proxy: only https (and about:blank) loads; no app schemes.
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let scheme = navigationAction.request.url?.scheme?.lowercased() ?? ""
        decisionHandler(["https", "about"].contains(scheme) ? .allow : .cancel)
    }

    // MARK: WKUIDelegate

    /// `target=_blank` and `window.open`: stay in the same page (one tab, one thing to watch).
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if let url = navigationAction.request.url { webView.load(URLRequest(url: url)) }
        return nil
    }
}

class BrowserConfigureArgs: Decodable { let proxyPort: Int }
class BrowserNavigateArgs: Decodable { let url: String }
class BrowserEvalArgs: Decodable { let js: String }

/// Commands called from Rust (`browserConfigure`, `browserNavigate`, `browserEval`,
/// `browserScreenshot`, `browserClose`). Rust blocks until each one answers.
extension AiwNativePlugin {
    private static var panel: UIViewController?
    private static var proxyPort: UInt16 = 0

    private func topController() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first
        var vc = scene?.windows.first(where: { $0.isKeyWindow })?.rootViewController
        while let next = vc?.presentedViewController { vc = next }
        return vc
    }

    /// The panel, created and shown on demand (it keeps its page while the app lives).
    @available(iOS 17.0, *)
    private func withPanel(_ invoke: Invoke, _ body: @escaping (BrowserPanel) -> Void) {
        DispatchQueue.main.async {
            if let existing = AiwNativePlugin.panel as? BrowserPanel, existing.presentingViewController != nil || existing.isBeingPresented {
                body(existing)
                return
            }
            guard AiwNativePlugin.proxyPort != 0 else { invoke.reject("Browser non configurato"); return }
            guard let host = self.topController() else { invoke.reject("Nessuna schermata su cui mostrare il browser"); return }
            let panel = BrowserPanel(proxyPort: AiwNativePlugin.proxyPort)
            AiwNativePlugin.panel = panel
            var shown = false
            // If the system never completes the presentation, say so instead of leaving the agent waiting.
            DispatchQueue.main.asyncAfter(deadline: .now() + 10) {
                if !shown {
                    shown = true
                    AiwNativePlugin.panel = nil
                    invoke.reject("Il pannello del browser non si è aperto (presentazione di iOS non completata)")
                }
            }
            host.present(panel, animated: true) {
                if shown { return }
                shown = true
                // `viewDidLoad` has run: the web view exists.
                body(panel)
            }
        }
    }

    @objc public func browserConfigure(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(BrowserConfigureArgs.self)
        guard #available(iOS 17.0, *) else {
            invoke.reject("Il browser dell'agente richiede iOS 17 o successivo")
            return
        }
        AiwNativePlugin.proxyPort = UInt16(clamping: args.proxyPort)
        invoke.resolve()
    }

    @objc public func browserNavigate(_ invoke: Invoke) throws {
        guard #available(iOS 17.0, *) else { invoke.reject("Il browser dell'agente richiede iOS 17 o successivo"); return }
        let args = try invoke.parseArgs(BrowserNavigateArgs.self)
        guard let url = URL(string: args.url) else { invoke.reject("URL non valido"); return }
        withPanel(invoke) { panel in
            panel.navigate(url) { result in
                switch result {
                case .success(let final): invoke.resolve(["url": final])
                case .failure(let error): invoke.reject(error.localizedDescription)
                }
            }
        }
    }

    @objc public func browserEval(_ invoke: Invoke) throws {
        guard #available(iOS 17.0, *) else { invoke.reject("Il browser dell'agente richiede iOS 17 o successivo"); return }
        let args = try invoke.parseArgs(BrowserEvalArgs.self)
        withPanel(invoke) { panel in
            panel.evaluate(args.js) { result in
                switch result {
                case .success(let value): invoke.resolve(["value": value])
                case .failure(let error): invoke.reject(error.localizedDescription)
                }
            }
        }
    }

    @objc public func browserScreenshot(_ invoke: Invoke) {
        guard #available(iOS 17.0, *) else { invoke.reject("Il browser dell'agente richiede iOS 17 o successivo"); return }
        withPanel(invoke) { panel in
            panel.screenshot { result in
                switch result {
                case .success(let url): invoke.resolve(["dataUrl": url])
                case .failure(let error): invoke.reject(error.localizedDescription)
                }
            }
        }
    }

    @objc public func browserClose(_ invoke: Invoke) {
        DispatchQueue.main.async {
            AiwNativePlugin.panel?.dismiss(animated: true)
            AiwNativePlugin.panel = nil
            invoke.resolve()
        }
    }
}
