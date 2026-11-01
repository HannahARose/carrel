// The XPC service the agent offers the extensions (docs/design/macos-integration.md). The agent,
// a login item, has a Mach service named for its bundle identifier, which begins with the app
// group's, so sandboxed extensions in the group may look it up. Requests and replies cross it as
// JSON, so the protocol never changes when a request is added.

#if os(macOS)
import Foundation

/// The one call the agent's service answers: a JSON `AgentRequest` in, a JSON `AgentReply` out.
@objc public protocol CarrelAgentXPC {
    func handle(_ request: Data, reply: @escaping @Sendable (Data) -> Void)
}

/// The agent's side: accepts connections and answers each request with `handler`.
public final class AgentXPCListener: NSObject, NSXPCListenerDelegate, @unchecked Sendable {
    private let listener: NSXPCListener
    private let handler: AgentRequestHandler

    /// Listens on `listener`, the agent's Mach service unless given, as tests give an
    /// anonymous one. Call `resume()` to start.
    public init(handler: AgentRequestHandler, listener: NSXPCListener? = nil) {
        self.handler = handler
        self.listener = listener ?? NSXPCListener(machServiceName: AgentService.machServiceName)
        super.init()
        self.listener.delegate = self
    }

    /// For connecting to an anonymous listener.
    public var endpoint: NSXPCListenerEndpoint { listener.endpoint }

    public func resume() { listener.resume() }

    public func invalidate() { listener.invalidate() }

    public func listener(
        _ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection
    ) -> Bool {
        connection.exportedInterface = NSXPCInterface(with: CarrelAgentXPC.self)
        connection.exportedObject = Exported(handler: handler)
        connection.resume()
        return true
    }

    private final class Exported: NSObject, CarrelAgentXPC, @unchecked Sendable {
        let handler: AgentRequestHandler

        init(handler: AgentRequestHandler) {
            self.handler = handler
        }

        func handle(_ request: Data, reply: @escaping @Sendable (Data) -> Void) {
            guard let request = try? JSONDecoder().decode(AgentRequest.self, from: request) else {
                reply(
                    AgentService.encode(
                        AgentReply.refused(
                            "Carrel Agent didn’t understand the request. Update Carrel, then "
                                + "try again.")))
                return
            }
            let handler = handler
            Task { @MainActor in
                reply(AgentService.encode(await handler.handle(request)))
            }
        }
    }
}

/// The extensions' side: sends requests to the agent and waits for its replies.
public final class AgentClient: @unchecked Sendable {
    private let connection: NSXPCConnection

    /// A client of the agent's Mach service, or the one named.
    public convenience init(machServiceName: String = AgentService.machServiceName) {
        self.init(connection: NSXPCConnection(machServiceName: machServiceName))
    }

    /// A client of the listener at `endpoint`, as in tests.
    public convenience init(endpoint: NSXPCListenerEndpoint) {
        self.init(connection: NSXPCConnection(listenerEndpoint: endpoint))
    }

    private init(connection: NSXPCConnection) {
        self.connection = connection
        connection.remoteObjectInterface = NSXPCInterface(with: CarrelAgentXPC.self)
        connection.resume()
    }

    deinit {
        connection.invalidate()
    }

    /// Sends `request` and waits for the reply. When the agent can't be reached, the reply
    /// is a refusal saying it isn't running.
    public func send(_ request: AgentRequest) async -> AgentReply {
        let data = AgentService.encode(request)
        return await withCheckedContinuation { continuation in
            let once = Once(continuation)
            let proxy = connection.remoteObjectProxyWithErrorHandler { _ in
                once.resume(.notRunning)
            }
            guard let agent = proxy as? CarrelAgentXPC else {
                once.resume(.notRunning)
                return
            }
            agent.handle(data) { reply in
                once.resume(
                    (try? JSONDecoder().decode(AgentReply.self, from: reply))
                        ?? AgentReply(.failed, "Carrel Agent’s reply couldn’t be read."))
            }
        }
    }

    /// Resumes a continuation once, whichever of the reply and the error handler comes first.
    private final class Once: @unchecked Sendable {
        private let lock = NSLock()
        private var continuation: CheckedContinuation<AgentReply, Never>?

        init(_ continuation: CheckedContinuation<AgentReply, Never>) {
            self.continuation = continuation
        }

        func resume(_ reply: AgentReply) {
            lock.withLock {
                continuation?.resume(returning: reply)
                continuation = nil
            }
        }
    }
}
#endif
