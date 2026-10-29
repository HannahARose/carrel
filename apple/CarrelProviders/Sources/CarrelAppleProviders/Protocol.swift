// The provider protocol (docs/providers.md): JSON-RPC 2.0, one message per line.

import Foundation

/// The protocol version these providers speak.
public let protocolVersion = 1

/// Priorities by convention (docs/providers.md).
public enum Priority {
    /// A platform's own tools.
    public static let native = 100
}

/// What an offer does.
public enum Capability: String, Codable, Sendable {
    case textModel = "text-model"
    case extractText = "extract-text"
}

/// One thing a provider offers, as reported by `initialize`.
public struct Offer: Codable, Equatable, Sendable {
    public var capability: Capability
    public var id: String
    public var label: String?
    public var priority: Int
    public var available: Bool
    public var reason: String?
    public var mediaTypes: [String]?

    public init(
        capability: Capability, id: String, label: String? = nil, priority: Int = Priority.native,
        available: Bool = true, reason: String? = nil, mediaTypes: [String]? = nil
    ) {
        self.capability = capability
        self.id = id
        self.label = label
        self.priority = priority
        self.available = available
        self.reason = reason
        self.mediaTypes = mediaTypes
    }

    enum CodingKeys: String, CodingKey {
        case capability, id, label, priority, available, reason
        case mediaTypes = "media_types"
    }
}

/// Why a request failed, with the protocol's error codes.
public enum ProviderError: Error, Equatable {
    /// The offer can't be used right now (code 1).
    case unavailable(String)
    /// The input isn't one the offer handles (code 2).
    case unsupported(String)
    /// The offer tried and couldn't (code 3).
    case failed(String)
    /// A malformed request (JSON-RPC's -32602).
    case invalidParams(String)
    /// An unknown method (JSON-RPC's -32601).
    case unknownMethod(String)

    var code: Int {
        switch self {
        case .unavailable: 1
        case .unsupported: 2
        case .failed: 3
        case .invalidParams: -32602
        case .unknownMethod: -32601
        }
    }

    var message: String {
        switch self {
        case .unavailable(let m), .unsupported(let m), .failed(let m), .invalidParams(let m),
            .unknownMethod(let m):
            m
        }
    }
}

/// The text read from a file.
public struct Extraction: Equatable, Sendable {
    public var text: String
    /// `embedded` for text stored in the file, `ocr` for recognized text.
    public var method: String
    public var pages: Int?

    public init(text: String, method: String, pages: Int? = nil) {
        self.text = text
        self.method = method
        self.pages = pages
    }
}

/// Something that reads the text of files.
public protocol TextExtractor: Sendable {
    /// What it offers. Checked when the provider starts, so availability is current.
    var offer: Offer { get }

    /// Reads the text of the file at `url`, of media type `mediaType`. Never changes the file.
    func extract(_ url: URL, mediaType: String) throws(ProviderError) -> Extraction
}

/// Answers provider protocol messages for a set of extractors.
public struct Server: Sendable {
    public let name: String
    public let version: String
    public let extractors: [any TextExtractor]

    public init(name: String, version: String, extractors: [any TextExtractor]) {
        self.name = name
        self.version = version
        self.extractors = extractors
    }

    /// The reply to one line of input, or nil when no reply is due (a notification, or a line
    /// that isn't a request at all).
    public func reply(to line: String) -> String? {
        guard let data = line.data(using: .utf8),
            let message = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let method = message["method"] as? String
        else {
            return nil
        }
        guard let id = message["id"], !(id is NSNull) else {
            return nil
        }
        let params = message["params"] as? [String: Any] ?? [:]
        var reply: [String: Any] = ["jsonrpc": "2.0", "id": id]
        do {
            reply["result"] = try answer(method, params)
        } catch {
            reply["error"] = ["code": error.code, "message": error.message]
        }
        guard let out = try? JSONSerialization.data(withJSONObject: reply, options: [.sortedKeys]),
            let text = String(data: out, encoding: .utf8)
        else {
            return nil
        }
        return text
    }

    func answer(_ method: String, _ params: [String: Any]) throws(ProviderError) -> Any {
        switch method {
        case "initialize":
            return [
                "protocol": protocolVersion,
                "name": name,
                "version": version,
                "offers": extractors.map { Self.json($0.offer) },
            ]
        case "shutdown":
            return NSNull()
        case "text/extract":
            guard let offerID = params["offer"] as? String,
                let path = params["path"] as? String,
                let mediaType = params["media_type"] as? String
            else {
                throw .invalidParams("text/extract needs offer, path and media_type")
            }
            guard let extractor = extractors.first(where: { $0.offer.id == offerID }) else {
                throw .unsupported("\(name) has no extractor \(offerID)")
            }
            guard extractor.offer.available else {
                throw .unavailable(extractor.offer.reason ?? "\(offerID) is unavailable")
            }
            let result = try extractor.extract(URL(filePath: path), mediaType: mediaType)
            var json: [String: Any] = ["text": result.text, "method": result.method]
            if let pages = result.pages {
                json["pages"] = pages
            }
            return json
        case "text/complete":
            throw .unsupported("\(name) has no text models yet")
        default:
            throw .unknownMethod("unknown method \(method)")
        }
    }

    static func json(_ offer: Offer) -> Any {
        let data = (try? JSONEncoder().encode(offer)) ?? Data("{}".utf8)
        return (try? JSONSerialization.jsonObject(with: data)) ?? [:]
    }
}
