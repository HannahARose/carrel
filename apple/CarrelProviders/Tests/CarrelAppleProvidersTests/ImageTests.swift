@testable import CarrelAppleProviders
import CoreGraphics
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers

/// `image` written as `type` with `properties`, as a camera or the screenshot tool would.
func write(_ image: CGImage, as type: UTType, properties: [CFString: Any]) throws -> URL {
    let url = temporary(type.preferredFilenameExtension ?? "img")
    let destination = try #require(
        CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil))
    CGImageDestinationAddImage(destination, image, properties as CFDictionary)
    #expect(CGImageDestinationFinalize(destination))
    return url
}

/// A photo as a phone writes one: when, on what, and where.
var phonePhoto: [CFString: Any] { [
    kCGImagePropertyExifDictionary: [
        kCGImagePropertyExifDateTimeOriginal: "2026:06:24 16:28:05",
        "OffsetTimeOriginal" as CFString: "+02:00",
        kCGImagePropertyExifExposureTime: 0.01,
    ] as [CFString: Any],
    kCGImagePropertyTIFFDictionary: [
        kCGImagePropertyTIFFMake: "Apple", kCGImagePropertyTIFFModel: "iPhone 15 Pro",
    ] as [CFString: Any],
    kCGImagePropertyGPSDictionary: [
        kCGImagePropertyGPSLatitude: 46.2, kCGImagePropertyGPSLatitudeRef: "N",
        kCGImagePropertyGPSLongitude: 6.1, kCGImagePropertyGPSLongitudeRef: "E",
    ] as [CFString: Any],
] }

@Test func photosSayWhenAndOnWhatButNotWhere() async throws {
    let url = try write(picture(of: ["Hello"]), as: .jpeg, properties: phonePhoto)
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ImageReader(describes: false).extract(url, mediaType: "image/jpeg")
    let first = try #require(result.text.split(separator: "\n").first)
    #expect(first == "Image: photo, 1200 × 200, taken 2026-06-24 16:28 on an iPhone 15 Pro, with a location")
    #expect(result.created == "2026-06-24T16:28:05+02:00")
    #expect(result.located == true)
    #expect(result.method == "ocr")
    #expect(!result.text.contains("46.2") && !result.text.contains("6.1"))
    #expect(normalized(result.text).contains("hello"))
}

@Test func screenshotsAreNamedAsSuch() async throws {
    let url = try write(
        picture(of: ["Quarterly budget"]), as: .png,
        properties: [
            kCGImagePropertyExifDictionary: [
                kCGImagePropertyExifUserComment: "Screenshot",
                kCGImagePropertyExifDateTimeOriginal: "2026:03:01 09:15:00",
            ] as [CFString: Any]
        ])
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ImageReader(describes: false).extract(url, mediaType: "image/png")
    #expect(result.text.hasPrefix("Image: screenshot, 1200 × 200, taken 2026-03-01 09:15\n"))
    // No offset, so only the day.
    #expect(result.created == "2026-03-01")
    #expect(result.located == false)
    #expect(result.text.contains("Text in the image:\nQuarterly budget"))
}

@Test func plainImagesHaveOnlyTheirSize() async throws {
    let url = try writePNG(picture(of: [""]))
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ImageReader(describes: false).extract(url, mediaType: "image/png")
    #expect(result.text.hasPrefix("Image: image, 1200 × 200\n"))
    #expect(result.text.hasSuffix("Text in the image: (none)"))
    #expect(result.created == nil)
}

@Test func exifDatesAreCheckedBeforeUse() {
    #expect(Metadata.date("0000:00:00 00:00:00", offset: nil) == (nil, nil))
    #expect(Metadata.date("not a date", offset: "+01:00") == (nil, nil))
    #expect(Metadata.date("2025:12:31 23:59:58", offset: "-05:00")
        == ("2025-12-31 23:59", "2025-12-31T23:59:58-05:00"))
}

@Test func unreadableImagesFail() async {
    await #expect(throws: ProviderError.self) {
        try await ImageReader(describes: false).extract(
            URL(filePath: "/nowhere/missing.png"), mediaType: "image/png")
    }
}

@Test func imagesAreScaledForTheDescription() throws {
    let url = try writePNG(picture(of: ["a", "b", "c", "d", "e", "f", "g"]))
    defer { try? FileManager.default.removeItem(at: url) }
    let source = try #require(CGImageSourceCreateWithURL(url as CFURL, nil))
    let image = try #require(ImageReader.scaled(source))
    #expect(max(image.width, image.height) == ImageReader.describeSize)
}

/// Only where Apple's model can see images: a description of a picture of words.
@Test func descriptionsComeFromAppleModelWhenItCanSee() async throws {
    let url = try writePNG(picture(of: ["Shopping list", "Milk", "Eggs"]))
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ImageReader().extract(url, mediaType: "image/png")
    if result.method == "described" {
        let shows = try #require(result.text.split(separator: "\n").first { $0.hasPrefix("Shows: ") })
        #expect(shows.count > "Shows: ".count + 10)
    } else {
        #expect(!result.text.contains("Shows:"))
    }
}
