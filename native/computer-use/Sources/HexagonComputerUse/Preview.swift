import AppKit
import Foundation
import ScreenCaptureKit
import PeekabooAutomationKit

// Owner extension 14, 2026-10-02: local owner preview is a separate, bounded
// capture lane. It never creates a model observation, AX tree, trace or lease.
struct PreviewRequest: Decodable, Sendable {
    let snapshot_id: String
    let window_id: UInt32
    let process_id: Int32
    let focus: Bool
}
struct PreviewReply: Encodable, Sendable {
    var protocol_version = 1
    var ok: Bool
    var error: String? = nil
    var data_url: String? = nil
    var width: Int? = nil
    var height: Int? = nil
    var window_id: UInt32? = nil
    var process_id: Int32? = nil
}
final class PreviewMailbox: @unchecked Sendable {
    static let shared = PreviewMailbox()
    private let lock = NSLock()
    private var serial: UInt64 = 0
    private var active: UInt64?
    private var cancelled = false
    private var result: String?
    private var last = Date.distantPast
    private var task: Task<Void, Never>?
    func start(_ request: PreviewRequest, operation: @escaping @MainActor @Sendable (PreviewRequest, UInt64) async -> PreviewReply = { request, id in await PreviewExecutor.shared.execute(request, id: id) }) -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        // Issue14 review: the 2fps frame budget must not throttle an explicit
        // owner focus click after the frame has finished. Both still serialize.
        guard active == nil, request.focus || Date().timeIntervalSince(last) >= 0.5 else { return 0 }
        serial &+= 1; if serial == 0 { serial = 1 }
        let id = serial
        active = id; cancelled = false; result = nil
        if !request.focus { last = Date() }
        task = Task { @MainActor in self.finish(id, await operation(request, id)) }
        return id
    }
    func checkpoint(_ id: UInt64) throws {
        lock.lock(); defer { lock.unlock() }
        guard active == id, !cancelled else { throw CancellationError() }
    }
    func stop() {
        lock.lock(); defer { lock.unlock() }
        cancelled = true; task?.cancel()
        // Finished images are discarded immediately; in-flight captures retain
        // their lane until completion, so hide/show cannot accumulate work.
        if result != nil { result = nil; active = nil }
    }
    func finish(_ id: UInt64, _ reply: PreviewReply) {
        lock.lock(); defer { lock.unlock() }
        guard active == id else { return }
        task = nil
        if cancelled { active = nil; result = nil; return }
        result = (try? JSONEncoder().encode(reply)).flatMap { String(data: $0, encoding: .utf8) }
    }
    func poll(_ id: UInt64) -> String? {
        lock.lock(); defer { lock.unlock() }
        guard active == id, let result else { return nil }
        active = nil; self.result = nil
        return result
    }
}

@MainActor final class PreviewExecutor {
    static let shared = PreviewExecutor()
    private var receipt: (String, WindowMutationIdentity)?
    func remember(_ snapshot: String, identity: WindowMutationIdentity) { receipt = (snapshot, identity) }
    static func matches(_ request: PreviewRequest, snapshot: String, identity: WindowMutationIdentity) -> Bool {
        request.snapshot_id == snapshot && request.window_id > 0 && request.process_id > 0
            && identity.windowID == Int(request.window_id) && identity.ownerProcessIdentifier == request.process_id
            && identity.ownerProcessStartIdentity > 0
    }
    func execute(_ request: PreviewRequest, id: UInt64) async -> PreviewReply {
        do {
            guard let (snapshot, identity) = receipt,
                  Self.matches(request, snapshot: snapshot, identity: identity),
                  SystemIdentityResolver.validateWindowMutationIdentity(identity),
                  CGPreflightScreenCaptureAccess() else { throw ActionError("preview_target_unavailable") }
            try PreviewMailbox.shared.checkpoint(id)
            if request.focus {
                guard let app = NSRunningApplication(processIdentifier: request.process_id), !app.isTerminated,
                      SystemIdentityResolver.validateWindowMutationIdentity(identity) else { throw ActionError("preview_target_unavailable") }
                try PreviewMailbox.shared.checkpoint(id)
                guard app.activate(options: []) else { throw ActionError("preview_focus_failed") }
                return PreviewReply(ok: true, window_id: request.window_id, process_id: request.process_id)
            }
            // ScreenCaptureKit captures this window only, without AX detection
            // or Peekaboo's application inventory/code-signature scan.
            let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
            try PreviewMailbox.shared.checkpoint(id)
            guard let window = content.windows.first(where: { $0.windowID == request.window_id && $0.owningApplication?.processID == request.process_id }),
                  SystemIdentityResolver.validateWindowMutationIdentity(identity) else { throw ActionError("preview_target_closed") }
            let size = Self.boundedSize(width: window.frame.width, height: window.frame.height)
            let config = SCStreamConfiguration()
            config.width = size.0; config.height = size.1; config.showsCursor = false
            let image = try await SCScreenshotManager.captureImage(contentFilter: SCContentFilter(desktopIndependentWindow: window), configuration: config)
            try PreviewMailbox.shared.checkpoint(id)
            guard SystemIdentityResolver.validateWindowMutationIdentity(identity), CGPreflightScreenCaptureAccess(),
                  let jpeg = NSBitmapImageRep(cgImage: image).representation(using: .jpeg, properties: [.compressionFactor: 0.65]),
                  jpeg.count <= 1_048_576 else { throw ActionError("preview_invalid_capture") }
            return PreviewReply(ok: true, data_url: "data:image/jpeg;base64," + jpeg.base64EncodedString(), width: image.width, height: image.height, window_id: request.window_id, process_id: request.process_id)
        } catch { return PreviewReply(ok: false, error: error.localizedDescription) }
    }
    static func boundedSize(width: Double, height: Double) -> (Int, Int) {
        guard width.isFinite, height.isFinite, width > 0, height > 0 else { return (1, 1) }
        let scale = min(1, 960 / max(width, height))
        return (max(1, Int(width * scale)), max(1, Int(height * scale)))
    }
}
@_cdecl("hexagon_preview_start_v1")
public func hexagonPreviewStart(_ json: UnsafePointer<CChar>?) -> UInt64 {
    guard let json else { return 0 }
    let count = strnlen(json, 4097)
    guard count <= 4096, let request = try? JSONDecoder().decode(PreviewRequest.self, from: Data(bytes: json, count: count)),
          request.window_id > 0, request.process_id > 0, !request.snapshot_id.isEmpty, request.snapshot_id.count <= 256 else { return 0 }
    return PreviewMailbox.shared.start(request)
}
@_cdecl("hexagon_preview_poll_v1")
public func hexagonPreviewPoll(_ id: UInt64) -> UnsafeMutablePointer<CChar>? { PreviewMailbox.shared.poll(id).flatMap { strdup($0) } }
@_cdecl("hexagon_preview_stop_v1")
public func hexagonPreviewStop() { PreviewMailbox.shared.stop() }
