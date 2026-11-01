// A card as the Quick Look extension shows it (docs/design/macos-integration.md, "Quick
// Look"): an HTML page laid out as the app's inspector is, its kind and title, summary,
// collection and tags, fields, notes and history. A restricted card shows its title only.

import Foundation

public enum QuickLookCardPage {
    /// The page for `card`.
    public static func html(_ card: QuickLookCard) -> String {
        var body = ""
        if card.restricted {
            body += "<h1>\(escape(card.title))</h1>\n"
            body += "<p class=\"restricted\">\(escape(card.restrictedNote))</p>\n"
            return page(title: card.title, body: body)
        }
        body += "<p class=\"kind\">\(escape(card.kind.capitalized))</p>\n"
        body += "<h1>\(escape(card.title))</h1>\n"
        if let summary = card.summary {
            body += "<p class=\"summary\">\(escape(summary))</p>\n"
        }
        if !card.collection.isEmpty {
            body += "<p class=\"collection\">\(escape(card.collection))</p>\n"
        }
        if !card.tags.isEmpty {
            let tags = card.tags.map { "<span class=\"tag\">\(escape($0))</span>" }
            body += "<p class=\"tags\">\(tags.joined(separator: " "))</p>\n"
        }
        if !card.fields.isEmpty {
            body += "<h2>Fields</h2>\n<table>\n"
            for field in card.fields {
                body += "<tr><th>\(escape(field.label))</th><td>\(escape(field.value))</td></tr>\n"
            }
            body += "</table>\n"
        }
        if !card.notes.isEmpty {
            body += "<h2>Notes</h2>\n<div class=\"notes\">\(escape(card.notes))</div>\n"
        }
        if !card.history.isEmpty {
            body += "<h2>History</h2>\n"
            for entry in card.history {
                let pending = entry.pending ? " pending" : ""
                body += "<div class=\"entry\(pending)\"><h3>\(escape(entry.heading))</h3>"
                body += "<div>\(escape(entry.text))</div></div>\n"
            }
        }
        return page(title: card.title, body: body)
    }

    /// A Markdown file that isn't a card, as its text.
    public static func plain(_ text: String, title: String) -> String {
        page(title: title, body: "<div class=\"notes\">\(escape(text))</div>\n")
    }

    /// `text` with HTML's special characters escaped.
    static func escape(_ text: String) -> String {
        var escaped = ""
        escaped.reserveCapacity(text.count)
        for character in text {
            switch character {
            case "&": escaped += "&amp;"
            case "<": escaped += "&lt;"
            case ">": escaped += "&gt;"
            case "\"": escaped += "&quot;"
            case "'": escaped += "&#39;"
            default: escaped.append(character)
            }
        }
        return escaped
    }

    private static func page(title: String, body: String) -> String {
        """
        <!doctype html>
        <html><head><meta charset="utf-8"><title>\(escape(title))</title>
        <style>
        :root { color-scheme: light dark; }
        body { font: 13px -apple-system, sans-serif; margin: 24px; line-height: 1.45; }
        h1 { font-size: 22px; margin: 2px 0 8px; }
        h2 { font-size: 13px; margin: 20px 0 6px; color: GrayText; }
        h3 { font-size: 13px; margin: 0 0 2px; }
        .kind, .collection { color: GrayText; margin: 0; }
        .summary { font-size: 14px; }
        .tag { display: inline-block; padding: 1px 8px; border-radius: 9px;
               background: color-mix(in srgb, GrayText 18%, transparent); }
        table { border-collapse: collapse; }
        th { text-align: right; font-weight: normal; color: GrayText; padding: 2px 12px 2px 0;
             vertical-align: top; white-space: nowrap; }
        td { padding: 2px 0; }
        .notes, .entry div { white-space: pre-wrap; }
        .entry { margin-bottom: 10px; }
        .entry.pending { border-left: 3px solid #e3a008; padding-left: 8px; }
        .restricted { color: GrayText; }
        </style></head>
        <body>
        \(body)</body></html>
        """
    }
}
