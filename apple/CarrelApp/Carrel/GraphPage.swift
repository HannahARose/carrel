// Discover's Connections page: the tag and link graph drawn (docs/design/restructure.md, "The
// tag and link graph"), as `carrel discover graph` lists it. Collections are the nodes, round
// a circle, sized by how many items they hold; a line between two is as thick as what ties
// their items: shared tags, links and days of work together. Choosing a connection, in the
// drawing or the list, shows why they connect and the items that do, to open in Browse.

import CarrelKit
import SwiftUI

struct GraphPage: View {
    @Bindable var discover: DiscoverModel
    let window: ArchiveWindowModel

    @State private var chosen: GraphConnection.ID?
    @State private var focus: String?

    var body: some View {
        if let graph = discover.graph {
            if graph.connections.isEmpty {
                ContentUnavailableView(
                    "No Connections Yet", systemImage: DiscoverPage.graph.symbol,
                    description: Text(
                        "Collections connect when their items share tags or links, or are worked on the same days."
                    ))
            } else {
                SideBySide {
                    GraphDrawing(graph: graph, chosen: $chosen, focus: $focus)
                        .frame(minWidth: 320, minHeight: 320)
                    ConnectionList(
                        graph: graph, chosen: $chosen, focus: $focus, window: window
                    )
                    .frame(minWidth: 260, idealWidth: 320)
                }
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

extension GraphConnection {
    /// Why the two collections connect, in words.
    var ties: String {
        var parts: [String] = []
        if links > 0 { parts.append(links == 1 ? "1 link" : "\(links) links") }
        if days > 0 {
            parts.append(
                days == 1 ? "worked on the same day once" : "worked on the same day \(days) times")
        }
        if !tags.isEmpty {
            let shown = tags.prefix(5).map { "\($0.tag) (\($0.pairs))" }.joined(separator: ", ")
            let more = tags.count > 5 ? " and \(tags.count - 5) more" : ""
            parts.append("tags \(shown)\(more)")
        }
        return parts.joined(separator: "; ")
    }

    /// Whether `path` is one of its ends.
    func touches(_ path: String) -> Bool { a == path || b == path }
}

/// Where each collection sits in the drawing: round a circle in path order, so a collection's
/// sub-collections sit beside it.
struct GraphLayout {
    let positions: [String: CGPoint]
    let radius: [String: CGFloat]

    init(graph: DiscoveryGraph, in size: CGSize) {
        // Only collections with a connection are drawn; the rest would be dots on their own.
        let connected = Set(graph.connections.flatMap { [$0.a, $0.b] })
        let shown = graph.collections.filter { connected.contains($0.path) }
        let centre = CGPoint(x: size.width / 2, y: size.height / 2)
        let ring = max(min(size.width, size.height) / 2 - 70, 40)
        let largest = CGFloat(max(shown.map(\.items).max() ?? 1, 1))
        var positions: [String: CGPoint] = [:]
        var radius: [String: CGFloat] = [:]
        for (index, collection) in shown.enumerated() {
            let angle = 2 * .pi * CGFloat(index) / CGFloat(max(shown.count, 1)) - .pi / 2
            positions[collection.path] = CGPoint(
                x: centre.x + ring * cos(angle), y: centre.y + ring * sin(angle))
            radius[collection.path] = 6 + 14 * sqrt(CGFloat(collection.items) / largest)
        }
        self.positions = positions
        self.radius = radius
    }
}

struct GraphDrawing: View {
    let graph: DiscoveryGraph
    @Binding var chosen: GraphConnection.ID?
    @Binding var focus: String?

    var body: some View {
        GeometryReader { geometry in
            let layout = GraphLayout(graph: graph, in: geometry.size)
            let heaviest = graph.connections.map(\.weight).max() ?? 1
            ZStack {
                Canvas { context, _ in
                    for connection in graph.connections {
                        guard let from = layout.positions[connection.a],
                            let to = layout.positions[connection.b]
                        else { continue }
                        var line = Path()
                        line.move(to: from)
                        line.addLine(to: to)
                        let lit =
                            connection.id == chosen
                            || focus.map(connection.touches) == true
                        let faded = focus != nil && !lit
                        context.stroke(
                            line,
                            with: .color(
                                lit ? Color.accentColor : .secondary.opacity(faded ? 0.15 : 0.5)),
                            style: StrokeStyle(
                                lineWidth: 1 + 7 * connection.weight / heaviest, lineCap: .round,
                                dash: connection.separateBranches ? [] : [4, 4]))
                    }
                }
                ForEach(graph.collections.filter { layout.positions[$0.path] != nil }, id: \.path) {
                    collection in
                    let point = layout.positions[collection.path]!
                    let size = (layout.radius[collection.path] ?? 8) * 2
                    Circle()
                        .fill(focus == collection.path ? Color.accentColor : Color.gray)
                        .overlay { Circle().stroke(.background, lineWidth: 2) }
                        .frame(width: size, height: size)
                        .overlay(alignment: .top) {
                            Text(collection.title).font(.caption).lineLimit(1).fixedSize()
                                .padding(.horizontal, 3)
                                .background(.background.opacity(0.8), in: .rect(cornerRadius: 3))
                                .offset(y: size + 2)
                        }
                        .position(point)
                        .onTapGesture {
                            focus = focus == collection.path ? nil : collection.path
                            chosen = nil
                        }
                        .help(
                            "\(collection.path): \(collection.items == 1 ? "1 item" : "\(collection.items) items")"
                        )
                        .accessibilityElement(children: .combine)
                        .accessibilityAddTraits(.isButton)
                }
            }
        }
        .padding()
        .overlay(alignment: .bottomLeading) {
            Text("Thicker lines tie more; dashed ones join a collection to one inside it.")
                .font(.caption).foregroundStyle(.secondary).padding(8)
        }
    }
}

private struct ConnectionList: View {
    let graph: DiscoveryGraph
    @Binding var chosen: GraphConnection.ID?
    @Binding var focus: String?
    let window: ArchiveWindowModel

    var body: some View {
        List(selection: $chosen) {
            Section(focus.map { "Connections of \($0)" } ?? "Strongest Connections") {
                ForEach(listed) { connection in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(connection.a) ↔ \(connection.b)").bold()
                        Text(connection.ties).font(.caption).foregroundStyle(.secondary)
                    }
                    .tag(connection.id)
                }
            }
            if let connection = graph.connections.first(where: { $0.id == chosen }) {
                Section("Items That Tie Them") {
                    let ties = ties(of: connection)
                    if ties.isEmpty {
                        Text("Not among the strongest ties between items.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(Array(ties.enumerated()), id: \.offset) { _, tie in
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                itemButton(tie.a)
                                Image(systemName: "arrow.left.and.right").foregroundStyle(
                                    .secondary)
                                itemButton(tie.b)
                            }
                            Text(Self.words(tie)).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }

    /// The connections to list: the focused collection's, or all.
    private var listed: [GraphConnection] {
        guard let focus else { return graph.connections }
        return graph.connections.filter { $0.touches(focus) }
    }

    /// The ties between items, one in each of `connection`'s collections.
    private func ties(of connection: GraphConnection) -> [GraphItemTie] {
        graph.ties.filter {
            ($0.a.collection == connection.a && $0.b.collection == connection.b)
                || ($0.a.collection == connection.b && $0.b.collection == connection.a)
        }
    }

    private func itemButton(_ item: GraphItem) -> some View {
        Button(item.title + (item.sensitivity == .restricted ? " 🔒" : "")) {
            window.open(item: item.id)
        }
        .linkButtonStyle()
        .lineLimit(1)
        .help("\(item.card): show it in Browse")
    }

    /// What ties two items, in words.
    static func words(_ tie: GraphItemTie) -> String {
        var parts: [String] = []
        if !tie.links.isEmpty { parts.append("linked (\(tie.links.joined(separator: ", ")))") }
        if tie.days > 0 {
            parts.append(
                tie.days == 1 ? "worked on the same day" : "worked on together on \(tie.days) days")
        }
        if !tie.tags.isEmpty { parts.append("tags \(tie.tags.joined(separator: ", "))") }
        return parts.joined(separator: "; ")
    }
}
