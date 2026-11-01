// The agent as a login item: Settings' switch registers Carrel Agent, bundled in the app at
// Contents/Library/LoginItems, with SMAppService, or unregisters it.

import CarrelKit
import Observation
import ServiceManagement

/// What registers and unregisters the agent; tests use one of their own.
protocol LoginService {
    var status: SMAppService.Status { get }
    func register() throws
    func unregister() throws
}

extension SMAppService: LoginService {}

@Observable
final class AgentLoginItem {
    /// The bundled agent's identifier, which is also its Mach service: the Debug app's agent
    /// is its own, never the installed app's.
    static let identifier = AgentService.machServiceName

    private let service: LoginService
    private(set) var status: SMAppService.Status
    /// Why the last change didn't take.
    private(set) var problem: String?

    init(service: LoginService = SMAppService.loginItem(identifier: identifier)) {
        self.service = service
        status = service.status
    }

    /// Whether the agent runs at login, including while it waits for approval in System
    /// Settings.
    var isOn: Bool { status == .enabled || status == .requiresApproval }

    /// Whether System Settings has to approve it before it runs.
    var needsApproval: Bool { status == .requiresApproval }

    /// Registers the agent, which starts it now and at every login, or unregisters it, which
    /// stops it; the app then drafts on demand, as the CLI does.
    func set(_ on: Bool) {
        do {
            if on { try service.register() } else { try service.unregister() }
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
        status = service.status
    }

    /// Reads the status again, as after the owner approves it in System Settings.
    func refresh() {
        status = service.status
    }
}
