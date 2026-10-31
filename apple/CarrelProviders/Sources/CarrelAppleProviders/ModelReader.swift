// Reading 3D models (docs/design/models-3d.md): geometry through ModelIO, four views drawn
// off-screen with SceneKit, and a description of them from Apple's on-device model, returned
// as one text.

import AppKit
import CoreGraphics
import Foundation
import Metal
import ModelIO
import SceneKit
import SceneKit.ModelIO

/// Reads what a 3D model is: its size, triangles and parts, and what four views of it show.
///
/// The description isn't given the file's name: with it, Apple's model repeats the name
/// instead of looking. The card step sees both.
public struct ModelReader: TextExtractor {
    /// The model types ModelIO reads.
    public static let modelTypes = [
        "model/stl", "model/obj", "model/ply", "model/vnd.usdz+zip", "model/vnd.usd",
    ]

    /// Each view's side, in pixels; the four make one picture twice as wide and high.
    static let tile = 512

    /// Whether to draw the views and ask Apple's model about them. Off in tests that check the
    /// geometry.
    let describes: Bool

    public init() { self.init(describes: true) }

    init(describes: Bool) { self.describes = describes }

    public var offer: Offer {
        Offer(capability: .extractText, id: "model3d", label: "3D model reader", mediaTypes: Self.modelTypes)
    }

    public func extract(_ url: URL, mediaType: String) async throws(ProviderError) -> Extraction {
        let format = Self.format(url, mediaType: mediaType)
        guard FileManager.default.isReadableFile(atPath: url.path(percentEncoded: false)),
            MDLAsset.canImportFileExtension(url.pathExtension.lowercased())
        else {
            throw .failed("\(url.lastPathComponent) isn't a readable \(format) file")
        }
        let asset = MDLAsset(url: url)
        let geometry = Geometry(asset)
        guard geometry.triangles > 0 else {
            throw .failed("\(url.lastPathComponent) has no triangles")
        }
        var description: String?
        if describes, ImageReader.canSee, let views = Self.views(of: asset) {
            description = await ImageReader.describe(
                views,
                instructions: """
                    You describe 3D-printable models for a personal archive's catalog. Say \
                    plainly what the object is and what it's likely for, in one or two \
                    sentences. Describe only what you can see; never mention the views, the \
                    catalog or the archive.
                    """,
                prompt: "These are four views of a 3D model: angled (top left), front (top "
                    + "right), top (bottom left) and side (bottom right). It measures "
                    + "\(geometry.size), probably in millimetres. Describe the object.")
        }
        var lines = ["3D model: \(format), " + geometry.summary]
        if let description {
            lines.append("Shows: " + description)
        }
        return Extraction(
            text: lines.joined(separator: "\n"), method: description == nil ? "geometry" : "rendered")
    }

    /// The format's name for the text: `STL`, `OBJ`, `PLY`, `USDZ` or `USD`.
    static func format(_ url: URL, mediaType: String) -> String {
        switch mediaType {
        case "model/stl": "STL"
        case "model/obj": "OBJ"
        case "model/ply": "PLY"
        case "model/vnd.usdz+zip": "USDZ"
        case "model/vnd.usd": "USD"
        default: url.pathExtension.uppercased()
        }
    }

    /// The model drawn from four sides, angled, front, top and side, in light grey on white,
    /// as one 2 × 2 picture. Z is up, as in most STL files and slicers. Nil when there's no
    /// GPU to draw with.
    static func views(of asset: MDLAsset) -> CGImage? {
        guard let device = MTLCreateSystemDefaultDevice() else { return nil }
        // Normals for shading, worked out from the triangles: many files have none, or zeros.
        for case let mesh as MDLMesh in asset.childObjects(of: MDLMesh.self) {
            mesh.addNormals(withAttributeNamed: MDLVertexAttributeNormal, creaseThreshold: 0.5)
        }
        let scene = SCNScene(mdlAsset: asset)
        let grey = SCNMaterial()
        grey.diffuse.contents = NSColor(white: 0.75, alpha: 1)
        grey.lightingModel = .physicallyBased
        scene.rootNode.enumerateHierarchy { node, _ in
            node.geometry?.materials = [grey]
        }
        let (low, high) = scene.rootNode.boundingBox
        let center = SCNVector3((low.x + high.x) / 2, (low.y + high.y) / 2, (low.z + high.z) / 2)
        let radius = max(high.x - low.x, high.y - low.y, high.z - low.z)
        guard radius > 0 else { return nil }
        scene.background.contents = NSColor.white
        let ambient = SCNNode()
        ambient.light = SCNLight()
        ambient.light?.type = .ambient
        ambient.light?.intensity = 400
        scene.rootNode.addChildNode(ambient)

        let camera = SCNNode()
        camera.camera = SCNCamera()
        camera.camera?.fieldOfView = 30
        camera.camera?.zFar = Double(radius * 20)
        scene.rootNode.addChildNode(camera)
        let renderer = SCNRenderer(device: device, options: nil)
        renderer.scene = scene
        renderer.autoenablesDefaultLighting = true
        renderer.pointOfView = camera

        let side = tile * 2
        guard let grid = CGContext(
            data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { return nil }
        grid.setFillColor(CGColor(gray: 1, alpha: 1))
        grid.fill(CGRect(x: 0, y: 0, width: side, height: side))
        // From where the camera looks, and which way is up in the picture.
        let views: [(SCNVector3, SCNVector3)] = [
            (SCNVector3(1, -1, 0.8), SCNVector3(0, 0, 1)),  // angled
            (SCNVector3(0, -1, 0), SCNVector3(0, 0, 1)),  // front
            (SCNVector3(0, 0, 1), SCNVector3(0, 1, 0)),  // top
            (SCNVector3(1, 0, 0), SCNVector3(0, 0, 1)),  // side
        ]
        for (index, (from, up)) in views.enumerated() {
            let length = sqrt(from.x * from.x + from.y * from.y + from.z * from.z)
            let distance = radius * 2.2
            camera.position = SCNVector3(
                center.x + from.x / length * distance, center.y + from.y / length * distance,
                center.z + from.z / length * distance)
            camera.look(at: center, up: up, localFront: SCNVector3(0, 0, -1))
            let snapshot = renderer.snapshot(
                atTime: 0, with: CGSize(width: tile, height: tile), antialiasingMode: .multisampling4X)
            guard let image = snapshot.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                return nil
            }
            // Core Graphics counts rows from the bottom: the first two views go on top.
            grid.draw(image, in: CGRect(x: index % 2 * tile, y: (1 - index / 2) * tile, width: tile, height: tile))
        }
        return grid.makeImage()
    }
}

/// What a model's geometry says about it: its size, triangles, and separate parts.
struct Geometry: Equatable {
    /// The bounding box's width, depth and height, in the file's units.
    var extent: SIMD3<Float> = .zero
    var triangles = 0
    /// How many pieces aren't joined to each other by a shared corner.
    var parts = 0

    init(extent: SIMD3<Float>, triangles: Int, parts: Int) {
        self.extent = extent
        self.triangles = triangles
        self.parts = parts
    }

    init(_ asset: MDLAsset) {
        let box = asset.boundingBox
        extent = box.maxBounds - box.minBounds
        // Corners closer than this are the same corner: files repeat them per triangle.
        let tolerance = max(extent.max(), .leastNormalMagnitude) * 1e-5
        var corners: [SIMD3<Int32>: Int] = [:]
        var joined = Joined()
        for case let mesh as MDLMesh in asset.childObjects(of: MDLMesh.self) {
            guard let positions = mesh.vertexAttributeData(forAttributeNamed: MDLVertexAttributePosition, as: .float3)
            else { continue }
            let corner = { (vertex: Int) -> Int in
                let point = positions.dataStart.advanced(by: vertex * positions.stride)
                    .assumingMemoryBound(to: Float.self)
                let key = SIMD3<Int32>(
                    Int32(clamping: Int((point[0] / tolerance).rounded())),
                    Int32(clamping: Int((point[1] / tolerance).rounded())),
                    Int32(clamping: Int((point[2] / tolerance).rounded())))
                if let known = corners[key] { return known }
                let new = joined.add()
                corners[key] = new
                return new
            }
            for case let submesh as MDLSubmesh in mesh.submeshes ?? [] where submesh.geometryType == .triangles {
                let buffer = submesh.indexBuffer(asIndexType: .uInt32)
                let indices = buffer.map().bytes.assumingMemoryBound(to: UInt32.self)
                let count = submesh.indexCount - submesh.indexCount % 3
                triangles += count / 3
                for start in stride(from: 0, to: count, by: 3) {
                    let first = corner(Int(indices[start]))
                    joined.join(first, corner(Int(indices[start + 1])))
                    joined.join(first, corner(Int(indices[start + 2])))
                }
            }
        }
        parts = joined.groups
    }

    /// `86 × 86 × 19`.
    var size: String {
        [extent.x, extent.y, extent.z].map(Self.number).joined(separator: " × ")
    }

    /// `86 × 86 × 19 (mm, as stored), 10,220 triangles, 1 part`. STL has no units; most files
    /// are in millimetres, which the text says without insisting.
    var summary: String {
        let count = triangles.formatted(.number.locale(Locale(identifier: "en_US")))
        return "\(size) (mm, as stored), \(count) triangle\(triangles == 1 ? "" : "s"), "
            + "\(parts) part\(parts == 1 ? "" : "s")"
    }

    /// Three significant figures, more for whole numbers: `86`, `25.5`, `0.0912`, `1234`.
    static func number(_ value: Float) -> String {
        guard value > 0, value.isFinite else { return "0" }
        let decimals = min(max(0, 2 - Int(log10(value).rounded(.down))), 6)
        var text = String(format: "%.\(decimals)f", value)
        if text.contains(".") {
            while text.hasSuffix("0") { text.removeLast() }
            if text.hasSuffix(".") { text.removeLast() }
        }
        return text
    }
}

/// Which corners are joined to which, as a union–find.
struct Joined {
    private var parent: [Int] = []
    private(set) var groups = 0

    mutating func add() -> Int {
        parent.append(parent.count)
        groups += 1
        return parent.count - 1
    }

    mutating func root(_ item: Int) -> Int {
        var item = item
        while parent[item] != item {
            parent[item] = parent[parent[item]]
            item = parent[item]
        }
        return item
    }

    mutating func join(_ a: Int, _ b: Int) {
        let (rootA, rootB) = (root(a), root(b))
        if rootA != rootB {
            parent[rootA] = rootB
            groups -= 1
        }
    }
}
