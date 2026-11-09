// AppleFMCore: everything about talking to Apple Foundation Models that the iOS plugin and the
// macOS helper (`macos-helper/main.swift`, built by build.rs together with this file) share:
// the wire types, the JSON Schema → GenerationSchema conversion, native tool calling, the
// native Transcript and the context-window fitting. No Tauri, no UIKit.
//
// Tool calling. The tools of the app live in the Rust agent loop (permissions, approvals, audit),
// not here. So every tool the agent offers becomes a `Tool` whose `call` does NOT run anything:
// it records the call the model decided to make and interrupts the session. The caller gets the
// calls as a `toolcall` event, the agent runs them and asks again with their results, which come
// back as native `toolCalls` / `toolOutput` entries of the Transcript.
//
// Stdout/Channel events (dictionaries):
//   {"type":"text","delta":"..."}                       streamed answer text
//   {"type":"toolcall","calls":[{"id","name","arguments"}]}   (arguments: JSON text)
//   {"type":"done"} | {"type":"error","message":"..."}

import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

// MARK: - JSON

enum JSONValue: Decodable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else if let v = try? c.decode([String: JSONValue].self) { self = .object(v) }
        else { throw DecodingError.dataCorruptedError(in: c, debugDescription: "JSON non valido") }
    }

    subscript(key: String) -> JSONValue? {
        if case .object(let o) = self { return o[key] }
        return nil
    }
    var stringValue: String? { if case .string(let s) = self { return s } else { return nil } }
    var arrayValue: [JSONValue]? { if case .array(let a) = self { return a } else { return nil } }
    var objectValue: [String: JSONValue]? { if case .object(let o) = self { return o } else { return nil } }
}

func afmJSONText(_ value: Any) -> String {
    guard JSONSerialization.isValidJSONObject(value) || value is String || value is NSNumber,
          let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys, .fragmentsAllowed, .withoutEscapingSlashes]),
          let s = String(data: data, encoding: .utf8) else { return "{}" }
    return s
}

func afmParseJSON(_ text: String) -> Any? {
    guard let d = text.data(using: .utf8) else { return nil }
    return try? JSONSerialization.jsonObject(with: d, options: [.fragmentsAllowed])
}

// MARK: - Wire types

struct AFMToolSpec: Decodable {
    let name: String
    let description: String?
    let parameters: JSONValue?
}

struct AFMCall: Decodable {
    let id: String?
    let name: String
    /// JSON text of the arguments.
    let arguments: String
}

/// One message of the conversation so far: `user`, `assistant` (with the tool calls it made) or
/// `tool` (the result of one call, by `id`).
struct AFMTurn: Decodable {
    let role: String
    let text: String?
    let toolCalls: [AFMCall]?
    let id: String?
    let name: String?
}

struct AFMRequest: Decodable {
    let system: String
    /// The message to answer. With a `transcript` it comes after it; without one, it is all.
    let prompt: String
    let temperature: Double?
    let maxTokens: Int?
    let tools: [AFMToolSpec]?
    let transcript: [AFMTurn]?
    /// Context window of the system model, when the caller knows better than the default.
    let contextTokens: Int?

    init(system: String, prompt: String, temperature: Double? = nil, maxTokens: Int? = nil, tools: [AFMToolSpec]? = nil,
         transcript: [AFMTurn]? = nil, contextTokens: Int? = nil) {
        self.system = system
        self.prompt = prompt
        self.temperature = temperature
        self.maxTokens = maxTokens
        self.tools = tools
        self.transcript = transcript
        self.contextTokens = contextTokens
    }
}

typealias AFMEmit = ([String: Any]) -> Void

// MARK: - Errors

/// Human message for a failure, from the error's description (the case names are stable even
/// when their payloads change between SDKs).
func afmDescribe(_ error: Error) -> String {
    let d = "\(error)"
    if afmIsOverflow(error) {
        return "La conversazione supera la finestra di contesto del modello di sistema Apple. Inizia una nuova conversazione o usa un altro runtime."
    }
    if d.contains("guardrailViolation") {
        return "Apple Intelligence ha rifiutato la richiesta (filtri di sicurezza del modello di sistema)."
    }
    if d.contains("unsupportedLanguageOrLocale") {
        return "Apple Intelligence non supporta la lingua di questa richiesta."
    }
    if d.contains("assetsUnavailable") {
        return "Il modello di Apple Intelligence non è ancora scaricato: aprilo da Impostazioni di Sistema → Apple Intelligence e Siri."
    }
    if d.contains("rateLimited") || d.contains("concurrentRequests") {
        return "Il modello di sistema Apple è occupato: riprova tra poco."
    }
    let ns = error as NSError
    return "Apple Intelligence non è riuscita a rispondere. Riprova e, se il problema continua, controlla Apple Intelligence e Siri nelle Impostazioni di Sistema. (\(ns.domain), codice \(ns.code))"
}

func afmIsOverflow(_ error: Error) -> Bool {
    let d = "\(error)"
    return d.contains("exceededContextWindowSize") || d.contains("contextSizeExceeded")
}

// MARK: - Token estimates and fitting

/// Rough token count, measured against `SystemLanguageModel.tokenCount` (26.4+, not usable
/// while building for 26.0): letters and spaces cost about a third of a token each, punctuation
/// nearly one (JSON and code split into many small pieces). Slightly pessimistic on purpose;
/// an overflow that gets past it is retried with a tighter budget.
func afmTokens(_ s: String) -> Int {
    var plain = 0
    var punct = 0
    for u in s.unicodeScalars {
        if u.properties.isAlphabetic || u.properties.numericType != nil || u.properties.isWhitespace { plain += 1 } else { punct += 1 }
    }
    return plain * 10 / 34 + punct * 9 / 10 + 1
}

/// Context window of the system model: 4096 tokens up to iOS / macOS 26.x, larger from 27 (8192
/// today). `contextSize` exists only in newer SDKs, so this mirrors its own fallback; the caller
/// can override it, and an overflow is retried with less.
func afmDefaultWindow() -> Int {
    if #available(iOS 27.0, macOS 27.0, *) { return 8192 }
    return 4096
}

private func afmClip(_ s: String, _ maxChars: Int) -> String {
    guard s.count > maxChars, maxChars > 40 else { return s }
    let head = maxChars * 2 / 3
    let tail = maxChars - head
    return String(s.prefix(head)) + "\n… [\(s.count - maxChars) caratteri omessi] …\n" + String(s.suffix(tail))
}

private func afmTurnTokens(_ t: AFMTurn) -> Int {
    var n = 8 + afmTokens(t.text ?? "")
    for c in t.toolCalls ?? [] { n += 8 + afmTokens(c.name) + afmTokens(c.arguments) }
    return n
}

struct AFMFitted {
    var system: String
    var turns: [AFMTurn]
    var prompt: String
    /// Turns dropped from the start to make it fit.
    var dropped: Int
}

/// Makes system + history + prompt fit `budget` tokens (what is left after the tools and the room
/// for the answer): long outputs are clipped, then the oldest exchanges go, never the latest.
func afmFit(system: String, turns: [AFMTurn], prompt: String, budget: Int) -> AFMFitted {
    var system = system
    var prompt = prompt
    var turns = turns
    var dropped = 0

    func total() -> Int { afmTokens(system) + afmTokens(prompt) + turns.reduce(0) { $0 + afmTurnTokens($1) } }

    // A message cannot take more than its share.
    prompt = afmClip(prompt, max(400, budget * 3 / 2))
    var cap = 1800
    func clipTurns(_ cap: Int) {
        turns = turns.map { t in
            AFMTurn(role: t.role, text: t.text.map { afmClip($0, cap) }, toolCalls: t.toolCalls.map { calls in
                calls.map { AFMCall(id: $0.id, name: $0.name, arguments: afmClip($0.arguments, cap)) }
            }, id: t.id, name: t.name)
        }
    }
    clipTurns(cap)
    if afmTokens(system) > budget / 2 { system = afmClip(system, budget / 2 * 3) }

    // Oldest exchange first; an exchange starts at a user message, so a call is never separated
    // from its result.
    while total() > budget {
        let starts = turns.indices.filter { turns[$0].role == "user" }
        guard let second = starts.dropFirst().first ?? (starts.first == 0 ? nil : starts.first), second > 0 else { break }
        turns.removeFirst(second)
        dropped += second
    }
    while total() > budget, cap > 150 {
        cap /= 2
        clipTurns(cap)
    }
    if total() > budget {
        // Last resort: even the latest exchange is too big. Keep only the prompt.
        dropped += turns.count
        turns = []
    }
    if total() > budget {
        system = afmClip(system, max(300, (budget - afmTokens(prompt)) * 3))
    }
    return AFMFitted(system: system, turns: turns, prompt: prompt, dropped: dropped)
}

// MARK: - Argument shapes

/// How a tool's arguments were mapped to a schema the model can fill, so that the call can be
/// brought back to what the tool expects (and a past call forward to what the schema says).
indirect enum AFMShape: Sendable {
    case plain
    /// Free-form JSON the model cannot be given a schema for: written as a string, parsed back.
    case jsonString
    /// The whole argument object, written as a string in `{"arguments": "<json>"}`.
    case wrapper
    case object([String: AFMShape])
    case array(AFMShape)
}

func afmRestore(_ value: Any, _ shape: AFMShape) -> Any {
    switch shape {
    case .plain: return value
    case .jsonString:
        if let s = value as? String, let parsed = afmParseJSON(s) { return parsed }
        return value
    case .wrapper:
        if let d = value as? [String: Any], let inner = d["arguments"] { return afmRestore(inner, .jsonString) }
        return value
    case .object(let props):
        guard var d = value as? [String: Any] else { return value }
        for (k, s) in props { if let v = d[k] { d[k] = afmRestore(v, s) } }
        return d
    case .array(let s):
        guard let a = value as? [Any] else { return value }
        return a.map { afmRestore($0, s) }
    }
}

func afmStringify(_ value: Any, _ shape: AFMShape) -> Any {
    switch shape {
    case .plain: return value
    case .jsonString: return value is String ? value : afmJSONText(value)
    case .wrapper: return ["arguments": afmStringify(value, .jsonString)]
    case .object(let props):
        guard var d = value as? [String: Any] else { return value }
        for (k, s) in props { if let v = d[k] { d[k] = afmStringify(v, s) } }
        return d
    case .array(let s):
        guard let a = value as? [Any] else { return value }
        return a.map { afmStringify($0, s) }
    }
}

/// Name the model sees: letters, digits and underscores only.
func afmSanitizeName(_ name: String) -> String {
    var out = String(name.map { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "_") ? $0 : "_" }.prefix(48))
    if out.isEmpty || !(out.first!.isLetter) { out = "t_" + out }
    return out
}

#if canImport(FoundationModels)

// MARK: - JSON Schema → GenerationSchema

@available(iOS 26.0, macOS 26.0, *)
struct AFMSchemaBuilder {
    private var used = Set<String>()
    private let maxDepth = 4

    mutating func unique(_ base: String) -> String {
        var n = afmSanitizeName(base)
        var i = 2
        while used.contains(n) { n = "\(afmSanitizeName(base))_\(i)"; i += 1 }
        used.insert(n)
        return n
    }

    static func shortDescription(_ s: String?, _ max: Int) -> String? {
        guard let s = s?.trimmingCharacters(in: .whitespacesAndNewlines), !s.isEmpty else { return nil }
        let first = s.components(separatedBy: "\n").first ?? s
        return first.count > max ? String(first.prefix(max - 1)) + "…" : first
    }

    /// The node to use for `j`: a union (`anyOf`/`oneOf`) becomes its first non-null variant.
    private func resolved(_ j: JSONValue) -> JSONValue {
        for key in ["anyOf", "oneOf"] {
            if let variants = j[key]?.arrayValue,
               let pick = variants.first(where: { $0["type"]?.stringValue != "null" && $0["type"] != nil }) ?? variants.first {
                var merged = pick.objectValue ?? [:]
                if merged["description"] == nil, let d = j["description"] { merged["description"] = d }
                return .object(merged)
            }
        }
        return j
    }

    private func kind(_ j: JSONValue) -> String? {
        switch j["type"] {
        case .string(let s): return s
        case .array(let a): return a.compactMap { $0.stringValue }.first { $0 != "null" }
        default:
            if j["properties"] != nil { return "object" }
            if j["items"] != nil { return "array" }
            if j["enum"] != nil { return "string" }
            return nil
        }
    }

    mutating func build(_ node: JSONValue?, name: String, depth: Int) -> (DynamicGenerationSchema, AFMShape) {
        guard let node = node.map(resolved) else { return (DynamicGenerationSchema(type: String.self), .jsonString) }
        switch kind(node) {
        case "object":
            guard depth < maxDepth, let props = node["properties"]?.objectValue, !props.isEmpty else {
                return (DynamicGenerationSchema(type: String.self), .jsonString)
            }
            let required = Set((node["required"]?.arrayValue ?? []).compactMap { $0.stringValue })
            let objName = unique(name)
            var shapes: [String: AFMShape] = [:]
            var properties: [DynamicGenerationSchema.Property] = []
            // Required properties first: the model fills them in that order.
            let keys = props.keys.sorted { (required.contains($0) ? 0 : 1, $0) < (required.contains($1) ? 0 : 1, $1) }
            for k in keys {
                let pj = props[k]!
                let (schema, shape) = build(pj, name: "\(name)_\(k)", depth: depth + 1)
                shapes[k] = shape
                var desc = Self.shortDescription(resolved(pj)["description"]?.stringValue, 90)
                switch shape {
                case .jsonString: desc = (desc.map { $0 + " " } ?? "") + "(JSON come stringa)"
                case .array(.jsonString): desc = (desc.map { $0 + " " } ?? "") + "(ogni elemento è un oggetto JSON scritto come stringa)"
                default: break
                }
                properties.append(.init(name: k, description: desc, schema: schema, isOptional: !required.contains(k)))
            }
            return (DynamicGenerationSchema(name: objName, description: Self.shortDescription(node["description"]?.stringValue, 90), properties: properties), .object(shapes))
        case "array":
            let (item, shape) = build(node["items"] ?? .object(["type": .string("string")]), name: "\(name)_item", depth: depth + 1)
            return (DynamicGenerationSchema(arrayOf: item), .array(shape))
        case "integer": return (DynamicGenerationSchema(type: Int.self), .plain)
        case "number": return (DynamicGenerationSchema(type: Double.self), .plain)
        case "boolean": return (DynamicGenerationSchema(type: Bool.self), .plain)
        case "string":
            if let values = node["enum"]?.arrayValue?.compactMap({ $0.stringValue }), !values.isEmpty, values.count <= 40 {
                return (DynamicGenerationSchema(name: unique(name), description: nil, anyOf: values), .plain)
            }
            return (DynamicGenerationSchema(type: String.self), .plain)
        default:
            return (DynamicGenerationSchema(type: String.self), .jsonString)
        }
    }

    /// The schema of a tool's arguments. Arguments the model cannot be given a schema for are
    /// wrapped in one string property; a tool with no parameters gets an empty object.
    static func toolParameters(_ spec: AFMToolSpec) -> (GenerationSchema, AFMShape)? {
        let params = spec.parameters
        let hasProps = !(params?["properties"]?.objectValue ?? [:]).isEmpty
        if hasProps {
            var b = AFMSchemaBuilder()
            let (root, shape) = b.build(params, name: afmSanitizeName(spec.name), depth: 0)
            if let schema = try? GenerationSchema(root: root, dependencies: []) { return (schema, shape) }
        } else if params == nil || (params?["type"]?.stringValue ?? "object") == "object", params?["additionalProperties"] == nil {
            let root = DynamicGenerationSchema(name: afmSanitizeName(spec.name), description: nil, properties: [])
            if let schema = try? GenerationSchema(root: root, dependencies: []) { return (schema, .plain) }
        }
        let root = DynamicGenerationSchema(name: afmSanitizeName(spec.name), description: nil, properties: [
            .init(name: "arguments", description: "Argomenti dello strumento, come JSON in una stringa", schema: DynamicGenerationSchema(type: String.self), isOptional: false),
        ])
        guard let schema = try? GenerationSchema(root: root, dependencies: []) else { return nil }
        return (schema, .wrapper)
    }
}

// MARK: - Tools that report instead of running

/// Calls the model decided to make, collected while the session runs.
actor AFMCollector {
    private(set) var calls: [AFMCall] = []
    func record(name: String, arguments: String) {
        if calls.contains(where: { $0.name == name && $0.arguments == arguments }) { return }
        calls.append(AFMCall(id: nil, name: name, arguments: arguments))
    }
}

struct AFMInterrupt: Error {}

@available(iOS 26.0, macOS 26.0, *)
struct AFMBridgeTool: Tool {
    /// Name the model sees (sanitised); `original` is the agent's.
    let name: String
    let original: String
    let description: String
    let parameters: GenerationSchema
    let shape: AFMShape
    let collector: AFMCollector

    func call(arguments: GeneratedContent) async throws -> String {
        let value = afmParseJSON(arguments.jsonString) ?? [String: Any]()
        await collector.record(name: original, arguments: afmJSONText(afmRestore(value, shape)))
        // Calls the model makes together reach us together: give the others a moment to register.
        try? await Task.sleep(nanoseconds: 80_000_000)
        throw AFMInterrupt()
    }
}

@available(iOS 26.0, macOS 26.0, *)
struct AFMToolSet {
    let tools: [AFMBridgeTool]
    /// Tools that could not be offered (their schema was refused).
    let skipped: [String]
    func tool(named n: String) -> AFMBridgeTool? { tools.first { $0.original == n || $0.name == n } }

    init(_ specs: [AFMToolSpec], collector: AFMCollector) {
        var tools: [AFMBridgeTool] = []
        var skipped: [String] = []
        var names = Set<String>()
        for spec in specs {
            guard let (schema, shape) = AFMSchemaBuilder.toolParameters(spec) else { skipped.append(spec.name); continue }
            var name = afmSanitizeName(spec.name)
            var i = 2
            while names.contains(name) { name = "\(afmSanitizeName(spec.name))_\(i)"; i += 1 }
            names.insert(name)
            tools.append(AFMBridgeTool(
                name: name, original: spec.name,
                description: AFMSchemaBuilder.shortDescription(spec.description, 160) ?? spec.name,
                parameters: schema, shape: shape, collector: collector))
        }
        self.tools = tools
        self.skipped = skipped
    }

    private init(tools: [AFMBridgeTool], skipped: [String]) {
        self.tools = tools
        self.skipped = skipped
    }

    private static func tokens(_ t: AFMBridgeTool) -> Int {
        16 + afmTokens(t.name) + afmTokens(t.description) + afmTokens("\(t.parameters)") / 2
    }

    /// Estimated tokens the model's view of these tools takes.
    var estimatedTokens: Int { tools.reduce(0) { $0 + Self.tokens($1) } }

    /// The leading tools that fit `limit` estimated tokens (the caller puts the relevant ones
    /// first); at least one is kept.
    func fitting(_ limit: Int) -> AFMToolSet {
        var kept: [AFMBridgeTool] = []
        var spent = 0
        for t in tools {
            let cost = Self.tokens(t)
            if !kept.isEmpty, spent + cost > limit { break }
            spent += cost
            kept.append(t)
        }
        return AFMToolSet(tools: kept, skipped: skipped)
    }
}

// MARK: - Transcript

@available(iOS 26.0, macOS 26.0, *)
func afmBuildTranscript(system: String, turns: [AFMTurn], tools: AFMToolSet) -> Transcript {
    func text(_ s: String) -> [Transcript.Segment] { [.text(Transcript.TextSegment(content: s))] }
    var entries: [Transcript.Entry] = [
        .instructions(Transcript.Instructions(
            segments: text(system),
            toolDefinitions: tools.tools.map { Transcript.ToolDefinition(tool: $0) })),
    ]
    // Calls and results are only valid together: a call without its result (or the other way
    // round) is left out rather than confusing the model.
    let answered = Set(turns.filter { $0.role == "tool" }.compactMap { $0.id })
    var offered = Set<String>()
    var pending: [String: String] = [:]  // id → tool name, while its result is expected
    for t in turns {
        switch t.role {
        case "user":
            entries.append(.prompt(Transcript.Prompt(segments: text(t.text ?? ""))))
        case "assistant":
            let calls = (t.toolCalls ?? []).filter { c in
                guard let id = c.id, answered.contains(id), tools.tool(named: c.name) != nil else { return false }
                return true
            }
            if calls.isEmpty {
                let s = t.text ?? ""
                if !s.isEmpty { entries.append(.response(Transcript.Response(assetIDs: [], segments: text(s)))) }
            } else {
                let items: [Transcript.ToolCall] = calls.map { c in
                    let tool = tools.tool(named: c.name)!
                    let args = afmParseJSON(c.arguments) ?? [String: Any]()
                    let content = (try? GeneratedContent(json: afmJSONText(afmStringify(args, tool.shape)))) ?? GeneratedContent(properties: [:])
                    pending[c.id!] = tool.name
                    offered.insert(c.id!)
                    return Transcript.ToolCall(id: c.id!, toolName: tool.name, arguments: content)
                }
                entries.append(.toolCalls(Transcript.ToolCalls(items)))
            }
        case "tool":
            guard let id = t.id, let name = pending.removeValue(forKey: id) else { continue }
            entries.append(.toolOutput(Transcript.ToolOutput(id: id, toolName: name, segments: text(t.text ?? ""))))
        default: break
        }
    }
    return Transcript(entries: entries)
}

// MARK: - Generation

/// Text of a snapshot that extends what was already sent, or nil when it does not (snapshots can
/// be revised; a delta cannot be taken back, so we wait until the text extends what went out).
/// Compared by unicode scalars: counting Characters breaks when a combining mark merges into
/// the previous grapheme cluster.
func afmSuffix(of text: String, after emitted: String) -> String? {
    let old = Array(emitted.unicodeScalars)
    let new = Array(text.unicodeScalars)
    guard new.count > old.count, Array(new[0..<old.count]) == old else { return nil }
    var view = String.UnicodeScalarView()
    view.append(contentsOf: new[old.count...])
    return String(view)
}

private final class AFMState: @unchecked Sendable {
    var emitted = ""
}

/// Runs one generation and reports it through `emit`; ends with `done` or `error`.
@available(iOS 26.0, macOS 26.0, *)
func afmGenerate(_ request: AFMRequest, emit: @escaping AFMEmit) async {
    // Each retry gives the tools and the history less room: the estimate may be optimistic.
    let scales: [Double] = [1.0, 0.7, 0.5, 0.35]
    let state = AFMState()
    var lastError: Error?
    for (attempt, scale) in scales.enumerated() {
        do {
            try await afmAttempt(request, scale: scale, state: state, emit: emit)
            return
        } catch {
            lastError = error
            // After text went out it cannot be taken back: only a clean failure is retried.
            if afmIsOverflow(error), state.emitted.isEmpty, attempt < scales.count - 1 { continue }
            break
        }
    }
    emit(["type": "error", "message": afmDescribe(lastError ?? AFMInterrupt())])
}

@available(iOS 26.0, macOS 26.0, *)
private func afmAttempt(_ request: AFMRequest, scale: Double, state: AFMState, emit: AFMEmit) async throws {
    let collector = AFMCollector()
    var specs = request.tools ?? []
    // The caller orders tools by relevance, so the tail is what goes first.
    if scale < 1 { specs = Array(specs.prefix(max(specs.isEmpty ? 0 : 1, Int((Double(specs.count) * scale).rounded(.up))))) }
    let window = request.contextTokens ?? afmDefaultWindow()
    // Tools may take 40% of the window; the rest is for the conversation and the answer.
    let toolSet = AFMToolSet(specs, collector: collector).fitting(window * 2 / 5)

    let answerRoom = min(request.maxTokens ?? 600, 700)
    let budget = Int(Double(window - answerRoom - 200 - toolSet.estimatedTokens) * scale)

    var options = GenerationOptions()
    if let t = request.temperature { options.temperature = t }

    let session: LanguageModelSession
    let prompt: String
    var used = toolSet.estimatedTokens
    if let turns = request.transcript {
        let fit = afmFit(system: request.system, turns: turns, prompt: request.prompt, budget: max(budget, 300))
        session = LanguageModelSession(model: .default, tools: toolSet.tools, transcript: afmBuildTranscript(system: fit.system, turns: fit.turns, tools: toolSet))
        prompt = fit.prompt
        used += afmTokens(fit.system) + afmTokens(fit.prompt) + fit.turns.reduce(0) { $0 + afmTurnTokens($1) }
    } else {
        session = LanguageModelSession(model: .default, tools: toolSet.tools, instructions: request.system)
        prompt = request.prompt
        used += afmTokens(request.system) + afmTokens(request.prompt)
    }
    // An answer that would not fit what is left of the window is cut short instead of failing
    // halfway through.
    options.maximumResponseTokens = max(200, min(request.maxTokens ?? 1024, window - used - 100))

    do {
        for try await snapshot in session.streamResponse(to: prompt, options: options) {
            if let delta = afmSuffix(of: snapshot.content, after: state.emitted) {
                state.emitted = snapshot.content
                emit(["type": "text", "delta": delta])
            }
        }
    } catch {
        // The interruption of a tool call is the answer, not a failure.
        let calls = await collector.calls
        if calls.isEmpty { throw error }
    }
    let calls = await collector.calls
    if !calls.isEmpty {
        emit(["type": "toolcall", "calls": calls.enumerated().map { (i, c) in
            ["id": "call_\(UUID().uuidString.prefix(8))_\(i)", "name": c.name, "arguments": c.arguments]
        }])
    }
    emit(["type": "done"])
}

#endif
