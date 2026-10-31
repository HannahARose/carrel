// A card read for the inspector: its front matter as labeled fields, its notes and its
// history (FORMAT.md §4). Only for showing; the core reads cards for everything else, and
// CarrelKit's card bindings (#233) replace this.

import Foundation

struct Card: Equatable {
    struct Field: Equatable, Identifiable {
        let key: String
        let value: String
        var id: String { key }
    }

    /// Top-level front matter fields in file order; nested values are joined on one line.
    let fields: [Field]
    /// The notes after the front matter, without the history section.
    let notes: String
    /// The history section's lines.
    let history: [String]

    init(text: String) {
        var lines = text.components(separatedBy: "\n")
        var frontMatter: [String] = []
        if lines.first == "---", let end = lines.dropFirst().firstIndex(of: "---") {
            frontMatter = Array(lines[1..<end])
            lines = Array(lines[(end + 1)...])
        }
        fields = Self.fields(frontMatter)

        let body = lines.joined(separator: "\n")
        if let start = body.range(of: "<!-- carrel:history -->"),
            let end = body.range(
                of: "<!-- /carrel:history -->", range: start.upperBound..<body.endIndex)
        {
            notes = String(body[..<start.lowerBound]).trimmingCharacters(
                in: .whitespacesAndNewlines)
            history = body[start.upperBound..<end.lowerBound]
                .split(separator: "\n")
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty && $0 != "## History" }
        } else {
            notes = body.trimmingCharacters(in: .whitespacesAndNewlines)
            history = []
        }
    }

    func value(_ key: String) -> String? {
        fields.first { $0.key == key }?.value
    }

    private static func fields(_ lines: [String]) -> [Field] {
        var fields: [(key: String, parts: [String])] = []
        for line in lines where !line.trimmingCharacters(in: .whitespaces).isEmpty {
            if !line.hasPrefix(" "), !line.hasPrefix("-"), let colon = line.firstIndex(of: ":") {
                let key = String(line[..<colon])
                let value = unquote(line[line.index(after: colon)...])
                fields.append((key, value.isEmpty ? [] : [value]))
            } else if !fields.isEmpty {
                fields[fields.count - 1].parts.append(unquote(Substring(line)))
            }
        }
        return fields.map { Field(key: $0.key, value: $0.parts.joined(separator: ", ")) }
    }

    private static func unquote(_ text: Substring) -> String {
        var text = text.trimmingCharacters(in: .whitespaces)
        if text.hasPrefix("- ") { text.removeFirst(2) }
        if let colon = text.range(of: ": "), !text.hasPrefix("\"") {
            // A nested `key: value`.
            let key = text[..<colon.lowerBound]
            return "\(key) \(unquote(text[colon.upperBound...]))"
        }
        if text.count >= 2, text.first == "\"", text.last == "\"" {
            text = String(text.dropFirst().dropLast())
        }
        if text.count >= 2, text.first == "[", text.last == "]" {
            text = String(text.dropFirst().dropLast())
        }
        return text
    }
}
