// A squarified treemap layout (Bruls, Huizing and van Wijk): rectangles whose areas are in
// proportion to their values, kept as near square as the space allows, for the archive map.

import CoreGraphics

/// One tile of a treemap.
struct TreemapTile<ID: Hashable>: Equatable {
    let id: ID
    let rect: CGRect
}

enum Treemap {
    /// Tiles for `values` filling `rect`, largest first. Values of zero or less get no tile.
    static func layout<ID: Hashable>(_ values: [(id: ID, value: Double)], in rect: CGRect)
        -> [TreemapTile<ID>]
    {
        let positive = values.filter { $0.value > 0 }.sorted { $0.value > $1.value }
        let total = positive.reduce(0) { $0 + $1.value }
        guard total > 0, rect.width > 0, rect.height > 0 else { return [] }
        let scale = rect.width * rect.height / total
        var remaining = positive.map { (id: $0.id, area: $0.value * scale) }
        var space = rect
        var tiles: [TreemapTile<ID>] = []
        while !remaining.isEmpty {
            let side = min(space.width, space.height)
            var count = 1
            while count < remaining.count,
                worst(remaining[..<(count + 1)].map(\.area), side)
                    <= worst(remaining[..<count].map(\.area), side)
            {
                count += 1
            }
            let row = Array(remaining[..<count])
            remaining.removeFirst(count)
            let rowArea = row.reduce(0) { $0 + $1.area }
            if remaining.isEmpty {
                // The last row takes what's left, so rounding never leaves a gap.
                tiles += lay(row, rowArea: rowArea, in: space, filling: true)
                break
            }
            tiles += lay(row, rowArea: rowArea, in: space, filling: false)
            if space.width >= space.height {
                let width = rowArea / space.height
                space = CGRect(
                    x: space.minX + width, y: space.minY, width: space.width - width,
                    height: space.height)
            } else {
                let height = rowArea / space.width
                space = CGRect(
                    x: space.minX, y: space.minY + height, width: space.width,
                    height: space.height - height)
            }
        }
        return tiles
    }

    /// The row's tiles, along the shorter side of `space`: a column at its left when it's
    /// wide, a row at its top when it's tall.
    private static func lay<ID>(
        _ row: [(id: ID, area: Double)], rowArea: Double, in space: CGRect, filling: Bool
    ) -> [TreemapTile<ID>] {
        var tiles: [TreemapTile<ID>] = []
        if space.width >= space.height {
            let width = filling ? space.width : rowArea / space.height
            var y = space.minY
            for item in row {
                let height = space.height * item.area / rowArea
                tiles.append(
                    TreemapTile(id: item.id, rect: CGRect(x: space.minX, y: y, width: width, height: height)))
                y += height
            }
        } else {
            let height = filling ? space.height : rowArea / space.width
            var x = space.minX
            for item in row {
                let width = space.width * item.area / rowArea
                tiles.append(
                    TreemapTile(id: item.id, rect: CGRect(x: x, y: space.minY, width: width, height: height)))
                x += width
            }
        }
        return tiles
    }

    /// The worst aspect ratio in a row of `areas` laid along a side of length `side`.
    private static func worst(_ areas: [Double], _ side: Double) -> Double {
        let sum = areas.reduce(0, +)
        guard let largest = areas.max(), let smallest = areas.min(), sum > 0, smallest > 0
        else { return .infinity }
        let sideSquared = side * side
        let sumSquared = sum * sum
        return max(sideSquared * largest / sumSquared, sumSquared / (sideSquared * smallest))
    }
}
