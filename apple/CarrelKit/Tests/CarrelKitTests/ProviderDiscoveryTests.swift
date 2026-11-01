// Settings asks what the providers offer several times each time it's shown. The providers are
// started once to find out, then remembered, so showing Settings again doesn't start every
// provider program again or wait for them (#333).

import CarrelKit
import Foundation
import Testing

/// A provider program, as a shell script, that takes half a second to start, as real ones
/// can, and counts its starts in `starts` beside it.
private func slowProvider(in folder: URL) throws -> URL {
    let script = folder.appending(path: "carrel-provider-slowstart")
    let starts = folder.appending(path: "starts").path(percentEncoded: false)
    let text = #"""
        #!/bin/sh
        echo started >> '\#(starts)'
        sleep 0.5
        while IFS= read -r line; do
          id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
          case "$line" in
            *'"initialize"'*)
              printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol":1,"name":"Slow start","offers":[{"capability":"text-model","id":"slow","priority":1,"available":true}]}}\n' "$id" ;;
            *)
              printf '{"jsonrpc":"2.0","id":%s,"result":null}\n' "$id" ;;
          esac
        done
        """#
    try text.write(to: script, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    return folder.appending(path: "starts")
}

@MainActor
@Suite struct ProviderDiscovery {
    @Test func showingSettingsAgainStartsNoProviders() async throws {
        let folder = FileManager.default.temporaryDirectory
            .appending(path: "carrelkit-\(UUID().uuidString)")
        let helpers = folder.appending(path: "helpers")
        try FileManager.default.createDirectory(at: helpers, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: sampleArchive, to: folder.appending(path: "archive"))
        let starts = try slowProvider(in: helpers)
        let handle = try ArchiveHandle.open(
            folder.appending(path: "archive"),
            options: OpenOptions(
                configDir: folder.appending(path: "config").path(percentEncoded: false),
                providerFolders: [helpers.path(percentEncoded: false)],
                startProviders: true,
                home: folder.appending(path: "home").path(percentEncoded: false)
            )
        )
        let model = ArchiveModel(handle: handle)

        // What Settings asks for each time it's shown (`SettingsModel.refreshDevice`).
        func showSettings() async throws -> Duration {
            let clock = ContinuousClock()
            return try await clock.measure {
                let choices = try await model.call { try $0.modelChoices() }
                #expect(choices.available.contains("slowstart/slow"))
                _ = try await model.call { $0.providers() }
                _ = try await model.call { $0.formats() }
                _ = try await model.call { $0.embeddingModels() }
                try await model.refreshStatus()
            }
        }

        let first = try await showSettings()
        var again: [Duration] = []
        for _ in 0..<5 {
            again.append(try await showSettings())
        }
        let slowest = again.max() ?? .zero
        // The timings are printed, not checked: a busy CI runner can take longer than a provider
        // start without starting one. That the provider started once is what matters.
        print("Settings shown first in \(first), then at worst in \(slowest)")
        let started = try String(contentsOf: starts, encoding: .utf8)
            .split(separator: "\n").count
        #expect(started == 1, "the provider was started \(started) times")
    }
}
