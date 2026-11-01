// Status's reminders (docs/design/reminders.md): work sessions whose entries wait, checkouts
// due back, and the week's sum of what's waiting. Showing one goes to its item on Active.

import CarrelKit
import SwiftUI

struct StatusRemindersSection: View {
    let window: ArchiveWindowModel

    @State private var reminders: [Reminder] = []

    var body: some View {
        Group {
            if !reminders.isEmpty {
                Section("Reminders") {
                    ForEach(reminders, id: \.key) { reminder in
                        LabeledContent {
                            if let item = reminder.item {
                                Button("Show") {
                                    window.screen = .active
                                    window.selectedItemID = item
                                }
                            }
                        } label: {
                            Text(reminder.title)
                            Text(reminder.text)
                        }
                    }
                }
            }
        }
        .task(id: "\(window.handle != nil)-\(window.summary.itemCount)") {
            reminders = (try? await window.archive?.reminders()) ?? []
        }
    }
}
