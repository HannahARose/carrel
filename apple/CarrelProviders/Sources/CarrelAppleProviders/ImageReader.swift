// Reading images (docs/design/images.md): metadata through ImageIO, labels and text through
// Vision, and a description from Apple's on-device model, returned as one text.

import CoreGraphics
import Foundation
import ImageIO
import Vision

#if canImport(FoundationModels)
import FoundationModels
#endif

/// Reads what an image is, not just the text in it. It's listed before `VisionOCR`, so it's
/// tried first; OCR stays the fallback and reads scanned PDFs.
///
/// Coordinates never leave it: the text says only that a photo has a location, and the
/// result's `located` flag lets Carrel rate it.
public struct ImageReader: TextExtractor {
    /// The image types it reads: those Vision OCR reads, and RAW photos.
    public static let imageTypes = VisionOCR.imageTypes + [
        "image/x-adobe-dng", "image/x-canon-cr2", "image/x-canon-cr3", "image/x-nikon-nef",
        "image/x-sony-arw", "image/x-fuji-raf", "image/x-olympus-orf", "image/x-panasonic-rw2",
    ]

    /// The long side, in pixels, an image is scaled to for the description: its tokens come out
    /// of the model's context, and larger ones are slower.
    static let describeSize = 1024
    /// How many labels to keep, and how confident each must be.
    static let labelCount = 5
    static let labelConfidence: Float = 0.3

    /// Whether to ask Apple's model for a description. Off in tests that check the rest.
    let describes: Bool

    public init() { self.init(describes: true) }

    init(describes: Bool) { self.describes = describes }

    public var offer: Offer {
        Offer(
            capability: .extractText, id: "image", label: "Image reader", mediaTypes: Self.imageTypes,
            describes: Self.canSee ? true : nil)
    }

    public func extract(_ url: URL, mediaType: String) async throws(ProviderError) -> Extraction {
        try await extract(url, mediaType: mediaType, context: nil)
    }

    public func extract(_ url: URL, mediaType: String, context: String?) async throws(ProviderError)
        -> Extraction
    {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
            CGImageSourceGetCount(source) > 0
        else {
            throw .failed("\(url.lastPathComponent) isn't a readable image")
        }
        let metadata = Metadata(source)
        let labels = await Self.labels(url)
        let text: String
        do {
            text = VisionOCR.text(of: try await VisionOCR.request.perform(on: url))
        } catch {
            throw .failed("\(url.lastPathComponent): \(error.localizedDescription)")
        }
        let description = describes ? await Self.describe(source, labels: labels, context: context) : nil

        var lines = ["Image: " + metadata.summary]
        if let description {
            lines.append("Shows: " + description)
        }
        lines.append("Labels: " + (labels.isEmpty ? "(none)" : labels.joined(separator: ", ")))
        lines.append("Text in the image:" + (text.isEmpty ? " (none)" : "\n" + text))
        return Extraction(
            text: lines.joined(separator: "\n"), method: description == nil ? "ocr" : "described",
            pages: 1, created: metadata.created, located: metadata.located)
    }

    /// The most confident few of Vision's labels, with underscores as spaces. None if Vision
    /// can't classify it.
    static func labels(_ url: URL) async -> [String] {
        guard let observations = try? await ClassifyImageRequest().perform(on: url) else {
            return []
        }
        return observations
            .filter { $0.confidence >= labelConfidence }
            .sorted { $0.confidence > $1.confidence }
            .prefix(labelCount)
            .map { $0.identifier.replacingOccurrences(of: "_", with: " ") }
    }

    /// The image scaled to `describeSize` on its long side, upright.
    static func scaled(_ source: CGImageSource) -> CGImage? {
        CGImageSourceCreateThumbnailAtIndex(
            source, 0,
            [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: describeSize,
            ] as CFDictionary)
    }

    /// One or two sentences from Apple's model on what the image shows, or nil when the model
    /// can't see images (before macOS 27, or Apple Intelligence off) or declines. `context`
    /// says what a picture Carrel was given is, such as a 3D print project's thumbnail.
    static func describe(_ source: CGImageSource, labels: [String], context: String? = nil) async -> String? {
        guard canSee, let image = scaled(source) else { return nil }
        var hint = labels.isEmpty ? "" : " Vision's labels for it: \(labels.joined(separator: ", "))."
        if let context, !context.isEmpty {
            hint = " It's \(context)." + hint
        }
        return await describe(
            image,
            instructions: """
                You describe images for a personal archive's catalog. Say plainly what the \
                image shows in one or two sentences. Don't guess names of people or places.
                """,
            prompt: "Describe this image." + hint)
    }

    /// Whether Apple's model can see images here: macOS 27 or later, with Apple Intelligence on.
    static var canSee: Bool {
        #if canImport(FoundationModels) && compiler(>=6.4)
        if #available(macOS 27.0, *) {
            let model = SystemLanguageModel.default
            return model.isAvailable && model.capabilities.contains(.vision)
        }
        #endif
        return false
    }

    /// What Apple's model says of `image`, given `instructions` and `prompt`, or nil when it
    /// can't see images or declines. Shared with the 3D model reader.
    static func describe(_ image: CGImage, instructions: String, prompt: String) async -> String? {
        #if canImport(FoundationModels) && compiler(>=6.4)
        if #available(macOS 27.0, *), canSee {
            let session = LanguageModelSession(model: .default, instructions: instructions)
            do {
                let response = try await session.respond(
                    options: GenerationOptions(maximumResponseTokens: 120)
                ) {
                    prompt
                    Attachment(image)
                }
                let text = response.content.trimmingCharacters(in: .whitespacesAndNewlines)
                return text.isEmpty ? nil : text
            } catch {
                FileHandle.standardError.write(
                    Data("carrel-provider-apple: no description: \(error.localizedDescription)\n".utf8))
                return nil
            }
        }
        #endif
        return nil
    }
}

/// What an image's metadata says about it.
struct Metadata: Equatable {
    var width: Int?
    var height: Int?
    /// `screenshot`, `photo` or `image`.
    var kind = "image"
    /// When it was taken, as the card's `created`: a date, or a time when the offset is known.
    var created: String?
    /// When it was taken, as said in the text.
    var taken: String?
    /// The camera that took it.
    var camera: String?
    /// The app or system that made or last saved it.
    var software: String?
    var located = false

    init(_ source: CGImageSource) {
        self.init(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] ?? [:])
    }

    init(_ properties: [CFString: Any]) {
        width = properties[kCGImagePropertyPixelWidth] as? Int
        height = properties[kCGImagePropertyPixelHeight] as? Int
        let exif = properties[kCGImagePropertyExifDictionary] as? [CFString: Any] ?? [:]
        let tiff = properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]
        let gps = properties[kCGImagePropertyGPSDictionary] as? [CFString: Any] ?? [:]

        let make = (tiff[kCGImagePropertyTIFFMake] as? String)?.trimmingCharacters(in: .whitespaces)
        let model = (tiff[kCGImagePropertyTIFFModel] as? String)?.trimmingCharacters(in: .whitespaces)
        let software = (tiff[kCGImagePropertyTIFFSoftware] as? String)?.trimmingCharacters(in: .whitespaces)
        if (exif[kCGImagePropertyExifUserComment] as? String)?.lowercased() == "screenshot" {
            kind = "screenshot"
        } else if model != nil || exif[kCGImagePropertyExifExposureTime] != nil {
            kind = "photo"
        }
        switch (make, model) {
        // The maker's name only where the model doesn't say it: `SONY ILCE-7M3`, but `Canon EOS
        // R5` and `iPhone 15 Pro` alone.
        case let (make?, model?)
        where make != "Apple" && !model.lowercased().hasPrefix(make.lowercased().prefix(5)):
            camera = "\(make) \(model)"
        case let (_, model?):
            camera = model
        default:
            break
        }
        self.software = software.flatMap { $0.isEmpty ? nil : $0 }
        // EXIF's dates, not TIFF's `DateTime`: that's when it was last saved.
        if let original = exif[kCGImagePropertyExifDateTimeOriginal] as? String {
            (taken, created) = Self.date(original, offset: exif["OffsetTimeOriginal" as CFString] as? String)
        } else if let digitized = exif[kCGImagePropertyExifDateTimeDigitized] as? String {
            (taken, created) = Self.date(digitized, offset: exif["OffsetTimeDigitized" as CFString] as? String)
        }
        located = gps[kCGImagePropertyGPSLatitude] != nil || gps[kCGImagePropertyGPSLongitude] != nil
    }

    /// EXIF's `2026:06:24 16:28:05` as `2026-06-24 16:28` for the text, and as the card's date:
    /// `2026-06-24T16:28:05+02:00` with an offset, or `2026-06-24` without, since the time
    /// alone doesn't say which day it was elsewhere.
    static func date(_ exif: String, offset: String?) -> (String?, String?) {
        let parts = exif.split(whereSeparator: { $0 == " " || $0 == ":" }).map(String.init)
        guard parts.count >= 5, parts[0].count == 4, parts[0] != "0000",
            parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy(\.isNumber) })
        else { return (nil, nil) }
        let day = "\(parts[0])-\(parts[1])-\(parts[2])"
        let taken = "\(day) \(parts[3]):\(parts[4])"
        if let offset, offset.count == 6, offset.first == "+" || offset.first == "-" {
            let seconds = parts.count > 5 ? parts[5] : "00"
            return (taken, "\(day)T\(parts[3]):\(parts[4]):\(seconds)\(offset)")
        }
        return (taken, day)
    }

    /// `photo, 4032 × 3024, taken 2026-06-24 16:28 on an iPhone 15 Pro, with a location`.
    var summary: String {
        var parts = [kind]
        if let width, let height {
            parts.append("\(width) × \(height)")
        }
        var when = ""
        if let taken {
            when = "taken \(taken)"
        }
        if let camera {
            let article = "aeiouAEIOU".contains(camera.first ?? "x") ? "an" : "a"
            when += (when.isEmpty ? "taken" : "") + " on \(article) \(camera)"
        } else if let software {
            when += (when.isEmpty ? "made" : "") + " with \(software)"
        }
        if !when.isEmpty {
            parts.append(when)
        }
        if located {
            parts.append("with a location")
        }
        return parts.joined(separator: ", ")
    }
}
