// Draws the benchmark's invented images into `items` (docs/design/images.md, step 5), with
// the metadata a phone, the screenshot tool or a scanner would write. Everything in them is
// made up for Carrel. Run it from this folder to make them again:
//
//     swift make-images.swift

import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A canvas with its origin at the top left, as the pictures are laid out.
final class Canvas {
    let context: CGContext
    let width: Int
    let height: Int

    init(_ width: Int, _ height: Int, background: CGColor) {
        self.width = width
        self.height = height
        context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 1, y: -1)
        fill(CGRect(x: 0, y: 0, width: width, height: height), background)
    }

    func fill(_ rect: CGRect, _ color: CGColor) {
        context.setFillColor(color)
        context.fill(rect)
    }

    func fillEllipse(_ rect: CGRect, _ color: CGColor) {
        context.setFillColor(color)
        context.fillEllipse(in: rect)
    }

    func stroke(_ rect: CGRect, _ color: CGColor, width: CGFloat) {
        context.setStrokeColor(color)
        context.setLineWidth(width)
        context.stroke(rect)
    }

    func line(_ points: [CGPoint], _ color: CGColor, width: CGFloat) {
        context.setStrokeColor(color)
        context.setLineWidth(width)
        context.setLineCap(.round)
        context.addLines(between: points)
        context.strokePath()
    }

    func polygon(_ points: [CGPoint], _ color: CGColor) {
        context.setFillColor(color)
        context.addLines(between: points)
        context.closePath()
        context.fillPath()
    }

    /// An arrow from `from` to `to`, with its head at `to`.
    func arrow(_ from: CGPoint, _ to: CGPoint, _ color: CGColor, width: CGFloat) {
        line([from, to], color, width: width)
        let angle = atan2(to.y - from.y, to.x - from.x)
        let size = width * 5
        polygon(
            [
                to,
                CGPoint(x: to.x - size * cos(angle - 0.4), y: to.y - size * sin(angle - 0.4)),
                CGPoint(x: to.x - size * cos(angle + 0.4), y: to.y - size * sin(angle + 0.4)),
            ], color)
    }

    /// `lines` of text from `origin` (the first baseline), each `leading` below the last,
    /// turned by `angle` radians about `origin`.
    func text(
        _ lines: [String], at origin: CGPoint, font: String, size: CGFloat, color: CGColor,
        leading: CGFloat? = nil, angle: CGFloat = 0
    ) {
        let ctFont = CTFontCreateWithName(font as CFString, size, nil)
        context.saveGState()
        context.translateBy(x: origin.x, y: origin.y)
        context.rotate(by: angle)
        for (n, line) in lines.enumerated() {
            let attributed = NSAttributedString(
                string: line,
                attributes: [
                    NSAttributedString.Key(kCTFontAttributeName as String): ctFont,
                    NSAttributedString.Key(kCTForegroundColorAttributeName as String): color,
                ])
            let ctLine = CTLineCreateWithAttributedString(attributed)
            context.saveGState()
            context.translateBy(x: 0, y: CGFloat(n) * (leading ?? size * 1.35))
            context.scaleBy(x: 1, y: -1)
            context.textPosition = .zero
            CTLineDraw(ctLine, context)
            context.restoreGState()
        }
        context.restoreGState()
    }

    /// Turns everything drawn after it by `angle` about `center`, until `restore`.
    func turn(_ angle: CGFloat, about center: CGPoint) {
        context.saveGState()
        context.translateBy(x: center.x, y: center.y)
        context.rotate(by: angle)
        context.translateBy(x: -center.x, y: -center.y)
    }

    func restore() { context.restoreGState() }

    /// Light falling off towards the edges, as in a phone photo.
    func vignette() {
        let colors = [rgb(0, 0, 0, 0), rgb(0, 0, 0, 0.28)] as CFArray
        let gradient = CGGradient(
            colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: colors, locations: [0.55, 1])!
        let center = CGPoint(x: width / 2, y: height / 2)
        context.drawRadialGradient(
            gradient, startCenter: center, startRadius: 0, endCenter: center,
            endRadius: CGFloat(max(width, height)) * 0.75, options: .drawsAfterEndLocation)
    }

    /// Writes the picture to `items/name` with `properties`.
    func save(_ name: String, as type: UTType, properties: [CFString: Any] = [:]) {
        let url = URL(fileURLWithPath: "items").appendingPathComponent(name)
        let destination = CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil)!
        var options = properties
        if type == .jpeg {
            options[kCGImageDestinationLossyCompressionQuality] = 0.72
        }
        CGImageDestinationAddImage(destination, context.makeImage()!, options as CFDictionary)
        precondition(CGImageDestinationFinalize(destination), "couldn't write \(name)")
        print("wrote items/\(name)")
    }
}

func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r, green: g, blue: b, alpha: a)
}

func gray(_ white: CGFloat) -> CGColor { rgb(white, white, white) }

/// What a phone writes into a photo: when, on what, and, given `location`, where.
func phone(_ taken: String, offset: String, model: String, location: (Double, Double)? = nil) -> [CFString: Any] {
    var properties: [CFString: Any] = [
        kCGImagePropertyExifDictionary: [
            kCGImagePropertyExifDateTimeOriginal: taken,
            kCGImagePropertyExifDateTimeDigitized: taken,
            "OffsetTimeOriginal" as CFString: offset,
            kCGImagePropertyExifExposureTime: 1.0 / 60,
            kCGImagePropertyExifFNumber: 1.8,
        ] as [CFString: Any],
        kCGImagePropertyTIFFDictionary: [
            kCGImagePropertyTIFFMake: "Apple", kCGImagePropertyTIFFModel: model,
        ] as [CFString: Any],
    ]
    if let (latitude, longitude) = location {
        properties[kCGImagePropertyGPSDictionary] = [
            kCGImagePropertyGPSLatitude: abs(latitude),
            kCGImagePropertyGPSLatitudeRef: latitude >= 0 ? "N" : "S",
            kCGImagePropertyGPSLongitude: abs(longitude),
            kCGImagePropertyGPSLongitudeRef: longitude >= 0 ? "E" : "W",
        ] as [CFString: Any]
    }
    return properties
}

/// What macOS writes into a screenshot.
func screenshot(_ taken: String) -> [CFString: Any] {
    [
        kCGImagePropertyExifDictionary: [
            kCGImagePropertyExifUserComment: "Screenshot",
            kCGImagePropertyExifDateTimeOriginal: taken,
        ] as [CFString: Any]
    ]
}

/// A wooden table top, in planks.
func table(_ canvas: Canvas, base: (CGFloat, CGFloat, CGFloat)) {
    let plank = canvas.height / 5
    for n in 0..<6 {
        let shade = CGFloat(n % 2 == 0 ? 0 : 0.05) - CGFloat(n) * 0.01
        canvas.fill(
            CGRect(x: 0, y: n * plank, width: canvas.width, height: plank),
            rgb(base.0 - shade, base.1 - shade, base.2 - shade))
        for grain in stride(from: 40, to: plank, by: 37) {
            canvas.line(
                [CGPoint(x: 0, y: n * plank + grain), CGPoint(x: canvas.width, y: n * plank + grain + 9)],
                rgb(base.0 - 0.12, base.1 - 0.12, base.2 - 0.1, 0.35), width: 3)
        }
        canvas.line(
            [CGPoint(x: 0, y: n * plank), CGPoint(x: canvas.width, y: n * plank)],
            rgb(0.25, 0.15, 0.08), width: 4)
    }
}

// A till receipt for a kettle, photographed on a kitchen table.
do {
    let canvas = Canvas(2016, 1512, background: gray(0.5))
    table(canvas, base: (0.62, 0.45, 0.3))
    let center = CGPoint(x: 1008, y: 756)
    canvas.turn(-0.06, about: center)
    canvas.fill(CGRect(x: 718, y: 70, width: 600, height: 1400), rgb(0, 0, 0, 0.25))
    canvas.fill(CGRect(x: 700, y: 50, width: 600, height: 1400), rgb(0.97, 0.96, 0.93))
    canvas.text(
        ["HARTLEY & SONS", "HARDWARE"], at: CGPoint(x: 800, y: 150), font: "Menlo-Bold", size: 44,
        color: gray(0.1))
    canvas.text(
        [
            "22 Mill Lane, Brackley",
            "Tel 01632 960 214",
            "",
            "14/06/2025        11:52",
            "Till 2   Receipt 004817",
            "--------------------------",
            "Electric kettle 1.7L",
            "  KX-170 steel     34.99",
            "Descaler sachets    4.50",
            "--------------------------",
            "TOTAL              39.49",
            "Card ************4417",
            "Contactless      APPROVED",
            "",
            "Returns within 30 days",
            "with this receipt.",
            "Kettle: 2 year guarantee",
            "",
            "Thank you for shopping",
            "local!",
        ], at: CGPoint(x: 740, y: 300), font: "Menlo", size: 30, color: gray(0.15), leading: 50)
    canvas.restore()
    canvas.vignette()
    canvas.save("IMG_2041.jpg", as: .jpeg, properties: phone("2025:06:14 12:05:31", offset: "+01:00", model: "iPhone 14"))
}

// A whiteboard after a planning meeting at work.
do {
    let canvas = Canvas(2016, 1512, background: rgb(0.78, 0.8, 0.82))
    canvas.fill(CGRect(x: 0, y: 1300, width: 2016, height: 212), rgb(0.55, 0.5, 0.45))
    canvas.fill(CGRect(x: 110, y: 90, width: 1800, height: 1180), gray(0.62))
    canvas.fill(CGRect(x: 130, y: 110, width: 1760, height: 1140), rgb(0.96, 0.97, 0.97))
    canvas.fill(CGRect(x: 600, y: 1268, width: 800, height: 22), gray(0.55))
    let blue = rgb(0.1, 0.25, 0.7), red = rgb(0.8, 0.12, 0.12), green = rgb(0.1, 0.5, 0.2)
    canvas.text(["Sprint 14 planning - 7 Oct"], at: CGPoint(x: 220, y: 230), font: "MarkerFelt-Wide", size: 84, color: blue)
    canvas.line([CGPoint(x: 220, y: 260), CGPoint(x: 1150, y: 255)], blue, width: 6)
    canvas.text(["TO DO"], at: CGPoint(x: 240, y: 380), font: "MarkerFelt-Wide", size: 58, color: red)
    canvas.text(["DOING"], at: CGPoint(x: 860, y: 380), font: "MarkerFelt-Wide", size: 58, color: red)
    canvas.text(["DONE"], at: CGPoint(x: 1450, y: 380), font: "MarkerFelt-Wide", size: 58, color: red)
    for x in [790, 1380] {
        canvas.line([CGPoint(x: x, y: 330), CGPoint(x: x + 6, y: 1180)], gray(0.3), width: 5)
    }
    let notes: [(CGFloat, CGFloat, String, String, CGColor)] = [
        (230, 450, "Export to CSV", "(Priya)", rgb(1, 0.93, 0.45)),
        (230, 720, "Fix login", "timeout", rgb(1, 0.93, 0.45)),
        (850, 450, "Dashboard", "filters (Sam)", rgb(0.6, 0.85, 1)),
        (1440, 450, "Release 3.2", "notes", rgb(0.7, 0.95, 0.6)),
        (1440, 720, "On-call rota", "Q4", rgb(0.7, 0.95, 0.6)),
    ]
    for (x, y, first, second, color) in notes {
        canvas.fill(CGRect(x: x, y: y, width: 300, height: 220), color)
        canvas.text([first, second], at: CGPoint(x: x + 22, y: y + 80), font: "MarkerFelt-Thin", size: 42, color: gray(0.15))
    }
    canvas.text(
        ["Retro: Thu 2pm", "Demo to client 21 Oct"], at: CGPoint(x: 850, y: 860), font: "MarkerFelt-Wide",
        size: 52, color: green, leading: 80)
    canvas.vignette()
    canvas.save("IMG_3310.jpg", as: .jpeg, properties: phone("2025:10:07 10:32:14", offset: "+01:00", model: "iPhone 15 Pro"))
}

// A ginger cat asleep on a sofa, at home.
do {
    let canvas = Canvas(2016, 1512, background: rgb(0.85, 0.82, 0.74))
    canvas.fill(CGRect(x: 1400, y: 0, width: 616, height: 700), rgb(0.95, 0.96, 0.98))
    canvas.fill(CGRect(x: 1390, y: 0, width: 20, height: 700), gray(0.9))
    canvas.fill(CGRect(x: 0, y: 650, width: 2016, height: 862), rgb(0.2, 0.32, 0.5))
    canvas.fill(CGRect(x: 0, y: 600, width: 2016, height: 140), rgb(0.24, 0.37, 0.56))
    canvas.fillEllipse(CGRect(x: 150, y: 520, width: 420, height: 360), rgb(0.85, 0.75, 0.4))
    let fur = rgb(0.9, 0.55, 0.2), dark = rgb(0.7, 0.38, 0.12)
    // The tail, the body and the head.
    canvas.line([CGPoint(x: 1450, y: 1080), CGPoint(x: 1600, y: 1180), CGPoint(x: 1500, y: 1280)], fur, width: 70)
    canvas.fillEllipse(CGRect(x: 650, y: 820, width: 900, height: 470), fur)
    for x in stride(from: 760, to: 1450, by: 120) {
        canvas.line([CGPoint(x: x, y: 840), CGPoint(x: x + 40, y: 990)], dark, width: 26)
    }
    canvas.fillEllipse(CGRect(x: 470, y: 760, width: 420, height: 380), fur)
    canvas.polygon([CGPoint(x: 500, y: 860), CGPoint(x: 520, y: 650), CGPoint(x: 650, y: 790)], fur)
    canvas.polygon([CGPoint(x: 730, y: 790), CGPoint(x: 860, y: 650), CGPoint(x: 870, y: 860)], fur)
    canvas.polygon([CGPoint(x: 535, y: 830), CGPoint(x: 545, y: 710), CGPoint(x: 620, y: 795)], rgb(0.95, 0.7, 0.65))
    canvas.polygon([CGPoint(x: 760, y: 795), CGPoint(x: 840, y: 710), CGPoint(x: 845, y: 830)], rgb(0.95, 0.7, 0.65))
    // Closed eyes, a nose and whiskers.
    canvas.line([CGPoint(x: 570, y: 930), CGPoint(x: 620, y: 945), CGPoint(x: 660, y: 930)], gray(0.15), width: 8)
    canvas.line([CGPoint(x: 710, y: 930), CGPoint(x: 750, y: 945), CGPoint(x: 800, y: 930)], gray(0.15), width: 8)
    canvas.polygon([CGPoint(x: 665, y: 990), CGPoint(x: 705, y: 990), CGPoint(x: 685, y: 1015)], rgb(0.85, 0.45, 0.45))
    for dy in [-10, 15, 40] {
        canvas.line([CGPoint(x: 640, y: 1020), CGPoint(x: 470, y: 1000 + dy * 2)], gray(0.95), width: 3)
        canvas.line([CGPoint(x: 730, y: 1020), CGPoint(x: 900, y: 1000 + dy * 2)], gray(0.95), width: 3)
    }
    canvas.fillEllipse(CGRect(x: 560, y: 1090, width: 140, height: 70), fur)
    canvas.fillEllipse(CGRect(x: 690, y: 1100, width: 140, height: 70), fur)
    canvas.vignette()
    canvas.save(
        "IMG_4127.jpg", as: .jpeg,
        properties: phone("2025:09:21 16:10:48", offset: "+01:00", model: "iPhone 15 Pro", location: (53.7415, -2.0157)))
}

// A station's departure board, photographed on a trip.
do {
    let canvas = Canvas(2016, 1512, background: rgb(0.92, 0.93, 0.95))
    // Blue and white tiles around the board.
    for row in 0..<12 {
        for column in 0..<16 {
            let tile = CGRect(x: column * 128, y: row * 128, width: 124, height: 124)
            canvas.fill(tile, (row + column) % 2 == 0 ? rgb(0.15, 0.3, 0.65) : rgb(0.93, 0.94, 0.97))
        }
    }
    canvas.fill(CGRect(x: 160, y: 260, width: 1700, height: 900), gray(0.25))
    canvas.fill(CGRect(x: 180, y: 280, width: 1660, height: 860), gray(0.04))
    let amber = rgb(1, 0.7, 0.1)
    canvas.text(["PARTIDAS  DEPARTURES"], at: CGPoint(x: 240, y: 380), font: "Menlo-Bold", size: 64, color: amber)
    canvas.text(["Porto Sao Bento"], at: CGPoint(x: 1300, y: 380), font: "Menlo", size: 40, color: amber)
    canvas.text(
        [
            "HORA   DESTINO           LINHA",
            "18:55  Braga               2",
            "19:10  Guimaraes           4",
            "19:25  Aveiro              1",
            "19:40  Marco de Canaveses  3",
            "20:05  Regua               5",
        ], at: CGPoint(x: 240, y: 520), font: "Menlo-Bold", size: 56, color: amber, leading: 110)
    canvas.vignette()
    canvas.save(
        "IMG_1876.jpg", as: .jpeg,
        properties: phone("2025:05:17 18:42:10", offset: "+01:00", model: "iPhone 15 Pro", location: (41.1456, -8.6105)))
}

// A handwritten recipe card on a kitchen counter.
do {
    let canvas = Canvas(2016, 1512, background: gray(0.82))
    for x in stride(from: 0, to: 2016, by: 252) {
        canvas.line([CGPoint(x: x, y: 0), CGPoint(x: x, y: 1512)], gray(0.72), width: 4)
    }
    canvas.turn(0.04, about: CGPoint(x: 1008, y: 756))
    canvas.fill(CGRect(x: 300, y: 240, width: 1440, height: 1040), rgb(0, 0, 0, 0.2))
    canvas.fill(CGRect(x: 280, y: 220, width: 1440, height: 1040), rgb(0.99, 0.98, 0.94))
    canvas.line([CGPoint(x: 280, y: 360), CGPoint(x: 1720, y: 360)], rgb(0.85, 0.3, 0.3), width: 3)
    for y in stride(from: 450, to: 1260, by: 78) {
        canvas.line([CGPoint(x: 280, y: y), CGPoint(x: 1720, y: y)], rgb(0.6, 0.75, 0.9), width: 2)
    }
    canvas.text(["Gran's oat biscuits"], at: CGPoint(x: 340, y: 330), font: "BradleyHandITCTT-Bold", size: 72, color: gray(0.12))
    canvas.text(
        [
            "100g butter, 100g sugar, 1 tbsp golden syrup",
            "100g rolled oats, 100g self-raising flour",
            "Melt butter, sugar and syrup. Stir in oats",
            "and flour. Roll into balls, press flat.",
            "Bake 12-15 min at 180C until golden.",
            "Makes about 20 - double it for Christmas!",
        ], at: CGPoint(x: 340, y: 520), font: "BradleyHandITCTT-Bold", size: 52, color: rgb(0.1, 0.15, 0.4),
        leading: 117)
    canvas.restore()
    canvas.vignette()
    canvas.save("IMG_1502.jpg", as: .jpeg, properties: phone("2025:04:20 15:03:22", offset: "+01:00", model: "iPhone 14"))
}

// A screenshot of a letter from the landlord, open in a mail app.
do {
    let canvas = Canvas(1440, 900, background: gray(0.96))
    canvas.fill(CGRect(x: 0, y: 0, width: 1440, height: 52), gray(0.88))
    for (n, color) in [rgb(1, 0.37, 0.34), rgb(1, 0.74, 0.18), rgb(0.16, 0.79, 0.25)].enumerated() {
        canvas.fillEllipse(CGRect(x: 20 + n * 26, y: 18, width: 16, height: 16), color)
    }
    canvas.fill(CGRect(x: 0, y: 52, width: 360, height: 848), gray(0.92))
    canvas.text(
        ["Inbox", "", "Oakfield Lettings", "Rent review: 4 Elm Court", "", "Northbridge Bank", "Your statement is ready"],
        at: CGPoint(x: 24, y: 100), font: "HelveticaNeue", size: 18, color: gray(0.25), leading: 30)
    canvas.fill(CGRect(x: 360, y: 52, width: 1080, height: 848), gray(1))
    canvas.text(["Rent review: 4 Elm Court"], at: CGPoint(x: 400, y: 110), font: "HelveticaNeue-Bold", size: 28, color: gray(0.1))
    canvas.text(
        ["From: Oakfield Lettings <lettings@oakfield.example>", "To: Alex Morgan", "28 August 2025"],
        at: CGPoint(x: 400, y: 150), font: "HelveticaNeue", size: 16, color: gray(0.45), leading: 24)
    canvas.text(
        [
            "Dear Alex,",
            "",
            "Following the annual review under clause 4 of your tenancy agreement,",
            "the rent for Flat 4, Elm Court will change from 950.00 to 985.00 a month",
            "from 1 October 2025. Your standing order will need updating before the",
            "first payment at the new rate.",
            "",
            "Everything else in the agreement stays the same. If you have any",
            "questions, reply to this email or call the office on 01632 960 555.",
            "",
            "Kind regards,",
            "Dana Price",
            "Oakfield Lettings",
        ], at: CGPoint(x: 400, y: 260), font: "HelveticaNeue", size: 20, color: gray(0.15), leading: 32)
    canvas.save("Screenshot 2025-08-28 at 09.14.03.png", as: .png, properties: screenshot("2025:08:28 09:14:03"))
}

// A screenshot of a banking app, with the account number showing.
do {
    let canvas = Canvas(1179, 2556, background: gray(0.97))
    canvas.fill(CGRect(x: 0, y: 0, width: 1179, height: 700), rgb(0.05, 0.3, 0.35))
    canvas.text(["9:41"], at: CGPoint(x: 90, y: 90), font: "HelveticaNeue-Bold", size: 46, color: gray(1))
    canvas.text(["Northbridge Bank"], at: CGPoint(x: 80, y: 260), font: "HelveticaNeue-Bold", size: 64, color: gray(1))
    canvas.text(
        ["Current account", "IBAN GB82 WEST 1234 5698 7654 32"], at: CGPoint(x: 80, y: 360),
        font: "HelveticaNeue", size: 42, color: gray(0.9), leading: 64)
    canvas.text(["3,412.07"], at: CGPoint(x: 80, y: 600), font: "HelveticaNeue-Bold", size: 120, color: gray(1))
    canvas.text(["Available balance"], at: CGPoint(x: 80, y: 660), font: "HelveticaNeue", size: 36, color: gray(0.85))
    canvas.text(["Recent"], at: CGPoint(x: 80, y: 840), font: "HelveticaNeue-Bold", size: 52, color: gray(0.1))
    let rows = [
        ("3 Oct", "Grocer's, High Street", "-23.18"),
        ("2 Oct", "Oakfield Lettings", "-985.00"),
        ("1 Oct", "Salary, Lumen Analytics", "+2,870.42"),
        ("29 Sep", "Northern Rail", "-14.60"),
        ("27 Sep", "Transfer to savings", "-300.00"),
    ]
    for (n, row) in rows.enumerated() {
        let y = CGFloat(980 + n * 170)
        canvas.fill(CGRect(x: 60, y: y - 70, width: 1059, height: 150), gray(1))
        canvas.text([row.1], at: CGPoint(x: 100, y: y), font: "HelveticaNeue", size: 44, color: gray(0.1))
        canvas.text([row.0], at: CGPoint(x: 100, y: y + 52), font: "HelveticaNeue", size: 34, color: gray(0.5))
        canvas.text([row.2], at: CGPoint(x: 820, y: y + 20), font: "HelveticaNeue-Medium", size: 44, color: gray(0.1))
    }
    canvas.save("Screenshot 2025-10-03 at 08.41.55.png", as: .png, properties: screenshot("2025:10:03 08:41:55"))
}

// A scanned letter from a friend.
do {
    let canvas = Canvas(1700, 2200, background: gray(0.9))
    canvas.fill(CGRect(x: 20, y: 15, width: 1660, height: 2170), rgb(0.98, 0.97, 0.95))
    canvas.text(
        ["14 Elm Row", "Hebden Bridge", "", "3 March 2025"], at: CGPoint(x: 1150, y: 220), font: "TimesNewRomanPSMT",
        size: 38, color: gray(0.15), leading: 52)
    canvas.text(
        [
            "Dear Alex,",
            "",
            "Thank you for the parcel - the scarf is perfect, and the colour is",
            "exactly the green of the hills behind the house. I wore it on the walk",
            "up to the reservoir on Sunday and got two compliments from strangers.",
            "",
            "The allotment is waking up. Ben and I dug the new bed last weekend and",
            "the broad beans are already through. If you come up at Easter you can",
            "help with the potatoes, which I'm told is a privilege.",
            "",
            "Do write back and tell me how the new job is going. I want to hear",
            "everything, especially about the colleague with the bicycle.",
            "",
            "With love,",
            "Jo",
        ], at: CGPoint(x: 160, y: 560), font: "TimesNewRomanPSMT", size: 38, color: gray(0.15), leading: 60)
    canvas.save(
        "scan0007.png", as: .png,
        properties: [kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFSoftware: "Image Capture"] as [CFString: Any]])
}

// A wiring diagram for a home electronics project, exported from a drawing app.
do {
    let canvas = Canvas(1600, 1000, background: gray(1))
    let ink = gray(0.15), wire = rgb(0.75, 0.2, 0.15), signal = rgb(0.15, 0.35, 0.75)
    canvas.text(["Plant monitor v2 - wiring"], at: CGPoint(x: 60, y: 90), font: "HelveticaNeue-Bold", size: 48, color: ink)
    let boxes: [(CGRect, [String])] = [
        (CGRect(x: 640, y: 380, width: 320, height: 200), ["ESP32-C3", "dev board"]),
        (CGRect(x: 100, y: 200, width: 330, height: 150), ["Solar panel", "1 W, 6 V"]),
        (CGRect(x: 100, y: 620, width: 330, height: 150), ["18650 cell", "+ TP4056 charger"]),
        (CGRect(x: 1170, y: 200, width: 330, height: 150), ["Soil moisture", "sensor (capacitive)"]),
        (CGRect(x: 1170, y: 620, width: 330, height: 150), ["Relay module", "-> 5 V pump"]),
    ]
    for (rect, label) in boxes {
        canvas.fill(rect, rgb(0.93, 0.96, 1))
        canvas.stroke(rect, ink, width: 4)
        canvas.text(label, at: CGPoint(x: rect.minX + 24, y: rect.minY + 62), font: "HelveticaNeue", size: 32, color: ink, leading: 44)
    }
    canvas.arrow(CGPoint(x: 265, y: 350), CGPoint(x: 265, y: 610), wire, width: 6)
    canvas.arrow(CGPoint(x: 430, y: 695), CGPoint(x: 630, y: 540), wire, width: 6)
    canvas.arrow(CGPoint(x: 1170, y: 275), CGPoint(x: 970, y: 420), signal, width: 6)
    canvas.arrow(CGPoint(x: 960, y: 540), CGPoint(x: 1160, y: 695), signal, width: 6)
    canvas.text(["charge"], at: CGPoint(x: 285, y: 490), font: "HelveticaNeue-Italic", size: 28, color: wire)
    canvas.text(["3.7 V -> 3V3 LDO"], at: CGPoint(x: 450, y: 820), font: "HelveticaNeue-Italic", size: 28, color: wire)
    canvas.text(["ADC GPIO2"], at: CGPoint(x: 900, y: 290), font: "HelveticaNeue-Italic", size: 28, color: signal)
    canvas.text(["GPIO5"], at: CGPoint(x: 1090, y: 600), font: "HelveticaNeue-Italic", size: 28, color: signal)
    canvas.text(
        ["Deep sleep 15 min between readings; pump runs 4 s when moisture < 30%"],
        at: CGPoint(x: 60, y: 930), font: "HelveticaNeue", size: 28, color: gray(0.4))
    canvas.save("export.png", as: .png)
}
