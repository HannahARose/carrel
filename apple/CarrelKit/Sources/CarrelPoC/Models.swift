// The two TextModel implementations the proof of concept offers Rust: Apple's on-device
// model when this device has it, and a plain offline stand-in otherwise.

import CarrelKit
import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

/// The best model this device offers.
func availableModel() -> any TextModel {
    #if canImport(FoundationModels)
    if #available(macOS 26.0, iOS 26.0, *), SystemLanguageModel.default.availability == .available {
        return OnDeviceModel()
    }
    #endif
    return OfflineModel()
}

#if canImport(FoundationModels)
/// Apple's Foundation Models, on device.
///
/// Rust calls `complete` synchronously, so it waits for the asynchronous session on a
/// semaphore. Callers must not be on the main thread. Async callback interfaces would remove
/// the wait and are the way to go for real use.
@available(macOS 26.0, iOS 26.0, *)
final class OnDeviceModel: TextModel {
    func complete(prompt: String) throws -> String {
        let result = Box()
        let done = DispatchSemaphore(value: 0)
        Task.detached {
            do {
                result.value = .success(try await LanguageModelSession().respond(to: prompt).content)
            } catch {
                result.value = .failure(error)
            }
            done.signal()
        }
        done.wait()
        switch result.value {
        case .success(let text): return text
        case .failure(let error): throw CarrelError.Callback(message: error.localizedDescription)
        case nil: throw CarrelError.Callback(message: "the model gave no answer")
        }
    }

    private final class Box: @unchecked Sendable {
        var value: Result<String, any Error>?
    }
}
#endif

/// Answers without a model by counting what the prompt lists, so the app works everywhere.
final class OfflineModel: TextModel {
    func complete(prompt: String) throws -> String {
        let lines = prompt.split(separator: "\n")
        let collections = lines.drop { $0 != "Collections:" }.dropFirst().prefix { $0.hasPrefix("- ") }
        let items = lines.drop { $0 != "Items:" }.dropFirst().prefix { $0.hasPrefix("- ") }
        return "An archive of \(items.count) items in \(collections.count) collections. "
            + "(No on-device model is available, so this summary only counts.)"
    }
}
