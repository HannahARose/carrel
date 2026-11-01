// The reminders due now, for Status (docs/design/reminders.md). The agent posts them as
// notifications; Status lists them whether or not they were posted.

extension ArchiveModel {
    /// The reminders due now, as the archive's `reminders` setting turns them on.
    public func reminders() async throws -> [Reminder] {
        try await call { try $0.reminders() }
    }
}
