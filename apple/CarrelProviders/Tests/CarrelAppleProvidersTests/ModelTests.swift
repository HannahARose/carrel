@testable import CarrelAppleProviders
import Foundation
import ModelIO
import Testing

/// The corners and triangles of a box `x` by `y` by `z`, moved by `offset`.
func box(_ x: Float, _ y: Float, _ z: Float, offset: SIMD3<Float> = .zero)
    -> (corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])
{
    let corners = (0..<8).map { i in
        SIMD3<Float>(i & 1 == 0 ? 0 : x, i & 2 == 0 ? 0 : y, i & 4 == 0 ? 0 : z) + offset
    }
    // Two triangles for each face, wound outwards.
    let triangles = [
        (0, 2, 1), (1, 2, 3), (4, 5, 6), (5, 7, 6),  // bottom, top
        (0, 1, 4), (1, 5, 4), (2, 6, 3), (3, 6, 7),  // front, back
        (0, 4, 2), (2, 4, 6), (1, 3, 5), (3, 7, 5),  // left, right
    ]
    return (corners, triangles)
}

/// Two shapes as one list of corners and triangles.
func combined(_ shapes: [(corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])])
    -> (corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])
{
    var corners: [SIMD3<Float>] = []
    var triangles: [(Int, Int, Int)] = []
    for shape in shapes {
        let base = corners.count
        corners += shape.corners
        triangles += shape.triangles.map { ($0.0 + base, $0.1 + base, $0.2 + base) }
    }
    return (corners, triangles)
}

func number(_ value: Float) -> String { String(format: "%g", value) }

/// An ASCII STL, with every triangle's corners written out, as STL does.
func stl(_ shape: (corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])) -> String {
    var text = "solid test\n"
    for (a, b, c) in shape.triangles {
        text += "facet normal 0 0 0\nouter loop\n"
        for corner in [a, b, c].map({ shape.corners[$0] }) {
            text += "vertex \(number(corner.x)) \(number(corner.y)) \(number(corner.z))\n"
        }
        text += "endloop\nendfacet\n"
    }
    return text + "endsolid test\n"
}

func obj(_ shape: (corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])) -> String {
    shape.corners.map { "v \(number($0.x)) \(number($0.y)) \(number($0.z))\n" }.joined()
        + shape.triangles.map { "f \($0.0 + 1) \($0.1 + 1) \($0.2 + 1)\n" }.joined()
}

func ply(_ shape: (corners: [SIMD3<Float>], triangles: [(Int, Int, Int)])) -> String {
    """
    ply
    format ascii 1.0
    element vertex \(shape.corners.count)
    property float x
    property float y
    property float z
    element face \(shape.triangles.count)
    property list uchar int vertex_indices
    end_header

    """
        + shape.corners.map { "\(number($0.x)) \(number($0.y)) \(number($0.z))\n" }.joined()
        + shape.triangles.map { "3 \($0.0) \($0.1) \($0.2)\n" }.joined()
}

func writeModel(_ text: String, _ ext: String) throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: "carrel-model-\(UUID().uuidString).\(ext)")
    try text.write(to: url, atomically: true, encoding: .utf8)
    return url
}

@Test(arguments: [("stl", "model/stl", "STL"), ("obj", "model/obj", "OBJ"), ("ply", "model/ply", "PLY")])
func geometryIsReadFromEachFormat(ext: String, mediaType: String, format: String) async throws {
    let shape = box(86, 86, 19)
    let text = switch ext {
    case "stl": stl(shape)
    case "obj": obj(shape)
    default: ply(shape)
    }
    let url = try writeModel(text, ext)
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ModelReader(describes: false).extract(url, mediaType: mediaType)
    #expect(result.text == "3D model: \(format), 86 × 86 × 19 (mm, as stored), 12 triangles, 1 part")
    #expect(result.method == "geometry")
}

@Test func separatePiecesAreCountedAsParts() async throws {
    let shape = combined([box(10, 10, 10), box(5, 5, 25.5, offset: [20, 0, 0])])
    let url = try writeModel(stl(shape), "stl")
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ModelReader(describes: false).extract(url, mediaType: "model/stl")
    #expect(result.text == "3D model: STL, 25 × 10 × 25.5 (mm, as stored), 24 triangles, 2 parts")
}

@Test func sizesKeepThreeFiguresAndCountsAreGrouped() {
    #expect(Geometry.number(86) == "86")
    #expect(Geometry.number(16.93) == "16.9")
    #expect(Geometry.number(0.0912) == "0.0912")
    #expect(Geometry.number(1234.4) == "1234")
    #expect(Geometry(extent: [1, 2, 3], triangles: 10220, parts: 1).summary
        == "1 × 2 × 3 (mm, as stored), 10,220 triangles, 1 part")
}

@Test func filesWithoutTrianglesFail() async throws {
    let url = try writeModel("solid empty\nendsolid empty\n", "stl")
    defer { try? FileManager.default.removeItem(at: url) }
    await #expect(throws: ProviderError.self) {
        try await ModelReader(describes: false).extract(url, mediaType: "model/stl")
    }
    await #expect(throws: ProviderError.self) {
        try await ModelReader(describes: false).extract(
            URL(filePath: "/nowhere/missing.stl"), mediaType: "model/stl")
    }
}

@Test func theFourViewsAreOnePictureOfGreyOnWhite() throws {
    let url = try writeModel(stl(box(86, 86, 19)), "stl")
    defer { try? FileManager.default.removeItem(at: url) }
    let views = try #require(ModelReader.views(of: MDLAsset(url: url)))
    #expect(views.width == ModelReader.tile * 2 && views.height == ModelReader.tile * 2)
    // The corners are background; each view's middle is the model.
    let pixels = try #require(views.dataProvider?.data as Data?)
    let row = views.bytesPerRow
    func grey(_ x: Int, _ y: Int) -> UInt8 { pixels[y * row + x * views.bitsPerPixel / 8] }
    #expect(grey(2, 2) == 255)
    let tile = ModelReader.tile
    for (x, y) in [(tile / 2, tile / 2), (tile * 3 / 2, tile / 2), (tile / 2, tile * 3 / 2), (tile * 3 / 2, tile * 3 / 2)] {
        #expect(grey(x, y) < 250, "view at \(x), \(y)")
    }
}

/// Only where Apple's model can see images: a description of a box.
@Test func modelsAreDescribedWhenAppleModelCanSee() async throws {
    let url = try writeModel(stl(box(86, 86, 19)), "stl")
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await ModelReader().extract(url, mediaType: "model/stl")
    if result.method == "rendered" {
        let shows = try #require(result.text.split(separator: "\n").last)
        #expect(shows.hasPrefix("Shows: ") && shows.count > "Shows: ".count + 10)
        #expect(!result.text.contains("carrel-model"), "no file name")
    } else {
        #expect(!result.text.contains("Shows:"))
    }
}
