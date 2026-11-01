// The gap log (#455): what Carrel couldn't do on this device, with no titles, text, paths or
// file names, an issue draft for each to open in the browser, where the owner reads, edits
// and submits it (Carrel never sends it), and missing features the owner reports themself.

extension ArchiveModel {
    /// This device's gaps, most items first.
    public func gaps() async throws -> [GapRecord] {
        try await call { try $0.gaps() }
    }

    /// The issue draft for gap `number`, with the owner's `added` words if any.
    public func gapIssueDraft(_ number: UInt32, added: String?) async throws -> GapIssueDraft {
        try await call { try $0.gapIssueDraft(number: number, added: added) }
    }

    /// Records that gap `number` was reported at `address`, or clears it when empty.
    @discardableResult
    public func markGapReported(_ number: UInt32, at address: String) async throws -> GapRecord {
        try await call { try $0.markGapReported(number: number, address: address) }
    }

    /// Records a missing feature in the owner's own `words`, about the item `id` if given:
    /// its type is recorded, never its name.
    @discardableResult
    public func reportMissingFeature(about id: String?, words: String) async throws -> GapRecord {
        try await call { try $0.reportMissingFeature(id: id, words: words) }
    }
}
