// What the Quick Look extension reads (docs/design/macos-integration.md, "Quick Look"). The
// extension is sandboxed and can't open the archive, so the agent keeps what it needs in
// ~/Library/Application Support/Carrel/QuickLook, which the extension's entitlements let it
// read and nothing more: Carrel's drawings of STEP, IGES and 3MF items, and the roots of the
// archives followed. An app group would need a provisioning profile, and Carrel is signed
// ad hoc.

import Foundation

public struct QuickLookStore: Sendable, Equatable {
    /// The folder.
    public let folder: URL

    public init(folder: URL) {
        self.folder = folder
    }

    /// The store in this person's Application Support folder. It's the real one even in the
    /// sandboxed extension, whose own home is its container.
    public static var shared: QuickLookStore {
        let home = getpwuid(getuid()).flatMap { $0.pointee.pw_dir.map { String(cString: $0) } }
        let homeURL =
            home.map { URL(filePath: $0) } ?? URL(filePath: NSHomeDirectory())
        return QuickLookStore(
            folder: homeURL.appending(path: "Library/Application Support/Carrel/QuickLook"))
    }

    /// The file extensions of the formats Carrel draws: STEP, IGES and 3MF.
    public static let drawnExtensions: Set<String> = ["step", "stp", "iges", "igs", "3mf"]

    /// Whether Carrel draws the file at `path`.
    public static func isDrawn(_ path: String) -> Bool {
        drawnExtensions.contains(URL(filePath: path).pathExtension.lowercased())
    }

    /// Carrel's drawings of STEP, IGES and 3MF items, named by their files.
    public var drawings: URL { folder.appending(path: "drawings") }

    /// The roots of the archives the agent follows, one path a line, to name a card's
    /// collection by.
    public var archives: URL { folder.appending(path: "archives.txt") }

    /// The archive roots last written.
    public func roots() -> [String] {
        ((try? String(contentsOf: archives, encoding: .utf8)) ?? "")
            .split(separator: "\n").map(String.init)
    }

    /// Writes the archive roots, when they've changed.
    public func write(roots: [String]) throws {
        guard roots != self.roots() else { return }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try roots.joined(separator: "\n").write(to: archives, atomically: true, encoding: .utf8)
    }
}
