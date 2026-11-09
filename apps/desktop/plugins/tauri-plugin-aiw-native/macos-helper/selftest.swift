// `aiw-apple-fm selftest`: checks of AppleFMCore that do not need the model. Run by the Rust
// tests of the plugin (macOS, Apple Silicon) and by hand.

import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

func runSelfTest() {
    var checks = 0
    var failures: [String] = []
    func check(_ ok: Bool, _ what: String) {
        checks += 1
        if !ok { failures.append(what) }
    }
    func decode<T: Decodable>(_ type: T.Type, _ json: String) -> T { try! JSONDecoder().decode(type, from: Data(json.utf8)) }

    // Names.
    check(afmSanitizeName("fs.read-file") == "fs_read_file", "sanitize punctuation")
    check(afmSanitizeName("9lives").hasPrefix("t_"), "sanitize leading digit")

    // Shapes: free-form JSON is written as a string and comes back as JSON.
    let shape: AFMShape = .object(["surface": .jsonString, "items": .array(.jsonString), "n": .plain])
    let restored = afmRestore(["surface": "{\"a\": [1, 2]}", "items": ["{\"x\": true}", "plain text"], "n": 3] as [String: Any], shape) as! [String: Any]
    check((restored["surface"] as? [String: Any])?["a"] as? [Int] == [1, 2], "restore object")
    check(((restored["items"] as? [Any])?.first as? [String: Any])?["x"] as? Bool == true, "restore array element")
    check((restored["items"] as? [Any])?.last as? String == "plain text", "restore keeps non-JSON text")
    let back = afmStringify(restored, shape) as! [String: Any]
    check(back["surface"] is String && back["n"] as? Int == 3, "stringify is the inverse")

    // Fitting: the oldest exchanges go, the latest stays, calls stay with their results.
    var turns: [AFMTurn] = []
    for i in 0..<30 {
        turns.append(AFMTurn(role: "user", text: "domanda \(i) " + String(repeating: "x", count: 400), toolCalls: nil, id: nil, name: nil))
        turns.append(AFMTurn(role: "assistant", text: nil, toolCalls: [AFMCall(id: "c\(i)", name: "list_dir", arguments: "{\"path\": \".\"}")], id: nil, name: nil))
        turns.append(AFMTurn(role: "tool", text: String(repeating: "riga\n", count: 800), toolCalls: nil, id: "c\(i)", name: "list_dir"))
        turns.append(AFMTurn(role: "assistant", text: "risposta \(i)", toolCalls: nil, id: nil, name: nil))
    }
    let fit = afmFit(system: "Sei utile.", turns: turns, prompt: "ultima domanda", budget: 1200)
    let used = afmTokens(fit.system) + afmTokens(fit.prompt) + fit.turns.reduce(0) { $0 + afmTokens($1.text ?? "") + 16 }
    check(used <= 1200, "fit stays inside the budget (\(used))")
    check(fit.dropped > 0 && fit.turns.first?.role == "user", "fit drops whole exchanges")
    check(fit.turns.last?.text == "risposta 29", "fit keeps the latest exchange")
    let ids = Set(fit.turns.compactMap { $0.toolCalls?.first?.id })
    let outs = Set(fit.turns.filter { $0.role == "tool" }.compactMap { $0.id })
    check(ids == outs, "fit never separates a call from its result")
    check(afmFit(system: "s", turns: [], prompt: String(repeating: "p", count: 60000), budget: 800).prompt.count < 3000, "fit clips a huge prompt")

    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        let specs = decode([AFMToolSpec].self, """
        [{"name":"fs.read_file","description":"Legge un file.\\nSeconda riga","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Percorso"},"max":{"type":"integer"}},"required":["path"]}},
         {"name":"render_ui","description":"Mostra una UI","parameters":{"type":"object","properties":{"surfaceId":{"type":"string"},"components":{"type":"array","items":{"type":"object"}}},"required":["surfaceId","components"]}},
         {"name":"pick","description":"Sceglie","parameters":{"type":"object","properties":{"mode":{"type":"string","enum":["a","b"]},"opt":{"anyOf":[{"type":"string"},{"type":"null"}]},"nested":{"type":"object","properties":{"deep":{"type":"object","properties":{"deeper":{"type":"object","properties":{"deepest":{"type":"object","properties":{"x":{"type":"string"}}}}}}}}}},"required":["mode"]}},
         {"name":"todo","description":"Piano","parameters":{"type":"object","properties":{"todos":{"type":"array","items":{"type":"object","properties":{"content":{"type":"string"},"status":{"type":"string","enum":["pending","done"]}},"required":["content","status"]}}},"required":["todos"]}},
         {"name":"noargs","description":"Niente argomenti","parameters":{"type":"object","properties":{}}},
         {"name":"freeform","description":"Oggetto libero","parameters":{"type":"object","additionalProperties":true}},
         {"name":"fs.read_file","description":"Duplicato","parameters":null}]
        """)
        let set = AFMToolSet(specs, collector: AFMCollector())
        check(set.tools.count == 7 && set.skipped.isEmpty, "every tool gets a schema (\(set.tools.count) tools, skipped \(set.skipped))")
        check(Set(set.tools.map { $0.name }).count == set.tools.count, "model-side names are unique")
        check(set.tool(named: "fs.read_file")?.name == "fs_read_file", "original name maps to the sanitised one")
        if let ui = set.tool(named: "render_ui"), case .object(let props) = ui.shape, case .array(.jsonString)? = props["components"] {
            check(true, "free-form array items are JSON strings")
        } else { check(false, "free-form array items are JSON strings") }
        if let f = set.tool(named: "freeform"), case .wrapper = f.shape { check(true, "free-form object is wrapped") } else { check(false, "free-form object is wrapped") }
        check(set.estimatedTokens > 0, "tool tokens estimated")

        // Transcript: instructions, a user turn, a call with its result, an answer, an unanswered call.
        let history = decode([AFMTurn].self, """
        [{"role":"user","text":"Cosa c'è qui?"},
         {"role":"assistant","text":"","toolCalls":[{"id":"c1","name":"fs.read_file","arguments":"{\\"path\\": \\"a.md\\"}"}]},
         {"role":"tool","id":"c1","name":"fs.read_file","text":"contenuto"},
         {"role":"assistant","text":"C'è un file."},
         {"role":"user","text":"E poi?"},
         {"role":"assistant","text":"","toolCalls":[{"id":"c2","name":"fs.read_file","arguments":"{}"}]}]
        """)
        let tr = afmBuildTranscript(system: "Sei utile.", turns: history, tools: set)
        // instructions, prompt, toolCalls, toolOutput, response, prompt  (c2 has no result: left out)
        check(tr.count == 6, "transcript has the expected entries (\(tr.count))")
        if tr.count == 6 {
            if case .instructions(let i) = tr[0] { check(i.toolDefinitions.count == 7, "instructions carry the tool definitions") } else { check(false, "first entry is instructions") }
            if case .toolCalls(let c) = tr[2] { check(c.first?.toolName == "fs_read_file", "history call uses the model-side name") } else { check(false, "third entry is a tool call") }
            if case .toolOutput = tr[3] { check(true, "result follows its call") } else { check(false, "fourth entry is the tool output") }
        }
    }
    #endif

    var out: [String: Any] = ["ok": failures.isEmpty, "checks": checks]
    if !failures.isEmpty { out["failures"] = failures }
    emit(out)
    exit(failures.isEmpty ? 0 : 1)
}
