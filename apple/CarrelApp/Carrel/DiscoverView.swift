// The Discover section's pages: on this day, wander the stacks with previews, the timeline,
// the archive map as a treemap and activity as a heatmap. Choosing an item shows its card in
// the inspector; its context menu asks the librarian about it.

import CarrelKit
import SwiftUI

struct DiscoverView: View {
    let page: DiscoverPage
    @Bindable var discover: DiscoverModel
    let window: ArchiveWindowModel

    var body: some View {
        Group {
            switch page {
            case .today: OnThisDayPage(discover: discover, window: window)
            case .wander: WanderPage(discover: discover, window: window)
            case .timeline: TimelinePage(discover: discover, window: window)
            case .map: ArchiveMapPage(discover: discover, window: window)
            case .activity: ActivityPage(discover: discover)
            }
        }
        .overlay(alignment: .bottom) {
            if let problem = discover.problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .padding(8)
                    .background(.regularMaterial, in: .rect(cornerRadius: 6))
                    .padding()
            }
        }
        .task(id: page) { await discover.refresh(page) }
    }
}

/// A discovered item as a row: its title, collection and a mark when it's sensitive.
private struct DiscoveredRow: View {
    let item: DiscoveredItem
    var detail: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Label(item.title, systemImage: item.kind.symbol).lineLimit(1)
                if item.sensitivity == .sensitive || item.sensitivity == .restricted {
                    Image(systemName: item.sensitivity == .restricted ? "lock.fill" : "lock")
                        .foregroundStyle(.secondary)
                        .help(item.sensitivity == .restricted ? "Restricted" : "Sensitive")
                }
            }
            Text([item.collection, detail].compactMap(\.self).joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
        .tag(item.id)
    }
}

/// The context menu for a discovered item: open it in Browse, or ask the librarian.
private struct DiscoveredItemMenu: View {
    let window: ArchiveWindowModel
    let id: String?

    var body: some View {
        if let id, let item = window.items.first(where: { $0.id == id }) {
            Button("Show in Browse") { window.open(item: id) }
            AskAboutButton(model: window, item: item)
        }
    }
}

// MARK: - On this day

private struct OnThisDayPage: View {
    @Bindable var discover: DiscoverModel
    @Bindable var window: ArchiveWindowModel

    var body: some View {
        List(selection: $window.selectedItemID) {
            if let day = discover.onThisDay {
                ForEach(day.years, id: \.year) { year in
                    Section(
                        year.yearsAgo == 1
                            ? "A year ago · \(String(year.year))"
                            : "\(year.yearsAgo) years ago · \(String(year.year))"
                    ) {
                        ForEach(year.items, id: \.item.id) { happened in
                            DiscoveredRow(
                                item: happened.item,
                                detail: happened.what.map(\.capitalized).joined(separator: ", "))
                        }
                    }
                }
            }
        }
        .contextMenu(forSelectionType: String.self) { ids in
            DiscoveredItemMenu(window: window, id: ids.first)
        }
        .overlay {
            if discover.onThisDay?.years.isEmpty == true {
                ContentUnavailableView(
                    "Nothing on This Day", systemImage: "calendar",
                    description: Text(
                        "Nothing was made, filed or worked on on this date in other years."))
            }
        }
        .toolbar {
            DatePicker("Day", selection: $discover.day, displayedComponents: .date)
                .help("Look back from another day")
        }
        .onChange(of: discover.day) { Task { await discover.refreshToday() } }
    }
}

// MARK: - Wander

private struct WanderPage: View {
    let discover: DiscoverModel
    let window: ArchiveWindowModel

    var body: some View {
        VStack(spacing: 0) {
            if let step = discover.currentStep {
                ScrollView {
                    WanderStepView(step: step, window: window)
                        .padding()
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .id(step.number)
                if discover.steps.count > 1 {
                    Divider()
                    trail
                }
            } else if discover.walkIsDone {
                ContentUnavailableView(
                    "Nowhere Left to Go", systemImage: "figure.walk",
                    description: Text("This walk has visited every item it can reach."))
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .toolbar {
            ToolbarItemGroup {
                Button("Start Again", systemImage: "arrow.counterclockwise") {
                    Task { await discover.startWalk() }
                }
                .help("Start a new walk")
                Button("Next", systemImage: "arrow.right") { Task { await discover.step() } }
                    .keyboardShortcut(.rightArrow, modifiers: [])
                    .help("Go on to the next item (→)")
                    .disabled(discover.walkIsDone || discover.isStepping)
            }
        }
        .onChange(of: discover.currentStep?.item.id) {
            if let id = discover.currentStep?.item.id { window.selectedItemID = id }
        }
    }

    /// The items visited so far, newest first, to go back to.
    private var trail: some View {
        ScrollView(.horizontal) {
            HStack {
                ForEach(discover.steps.dropLast().reversed(), id: \.number) { step in
                    Button {
                        window.selectedItemID = step.item.id
                    } label: {
                        Text("\(step.number). \(step.item.title)").lineLimit(1)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
            .padding(8)
        }
        .frame(height: 44)
    }
}

/// One step of a walk: why it came next, its preview, and its card's summary and tags.
private struct WanderStepView: View {
    let step: WanderStep
    let window: ArchiveWindowModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Step \(step.number)").font(.caption).foregroundStyle(.secondary)
            Text(step.item.title).font(.title2.bold()).textSelection(.enabled)
            Label(step.reason.words, systemImage: "arrow.turn.down.right")
                .foregroundStyle(.secondary)
            if let item = window.items.first(where: { $0.id == step.item.id }),
                let previews = window.previews
            {
                ItemPreviewView(item: item, store: previews)
                    .clipShape(.rect(cornerRadius: 6))
            }
            if let summary = step.summary, !summary.isEmpty {
                Text(summary).textSelection(.enabled)
            }
            if let preview = step.preview {
                Text(preview)
                    .font(.callout).foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .padding(10)
                    .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 6))
            }
            if !step.tags.isEmpty {
                Text(step.tags.map { "#\($0)" }.joined(separator: "  "))
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button("Show in Browse") { window.open(item: step.item.id) }
                if let item = window.items.first(where: { $0.id == step.item.id }) {
                    AskAboutButton(model: window, item: item)
                }
            }
        }
    }
}

// MARK: - Timeline

private struct TimelinePage: View {
    @Bindable var discover: DiscoverModel
    @Bindable var window: ArchiveWindowModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Words or filters, such as tag:receipts", text: $discover.timelineQuery)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await discover.refreshTimeline() } }
                Picker("By", selection: $discover.timelineDate) {
                    ForEach(TimelineDate.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .fixedSize()
                Picker("Per", selection: $discover.timelineGrouping) {
                    ForEach(TimelineGrouping.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
            .padding(8)
            Divider()
            List(selection: $window.selectedItemID) {
                ForEach((discover.timeline?.periods ?? []).reversed(), id: \.period) { period in
                    Section {
                        ForEach(period.items, id: \.self) { placed in
                            DiscoveredRow(
                                item: placed.item, detail: "\(placed.what.capitalized) \(placed.date)")
                        }
                    } header: {
                        PeriodHeader(period: period, most: busiestPeriod)
                    }
                }
            }
            .contextMenu(forSelectionType: String.self) { ids in
                DiscoveredItemMenu(window: window, id: ids.first)
            }
            .overlay {
                if discover.timeline?.periods.isEmpty == true {
                    ContentUnavailableView(
                        "Nothing on the Timeline", systemImage: "chart.bar.xaxis",
                        description: Text("No items with that date match."))
                }
            }
            if let undated = discover.timeline?.undated, undated > 0 {
                Text(undated == 1 ? "1 item has no such date." : "\(undated) items have no such date.")
                    .font(.caption).foregroundStyle(.secondary).padding(6)
            }
        }
        .onChange(of: discover.timelineDate) { Task { await discover.refreshTimeline() } }
        .onChange(of: discover.timelineGrouping) { Task { await discover.refreshTimeline() } }
    }

    private var busiestPeriod: Int {
        discover.timeline?.periods.map(\.items.count).max() ?? 0
    }
}

/// A period's heading, with a bar for how many items it has beside the busiest.
private struct PeriodHeader: View {
    let period: TimelinePeriod
    let most: Int

    var body: some View {
        HStack {
            Text(period.period).monospacedDigit()
            GeometryReader { geometry in
                Capsule().fill(.tint.opacity(0.5))
                    .frame(
                        width: most > 0
                            ? geometry.size.width * CGFloat(period.items.count) / CGFloat(most) : 0,
                        height: 5)
                    .frame(maxHeight: .infinity)
            }
            Text(period.items.count, format: .number).monospacedDigit()
        }
    }
}

// MARK: - The archive map

/// What the treemap's areas show.
enum MapMeasure: String, CaseIterable {
    case size
    case items

    var title: String { self == .size ? "Size" : "Items" }
}

private struct ArchiveMapPage: View {
    let discover: DiscoverModel
    let window: ArchiveWindowModel
    @State private var measure = MapMeasure.size
    /// The collection the map has zoomed into, or `nil` for the whole archive.
    @State private var inside: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Button("Archive") { inside = nil }.buttonStyle(.link).disabled(inside == nil)
                ForEach(breadcrumbs, id: \.path) { region in
                    Image(systemName: "chevron.right").foregroundStyle(.tertiary)
                    Button(region.title) { inside = region.path }.buttonStyle(.link)
                }
                Spacer()
                if let map = discover.map {
                    Text(summary(map)).font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(8)
            if let map = discover.map {
                TreemapView(
                    regions: map.regions(in: inside), measure: effectiveMeasure(map),
                    hasInside: { path in !map.regions(in: path).isEmpty },
                    zoom: { inside = $0 },
                    open: { window.screen = .collection($0) })
                .padding([.horizontal, .bottom], 8)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .toolbar {
            Picker("Measure", selection: $measure) {
                ForEach(MapMeasure.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .help("Size the collections by how much space they take, or by how many items they hold")
        }
    }

    /// Items when no card records a size, since the map would be empty by size.
    private func effectiveMeasure(_ map: ArchiveMap) -> MapMeasure {
        map.totalSize == 0 ? .items : measure
    }

    private var breadcrumbs: [MapRegion] {
        guard let map = discover.map, var path = inside else { return [] }
        var trail: [MapRegion] = []
        while let region = map.regions.first(where: { $0.path == path }) {
            trail.insert(region, at: 0)
            guard let parent = region.parent else { break }
            path = parent
        }
        return trail
    }

    private func summary(_ map: ArchiveMap) -> String {
        let size = ByteCountFormatter.string(fromByteCount: Int64(map.totalSize), countStyle: .file)
        var text = "\(map.totalItems) items · \(size)"
        if map.withoutSize > 0 { text += " · \(map.withoutSize) without a size" }
        return text
    }
}

/// The collections as a treemap: click to zoom into one with collections inside, double-click
/// to open it in Browse.
private struct TreemapView: View {
    let regions: [MapRegion]
    let measure: MapMeasure
    let hasInside: (String) -> Bool
    let zoom: (String) -> Void
    let open: (String) -> Void

    var body: some View {
        GeometryReader { geometry in
            let tiles = Treemap.layout(
                regions.map { (id: $0.path, value: value(of: $0)) },
                in: CGRect(origin: .zero, size: geometry.size))
            ZStack(alignment: .topLeading) {
                ForEach(tiles, id: \.id) { tile in
                    if let region = regions.first(where: { $0.path == tile.id }) {
                        tileView(region, color: color(for: region.path))
                            .frame(width: max(tile.rect.width - 2, 0), height: max(tile.rect.height - 2, 0))
                            .offset(x: tile.rect.minX + 1, y: tile.rect.minY + 1)
                    }
                }
            }
        }
        .overlay {
            if regions.allSatisfy({ value(of: $0) <= 0 }) {
                ContentUnavailableView("Nothing to Map", systemImage: "square.grid.3x3.square")
            }
        }
    }

    private func tileView(_ region: MapRegion, color: Color) -> some View {
        RoundedRectangle(cornerRadius: 4)
            .fill(color.gradient)
            .overlay(alignment: .topLeading) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(region.title).font(.callout.bold())
                    Text(detail(region)).font(.caption)
                }
                .foregroundStyle(.white)
                .padding(6)
            }
            .clipped()
            .contentShape(.rect)
            .help("\(region.path): \(detail(region))")
            .onTapGesture(count: 2) { open(region.path) }
            .onTapGesture { if hasInside(region.path) { zoom(region.path) } else { open(region.path) } }
            .contextMenu {
                Button("Show in Browse") { open(region.path) }
                if hasInside(region.path) { Button("Zoom In") { zoom(region.path) } }
            }
    }

    private func value(of region: MapRegion) -> Double {
        measure == .size ? Double(region.totalSize) : Double(region.totalItems)
    }

    private func detail(_ region: MapRegion) -> String {
        let size = ByteCountFormatter.string(fromByteCount: Int64(region.totalSize), countStyle: .file)
        return region.totalItems == 1 ? "1 item · \(size)" : "\(region.totalItems) items · \(size)"
    }

    /// A steady color for each collection, from its path.
    private func color(for path: String) -> Color {
        let hash = path.unicodeScalars.reduce(UInt32(5381)) { ($0 &* 33) &+ $1.value }
        return Color(hue: Double(hash % 360) / 360, saturation: 0.45, brightness: 0.62)
    }
}

// MARK: - Activity

private struct ActivityPage: View {
    let discover: DiscoverModel

    var body: some View {
        ScrollView {
            if let activity = discover.activity {
                VStack(alignment: .leading, spacing: 12) {
                    Text(totals(activity)).foregroundStyle(.secondary)
                    ActivityHeatmap(activity: activity)
                    HStack(spacing: 4) {
                        Text("Less").font(.caption).foregroundStyle(.secondary)
                        ForEach(0...4, id: \.self) { level in
                            ActivityHeatmap.cell(level: UInt8(level))
                        }
                        Text("More").font(.caption).foregroundStyle(.secondary)
                    }
                }
                .padding()
            } else {
                ProgressView().padding()
            }
        }
    }

    private func totals(_ activity: ArchiveActivity) -> String {
        let filed = activity.days.reduce(0) { $0 + Int($1.filed) }
        let sessions = activity.days.reduce(0) { $0 + Int($1.sessions) }
        return
            "From \(activity.from) to \(activity.to): \(filed) filed, \(sessions) work sessions, on \(activity.days.count) days."
    }
}

/// Activity as a heatmap: a column a week, a row a weekday, shaded by how busy each day was.
struct ActivityHeatmap: View {
    let activity: ArchiveActivity

    static let side: CGFloat = 11

    static func cell(level: UInt8) -> some View {
        RoundedRectangle(cornerRadius: 2)
            .fill(level == 0 ? AnyShapeStyle(.quaternary) : AnyShapeStyle(.tint.opacity(0.2 + 0.2 * Double(level))))
            .frame(width: side, height: side)
    }

    var body: some View {
        let weeks = HeatmapWeeks(activity: activity)
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 3) {
                ForEach(Array(weeks.columns.enumerated()), id: \.offset) { _, week in
                    VStack(spacing: 3) {
                        ForEach(Array(week.enumerated()), id: \.offset) { _, day in
                            if let day {
                                Self.cell(level: day.level)
                                    .help(describe(day))
                            } else {
                                Color.clear.frame(width: Self.side, height: Self.side)
                            }
                        }
                    }
                }
            }
        }
    }

    private func describe(_ day: ActivityDay) -> String {
        var parts: [String] = []
        if day.filed > 0 { parts.append(day.filed == 1 ? "1 filed" : "\(day.filed) filed") }
        if day.sessions > 0 {
            parts.append(day.sessions == 1 ? "1 work session" : "\(day.sessions) work sessions")
        }
        return "\(day.date): " + (parts.isEmpty ? "nothing" : parts.joined(separator: ", "))
    }
}

/// The days from the activity's first to its last, in weeks starting on Sunday, with the
/// days before the first and after the last left empty.
struct HeatmapWeeks {
    /// Each week's seven days; `nil` outside the span.
    let columns: [[ActivityDay?]]

    init(activity: ArchiveActivity, calendar: Calendar = .init(identifier: .gregorian)) {
        var calendar = calendar
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let parse = { (text: String) -> Date? in
            try? Date(text + "T00:00:00Z", strategy: .iso8601)
        }
        guard let from = parse(activity.from), let to = parse(activity.to), from <= to else {
            columns = []
            return
        }
        let byDate = Dictionary(activity.days.map { ($0.date, $0) }) { a, _ in a }
        var columns: [[ActivityDay?]] = []
        var week: [ActivityDay?] = Array(
            repeating: nil, count: calendar.component(.weekday, from: from) - 1)
        var day = from
        while day <= to {
            let text = DiscoverModel.dayText(day, in: calendar.timeZone)
            week.append(byDate[text] ?? ActivityDay(date: text, filed: 0, sessions: 0, level: 0))
            if week.count == 7 {
                columns.append(week)
                week = []
            }
            day = calendar.date(byAdding: .day, value: 1, to: day)!
        }
        if !week.isEmpty {
            columns.append(week + Array(repeating: nil, count: 7 - week.count))
        }
        self.columns = columns
    }
}

extension DiscoverModel {
    /// `date` as `YYYY-MM-DD` in `zone`.
    nonisolated static func dayText(_ date: Date, in zone: TimeZone) -> String {
        var style = Date.ISO8601FormatStyle(timeZone: zone)
        style = style.year().month().day().dateSeparator(.dash)
        return date.formatted(style)
    }
}
