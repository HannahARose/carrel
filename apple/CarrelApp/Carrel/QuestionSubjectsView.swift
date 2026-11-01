// What a question is about, at the top of its inspector (#446): one item's preview, as Browse
// shows it, or a strip of thumbnails for a group, each with Open. A restricted item's preview
// waits until it's asked for, as in Browse.

import AppKit
import CarrelKit
import SwiftUI

extension QuestionsModel {
    /// What the question `id` is about: the filed items it names, then the proposals it holds
    /// up, each with its file when it's on this Mac.
    func subjects(of id: Question.ID) async -> [QuestionSubject] {
        (try? await archive.call { try $0.questionSubjects(id: id) }) ?? []
    }
}

extension QuestionSubject {
    /// Its file, when it's on this Mac.
    var url: URL? { file.map { URL(filePath: $0) } }
}

struct QuestionSubjectsSection: View {
    let question: Question
    let questions: QuestionsModel
    /// Browse's previews, for filed items.
    let previews: PreviewStore?
    /// The filed item with an identifier.
    let item: (String) -> ItemSummary?

    @State private var subjects: [QuestionSubject] = []
    /// The restricted proposals whose previews were asked for.
    @State private var revealed: Set<String> = []

    var body: some View {
        Group {
            if !subjects.isEmpty {
                Section("What It’s About") {
                    if subjects.count == 1, let subject = subjects.first {
                        preview(of: subject)
                    } else {
                        ScrollView(.horizontal) {
                            HStack(spacing: 10) {
                                ForEach(subjects, id: \.id) { subject in
                                    SubjectThumbnail(
                                        subject: subject, previews: previews,
                                        shown: !subject.restricted
                                            || revealed.contains(subject.id))
                                }
                            }
                            .padding(.vertical, 4)
                        }
                    }
                    ForEach(subjects, id: \.id) { subject in
                        SubjectRow(subject: subject)
                    }
                }
            }
        }
        .task(id: question.id) {
            revealed = []
            subjects = await questions.subjects(of: question.id)
        }
    }

    /// One item's preview: Browse's for a filed item, Quick Look's for a proposal's file.
    @ViewBuilder
    private func preview(of subject: QuestionSubject) -> some View {
        if !subject.proposal, let previews, let summary = item(subject.id) {
            ItemPreviewView(item: summary, store: previews)
        } else if let url = subject.url {
            if subject.restricted, !revealed.contains(subject.id) {
                VStack(spacing: 8) {
                    Image(systemName: "lock").font(.title2).foregroundStyle(.secondary)
                    Text("This item is restricted.").foregroundStyle(.secondary)
                    Button("Show Preview") { revealed.insert(subject.id) }
                }
                .frame(maxWidth: .infinity, minHeight: 120)
            } else {
                QuickLookPreview(url: url).frame(height: 280)
            }
        }
    }
}

/// An item's title, where it is, and Open.
private struct SubjectRow: View {
    let subject: QuestionSubject

    var body: some View {
        HStack {
            Label {
                VStack(alignment: .leading, spacing: 2) {
                    Text(subject.title + (subject.proposal ? " (waiting)" : ""))
                    Text(subject.file ?? "Not on this Mac")
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
            } icon: {
                Image(systemName: subject.restricted ? "lock" : subject.kind.symbol)
            }
            Spacer()
            Button("Open") { if let url = subject.url { NSWorkspace.shared.open(url) } }
                .disabled(subject.url == nil)
        }
        .textSelection(.enabled)
    }
}

/// A small picture of one of a group's items: its thumbnail, or its kind's icon while there's
/// none, and for a restricted one until it's asked for.
private struct SubjectThumbnail: View {
    let subject: QuestionSubject
    let previews: PreviewStore?
    let shown: Bool

    @State private var picture: CGImage?

    var body: some View {
        VStack(spacing: 4) {
            Group {
                if shown,
                    let image = picture
                        ?? (subject.proposal ? nil : previews?.picture(for: subject.id))
                {
                    Image(decorative: image, scale: 2).resizable().scaledToFit()
                } else {
                    Image(systemName: subject.restricted ? "lock" : subject.kind.symbol)
                        .font(.title2).foregroundStyle(.secondary)
                }
            }
            .frame(width: 88, height: 88)
            Text(subject.title).font(.caption).lineLimit(1).frame(width: 88)
        }
        .help(subject.file ?? subject.title)
        .task(id: subject.id) {
            guard shown else { return }
            if !subject.proposal, let previews {
                await previews.loadThumbnail(subject.id)
            } else if let url = subject.url {
                picture = await QuickLookThumbnails().thumbnail(of: url, pixels: 176)
            }
        }
    }
}
