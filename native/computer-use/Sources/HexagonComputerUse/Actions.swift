import AppKit
import AXorcist
@preconcurrency import ApplicationServices
import Foundation
import Darwin
import PeekabooAutomationKit
import PeekabooFoundation

// Ticket 07, owner decision 2026-10-01: this executor never plans, approves,
// launches a shell, discovers a Bridge, or pauses on human input. Core owns those
// policies. An uncertain native result costs an observation, never a blind retry.
struct ActionRequest: Decodable, Sendable {
    let op: String
    let window_id: UInt32?
    let snapshot_id: String?
    let x: Double?
    let y: Double?
    let text: String?
    let keys: String?
    let direction: String?
    let amount: Int?
    let element_id: String?
    let click_type: String?
    let app_id: String?
    let display_index: Int?
    let to_x: Double?
    let to_y: Double?
    let duration_ms: Int?

    func validate() throws {
        guard ["list_apps", "activate", "observe", "observe_screen", "click", "type", "key", "scroll", "drag"].contains(op) else { throw ActionError("invalid_operation") }
        if window_id == 0 { throw ActionError("invalid_window") }
        if !["list_apps", "activate", "observe", "observe_screen"].contains(op), snapshot_id?.isEmpty != false { throw ActionError("snapshot_required") }
        if let display_index, !(0...15).contains(display_index) { throw ActionError("invalid_display") }
        if op == "activate", app_id?.isEmpty != false { throw ActionError("app_receipt_required") }
        if op == "drag" {
            guard let x, let y, let to_x, let to_y, [x, y, to_x, to_y].allSatisfy({ $0.isFinite && $0 >= 0 }), (100...2000).contains(duration_ms ?? 500) else { throw ActionError("invalid_drag") }
        }
        if op == "click" {
            guard let x, let y, x.isFinite, y.isFinite, x >= 0, y >= 0,
                  ["single", "double", "right"].contains(click_type ?? "single") else { throw ActionError("invalid_click") }
        }
        if op == "type", text == nil || text!.utf8.count > 16_384 { throw ActionError("invalid_text") }
        if op == "key" {
            guard ApprovedKeyboard.validChord(keys ?? "") else { throw ActionError("invalid_keys") }
        }
        if op == "scroll" {
            guard ["up", "down", "left", "right"].contains(direction ?? ""),
                  (1...20).contains(amount ?? 0), element_id?.isEmpty == false else { throw ActionError("invalid_scroll") }
        }
    }
}

struct ActionError: Error, LocalizedError {
    let code: String
    init(_ code: String) { self.code = code }
    var errorDescription: String? { code }
}

struct NativeRect: Codable, Sendable {
    let x: Double; let y: Double; let width: Double; let height: Double
    init(_ r: CGRect) { x = r.minX; y = r.minY; width = r.width; height = r.height }
}
struct NativeElement: Encodable, Sendable {
    let id: String; let role: String; let label: String?; let bounds: NativeRect; let enabled: Bool
}
struct NativeObservation: Encodable, Sendable {
    let snapshot_id: String
    var scope = "window"
    let window_id: Int?
    let process_id: Int32?
    let bundle_id: String?
    let title: String
    let bounds: NativeRect
    let image_width: Int
    let image_height: Int
    let mime_type: String
    let image_base64: String
    let elements: [NativeElement]
    let accessibility_warning: String?
}
struct NativeWindow: Encodable, Sendable {
    let window_id: UInt32; let title: String; let bounds: NativeRect
}
struct NativeApplication: Encodable, Sendable {
    let app_id: String; let process_id: Int32; let bundle_id: String?; let name: String; let windows: [NativeWindow]
}
struct NativeReply: Encodable, Sendable {
    let protocol_version = 1
    let ok: Bool
    var result: NativeObservation? = nil
    var error: String? = nil
    var outcome_unknown = false
    var cancelled = false
    var outcome: DesktopActionOutcome? = nil
    var applications: [NativeApplication]? = nil
}

// FFI calls can arrive off the main thread. This lock protects only the mailbox;
// never hold it over AppKit, AX, await, or a foreign callback. Cancellation does
// NOT release the lane: an uncooperative AX call must return before a new start.
final class ActionMailbox: @unchecked Sendable {
    static let shared = ActionMailbox()
    private let lock = NSLock()
    private var serial: UInt64 = 0
    private var active: UInt64?
    private var cancelled = false
    private var dispatchStarted = false
    private var result: String?
    private var task: Task<Void, Never>?

    func start(_ request: ActionRequest, operation: @escaping @MainActor @Sendable (ActionRequest, UInt64) async -> NativeReply = { request, id in await ActionExecutor.shared.execute(request, id: id) }) -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        guard active == nil else { return 0 }
        serial &+= 1; if serial == 0 { serial = 1 }
        let id = serial
        active = id; cancelled = false; dispatchStarted = false; result = nil
        task = Task { @MainActor in
            let reply = await operation(request, id)
            self.finish(id, reply)
        }
        return id
    }
    func checkpoint(_ id: UInt64, dispatch: Bool = false) throws {
        lock.lock(); defer { lock.unlock() }
        guard active == id, !cancelled else { throw CancellationError() }
        if dispatch { dispatchStarted = true }
    }
    func cancel(_ id: UInt64) {
        lock.lock(); defer { lock.unlock() }
        guard active == id else { return }
        cancelled = true; task?.cancel()
    }
    func finish(_ id: UInt64, _ value: NativeReply) {
        lock.lock(); defer { lock.unlock() }
        guard active == id else { return }
        var reply = value
        if cancelled {
            reply = NativeReply(ok: false, error: "cancelled", outcome_unknown: dispatchStarted, cancelled: true, outcome: value.outcome)
        }
        let data = try? JSONEncoder().encode(reply)
        result = data.flatMap { String(data: $0, encoding: .utf8) } ?? "{\"protocol_version\":1,\"ok\":false,\"error\":\"encoding_failed\",\"outcome_unknown\":true,\"cancelled\":false}"
        task = nil
    }
    func poll(_ id: UInt64) -> String? {
        lock.lock(); defer { lock.unlock() }
        guard active == id, let result else { return nil }
        active = nil; self.result = nil
        return result
    }
}

@MainActor
final class ActionExecutor {
    static let shared = ActionExecutor()
    private let snapshots = SnapshotManager()
    private let applications = ApplicationService()
    private var appReceipts: [String: ApplicationProcessIdentity] = [:]
    private var appDiscoveryTime = Date.distantPast
    private struct ScreenReceipt {
        let id: String; let displayID: CGDirectDisplayID; let bounds: CGRect; let imageSize: CGSize; let created: Date
        let windows: [SystemWindowIdentity]
    }
    private var screenReceipt: ScreenReceipt?
    private lazy var automation = UIAutomationService(snapshotManager: snapshots)
    // Live acceptance 2026-10-01: exact-window click overloads are background
    // AX presses, which cannot hit many Chrome controls. After exact focus and
    // fresh occlusion checks, the approved coordinate action uses synthesis.
    private lazy var foregroundPointer = UIAutomationService(
        snapshotManager: snapshots, inputPolicy: UIInputPolicy(click: .synthOnly))
    private let capture = ScreenCaptureService(loggingService: LoggingService())
    private struct Receipt {
        let id: String; let identity: WindowMutationIdentity; let bounds: CGRect
        let imageSize: CGSize; let created: Date; let elementIDs: Set<String>; let focusedElement: FocusedElementIdentity?
    }
    private var receipt: Receipt?

    func execute(_ request: ActionRequest, id: UInt64) async -> NativeReply {
        var dispatched = false
        var focusDispatched = false
        var focusRestored = false
        var focusDiagnostic = ""
        do {
            try request.validate()
            try ActionMailbox.shared.checkpoint(id)
            guard hexagonComputerPermissions() & 7 == 7 else { throw ActionError("system_permission_required") }
            try Self.checkSession()
            if request.op == "list_apps" {
                return NativeReply(ok: true, applications: try await discoverApps(id: id))
            }
            if request.op == "activate" {
                guard let target = appReceipts[request.app_id!], Date().timeIntervalSince(appDiscoveryTime) < 300,
                      SystemIdentityResolver.processStartIdentity(target.processIdentifier) == target.processStartIdentity else { throw ActionError("app_receipt_stale") }
                guard !Self.protectedApplication(pid: target.processIdentifier) else { throw ActionError("protected_application") }
                await clearReceipt()
                try Self.checkSession()
                try ActionMailbox.shared.checkpoint(id, dispatch: true)
                dispatched = true
                let outcome = try await applications.activateApplicationTargetedResult(request: ApplicationActivationRequest(identifier: "PID:\(target.processIdentifier)", expectedIdentity: target)).outcome
                return NativeReply(ok: true, outcome_unknown: outcome?.isConfirmed != true, outcome: outcome)
            }
            if request.op == "observe_screen" {
                let observation = try await observeScreen(request, id: id)
                return NativeReply(ok: true, result: observation)
            }
            if request.snapshot_id == screenReceipt?.id, let screen = screenReceipt {
                screenReceipt = nil
                guard Date().timeIntervalSince(screen.created) <= 60, CGDisplayBounds(screen.displayID) == screen.bounds,
                      CGDisplayIsActive(screen.displayID) != 0 else { throw ActionError("snapshot_stale") }
                guard request.op == "click" || request.op == "drag" else { throw ActionError("exact_window_observation_required") }
                let from = try Self.mapPoint(x: request.x!, y: request.y!, imageSize: screen.imageSize, bounds: screen.bounds)
                let source = try Self.validateScreenTarget(at: from, observed: screen.windows)
                let to: CGPoint? = request.op == "drag" ? try Self.mapPoint(x: request.to_x!, y: request.to_y!, imageSize: screen.imageSize, bounds: screen.bounds) : nil
                if let to {
                    _ = try Self.validateScreenTarget(at: to, observed: screen.windows)
                    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == source.ownerProcessIdentifier else { throw ActionError("activate_source_before_drag") }
                }
                try Self.checkSession()
                try ActionMailbox.shared.checkpoint(id, dispatch: to == nil)
                dispatched = to == nil
                let outcome: DesktopActionOutcome?
                if let to {
                    outcome = try await ApprovedDrag.execute(from: from, to: to, duration: request.duration_ms ?? 500, validate: { point in
                        try Self.checkSession()
                        try ActionMailbox.shared.checkpoint(id)
                        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == source.ownerProcessIdentifier else { throw ActionError("drag_source_not_frontmost") }
                        _ = try Self.validateScreenTarget(at: from, observed: screen.windows)
                        _ = try Self.validateScreenTarget(at: point, observed: screen.windows)
                    }, onDispatch: {
                        try ActionMailbox.shared.checkpoint(id, dispatch: true)
                        dispatched = true
                    })
                } else {
                    outcome = try await automation.clickWithOutcome(target: .coordinates(from), clickType: ClickType(rawValue: request.click_type ?? "single")!, snapshotId: nil).outcome
                }
                return NativeReply(ok: true, outcome_unknown: outcome?.isConfirmed != true, outcome: outcome)
            }
            if request.op == "observe" {
                let observation = try await observe(request, id: id)
                try ActionMailbox.shared.checkpoint(id)
                return NativeReply(ok: true, result: observation)
            }
            guard let current = receipt, current.id == request.snapshot_id,
                  Date().timeIntervalSince(current.created) <= 60,
                  SystemIdentityResolver.validateWindowMutationIdentity(current.identity) else { throw ActionError("snapshot_stale") }
            // One observation permits one mutation. Even failures require a fresh
            // observation: a timeout must not replay the same click or keystrokes.
            guard !Self.protectedApplication(pid: current.identity.ownerProcessIdentifier) else { throw ActionError("protected_application") }
            receipt = nil
            defer { Task { try? await snapshots.cleanSnapshot(snapshotId: current.id) } }
            let exact = try UIAutomationTarget.ExactWindow(identity: current.identity, bounds: current.bounds)
            var keyboard: ExactWindowKeyboardTarget?
            if request.op == "type" || request.op == "key" {
                let pinned = try await UIAutomationTarget.exactWindow(exact).pinningCurrentFocusedElement(using: automation)
                guard let focus = pinned.exactWindow?.focusedElement else { throw ActionError("focused_target_required: current_receipt_missing") }
                guard let observed = current.focusedElement else { throw ActionError("focused_target_required: observed_receipt_missing") }
                guard focus == observed else {
                    // Live action1250 (2026-10-01): distinguish stale identity
                    // from secure-field refusal without recording field contents.
                    throw ActionError("focused_target_required: identity_changed process=\(focus.processIdentifier == observed.processIdentifier),window=\(focus.windowID == observed.windowID),role=\(focus.role == observed.role),frame=\(focus.frame == observed.frame),title=\(focus.title == observed.title),identifier=\(focus.identifier == observed.identifier)")
                }
                guard focus.role != "AXSecureTextField", try Self.focusIsNotSecure(pid: current.identity.ownerProcessIdentifier) else { throw ActionError("focused_target_required: secure_field") }
                keyboard = ExactWindowKeyboardTarget(windowIdentity: current.identity, windowBounds: current.bounds, focusedElement: focus)
            }
            let point: CGPoint?
            if request.op == "click" || request.op == "drag" {
                point = try Self.mapPoint(x: request.x!, y: request.y!, imageSize: current.imageSize, bounds: current.bounds)
            } else { point = nil }
            var dragTo: CGPoint?
            if request.op == "drag" {
                dragTo = try Self.mapPoint(x: request.to_x!, y: request.to_y!, imageSize: current.imageSize, bounds: current.bounds)
            }
            if request.op == "click" || request.op == "drag" || keyboard != nil {
                // Ticket 07 / live click 1061: owner approval foregrounds Hexagon,
                // and activating Chrome alone may select another Chrome window.
                // Restore the exact observed window, then reject moved/covered
                // targets before synthesizing input. Never retry an unknown click.
                try Self.checkSession()
                try ActionMailbox.shared.checkpoint(id, dispatch: true)
                let focus: DesktopActionOutcome
                do {
                    // QA live action574 / 2026-10-05: bare window activation
                    // timed out when owner approval left another app frontmost.
                    // Use the existing generation-pinned activation service (with
                    // verified AX fallback), then restore the exact window. Never
                    // replace this with an unverified global focus or click retry.
                    focus = try await Self.focusApprovedWindow(focusDispatched: &focusDispatched, activate: {
                        let target = ApplicationProcessIdentity(
                            processIdentifier: current.identity.ownerProcessIdentifier,
                            processStartIdentity: current.identity.ownerProcessStartIdentity)
                        let activation = try await self.applications.activateApplicationTargetedResult(
                            request: ApplicationActivationRequest(
                                identifier: "PID:\(target.processIdentifier)", expectedIdentity: target)).outcome
                        return activation
                    }, focus: {
                        try await FocusManagementService().focusWindowResult(
                            windowID: CGWindowID(current.identity.windowID), expectedIdentity: current.identity)
                    })
                } catch {
                    // Live action 1183: preserve the original failure accounting;
                    // read-only diagnostics must not retry activation or change focus.
                    focusDiagnostic = Self.focusFailureDiagnostic(windowID: CGWindowID(current.identity.windowID),
                        processID: current.identity.ownerProcessIdentifier)
                    throw error
                }
                focusDispatched = focus.dispatchState.mutationDispatched
                dispatched = focusDispatched
                guard focus.isConfirmed else { throw ActionError("exact_window_focus_unconfirmed") }
                // A verified focus change is known preparation, not an unknown
                // click. Subsequent pre-dispatch rejection must say no click ran.
                focusRestored = true
                focusDispatched = false
                dispatched = false
                try ActionMailbox.shared.checkpoint(id)
                guard SystemIdentityResolver.validateWindowMutationIdentity(current.identity),
                      SystemIdentityResolver.windowIdentity(CGWindowID(current.identity.windowID))?.bounds == current.bounds else { throw ActionError("snapshot_stale_after_focus") }
                if let point { try await Self.awaitPointerReady(validate: {
                    try Self.checkSession()
                    try ActionMailbox.shared.checkpoint(id)
                    guard Date().timeIntervalSince(current.created) <= 60,
                          SystemIdentityResolver.validateWindowMutationIdentity(current.identity),
                          SystemIdentityResolver.windowIdentity(CGWindowID(current.identity.windowID))?.bounds == current.bounds else { throw ActionError("snapshot_stale_after_focus") }
                    try Self.validateForegroundPointer(
                    windowID: CGWindowID(current.identity.windowID),
                    processID: current.identity.ownerProcessIdentifier, bounds: current.bounds,
                    points: [point] + (dragTo.map { [$0] } ?? []),
                    frontmostPID: NSWorkspace.shared.frontmostApplication?.processIdentifier,
                    windows: Self.visibleWindows(),
                    dockPassThrough: { window, point in
                    Self.systemOverlayPassesThrough(window, at: point, expectedPID: current.identity.ownerProcessIdentifier, expectedWindowID: CGWindowID(current.identity.windowID))
                    })
                }, wait: { try await Task.sleep(for: .milliseconds(100)) }) }
            }
            if request.op == "scroll", !current.elementIDs.contains(request.element_id!) { throw ActionError("element_not_in_snapshot") }
            try Self.checkSession()
            let immediateDispatch = keyboard == nil && request.op != "drag"
            try ActionMailbox.shared.checkpoint(id, dispatch: immediateDispatch)
            dispatched = immediateDispatch
            let outcome: DesktopActionOutcome?
            switch request.op {
            case "drag":
                outcome = try await ApprovedDrag.execute(from: point!, to: dragTo!, duration: request.duration_ms ?? 500, validate: { position in
                    try Self.checkSession()
                    try ActionMailbox.shared.checkpoint(id)
                    guard SystemIdentityResolver.validateWindowMutationIdentity(current.identity),
                          SystemIdentityResolver.windowIdentity(CGWindowID(current.identity.windowID))?.bounds == current.bounds else { throw ActionError("drag_target_changed") }
                    try Self.validateForegroundPointer(windowID: CGWindowID(current.identity.windowID),
                        processID: current.identity.ownerProcessIdentifier, bounds: current.bounds,
                        points: [position], frontmostPID: NSWorkspace.shared.frontmostApplication?.processIdentifier,
                        windows: Self.visibleWindows(), dockPassThrough: { window, point in
                            Self.systemOverlayPassesThrough(window, at: point, expectedPID: current.identity.ownerProcessIdentifier, expectedWindowID: CGWindowID(current.identity.windowID))
                        })
                }, onDispatch: {
                    try ActionMailbox.shared.checkpoint(id, dispatch: true)
                    dispatched = true
                })
            case "click":
                outcome = try await foregroundPointer.clickWithOutcome(target: .coordinates(point!), clickType: ClickType(rawValue: request.click_type ?? "single")!, snapshotId: nil).outcome
            case "type", "key":
                let expected = keyboard!.focusedElement
                outcome = try await ApprovedKeyboard.execute(text: request.op == "type" ? request.text : nil,
                    keys: request.op == "key" ? request.keys : nil,
                    processID: current.identity.ownerProcessIdentifier, validate: {
                        try Self.checkSession()
                        try ActionMailbox.shared.checkpoint(id)
                        guard SystemIdentityResolver.validateWindowMutationIdentity(current.identity),
                              SystemIdentityResolver.windowIdentity(CGWindowID(current.identity.windowID))?.bounds == current.bounds,
                              NSWorkspace.shared.frontmostApplication?.processIdentifier == current.identity.ownerProcessIdentifier,
                              !Self.protectedApplication(pid: current.identity.ownerProcessIdentifier) else { throw ActionError("keyboard_target_changed") }
                        let pinned = try await UIAutomationTarget.exactWindow(exact).pinningCurrentFocusedElement(using: self.automation)
                        guard pinned.exactWindow?.focusedElement == expected,
                              try Self.focusIsNotSecure(pid: current.identity.ownerProcessIdentifier) else { throw ActionError("keyboard_focus_changed_or_secure") }
                        try ApprovedKeyboard.requireKeyWindow(processID: current.identity.ownerProcessIdentifier,
                            windowID: CGWindowID(current.identity.windowID))
                        // AX awaits can yield: recheck the foreground and window
                        // after the focus read, immediately before the key pair.
                        try Self.checkSession()
                        guard hexagonComputerPermissions() & 7 == 7,
                              NSWorkspace.shared.frontmostApplication?.processIdentifier == current.identity.ownerProcessIdentifier,
                              SystemIdentityResolver.validateWindowMutationIdentity(current.identity),
                              SystemIdentityResolver.windowIdentity(CGWindowID(current.identity.windowID))?.bounds == current.bounds else { throw ActionError("keyboard_target_changed") }
                    }, generationMatches: {
                        SystemIdentityResolver.processStartIdentity(current.identity.ownerProcessIdentifier) == current.identity.ownerProcessStartIdentity
                    }, onDispatch: {
                        try ActionMailbox.shared.checkpoint(id, dispatch: true)
                        dispatched = true
                    })
            case "scroll":
                outcome = try await automation.scrollWithOutcome(ScrollRequest(direction: ScrollDirection(rawValue: request.direction!)!, amount: request.amount!, target: request.element_id!, snapshotId: current.id, expectedWindow: exact, foreground: false)).outcome
            default: throw ActionError("invalid_operation")
            }
            return NativeReply(ok: true, outcome_unknown: outcome?.isConfirmed != true, outcome: outcome)
        } catch let failure as DesktopActionFailure {
            return Self.failedActionReply(failure, focusRestored: focusRestored,
                focusDispatched: focusDispatched, diagnostic: focusDiagnostic)
        } catch {
            return NativeReply(ok: false, error: (focusRestored ? "focus_restored=true; " : "") + error.localizedDescription + focusDiagnostic, outcome_unknown: focusDispatched || dispatched, cancelled: error is CancellationError)
        }
    }

    static func mapPoint(x: Double, y: Double, imageSize: CGSize, bounds: CGRect) throws -> CGPoint {
        guard x.isFinite, y.isFinite, imageSize.width > 0, imageSize.height > 0,
              x >= 0, y >= 0, x < imageSize.width, y < imageSize.height, !bounds.isEmpty else { throw ActionError("point_outside_snapshot") }
        return CGPoint(x: bounds.minX + x * bounds.width / imageSize.width, y: bounds.minY + y * bounds.height / imageSize.height)
    }

    private func observe(_ request: ActionRequest, id: UInt64) async throws -> NativeObservation {
        await clearReceipt()
        let captured = try await CapturePreparation.capture(checkpoint: {
            try ActionMailbox.shared.checkpoint(id)
            try Self.checkSession()
        }) {
            if let window = request.window_id {
                return try await capture.captureWindow(windowID: window)
            }
            return try await capture.captureFrontmost()
        }
        try ActionMailbox.shared.checkpoint(id)
        guard let window = captured.metadata.windowInfo,
              let identity = window.mutationIdentity,
              SystemIdentityResolver.validateWindowMutationIdentity(identity) else { throw ActionError("capture_identity_missing") }
        let bounds = window.bounds
        let (png, size) = try Self.boundedPNG(captured.imageData)
        let snapshotID = try await snapshots.createSnapshot()
        do {
            let context = WindowContext(applicationName: captured.metadata.applicationInfo?.name,
                applicationProcessId: identity.ownerProcessIdentifier,
                applicationProcessStartIdentity: identity.ownerProcessStartIdentity,
                windowTitle: window.title, windowID: window.windowID, windowBounds: bounds,
                windowMutationIdentity: identity, shouldFocusWebContent: false,
                includeMenuBarElements: false, requiresFreshAccessibilityTree: true,
                accessibilityTimeoutSeconds: 3, allowApplicationScopedAccessibilityFallback: false)
            var elements: [NativeElement] = []
            var warning: String?
            do {
                let detected = try await automation.detectElements(in: captured.imageData, snapshotId: snapshotID, windowContext: context)
                elements = detected.elements.all.prefix(250).map {
                    NativeElement(id: $0.id, role: $0.attributes["role"] ?? String(describing: $0.type), label: $0.label.map { String($0.prefix(200)) }, bounds: NativeRect($0.bounds), enabled: $0.isEnabled)
                }
            } catch { warning = "accessibility_unavailable" }
            try ActionMailbox.shared.checkpoint(id)
            guard SystemIdentityResolver.validateWindowMutationIdentity(identity) else { throw ActionError("snapshot_stale") }
            let exact = try UIAutomationTarget.ExactWindow(identity: identity, bounds: bounds)
            let focused = try? await UIAutomationTarget.exactWindow(exact).pinningCurrentFocusedElement(using: automation).exactWindow?.focusedElement
            receipt = Receipt(id: snapshotID, identity: identity, bounds: bounds, imageSize: size, created: Date(), elementIDs: Set(elements.map(\.id)), focusedElement: focused)
            PreviewExecutor.shared.remember(snapshotID, identity: identity)
            return NativeObservation(snapshot_id: snapshotID, window_id: window.windowID,
                process_id: identity.ownerProcessIdentifier,
                bundle_id: NSRunningApplication(processIdentifier: identity.ownerProcessIdentifier)?.bundleIdentifier,
                title: String(window.title.prefix(500)), bounds: NativeRect(bounds),
                image_width: Int(size.width), image_height: Int(size.height), mime_type: "image/png",
                image_base64: png.base64EncodedString(), elements: elements, accessibility_warning: warning)
        } catch {
            try? await snapshots.cleanSnapshot(snapshotId: snapshotID)
            throw error
        }
    }

    private func clearReceipt() async {
        screenReceipt = nil
        if let old = receipt { receipt = nil; try? await snapshots.cleanSnapshot(snapshotId: old.id) }
    }

    private func discoverApps(id: UInt64) async throws -> [NativeApplication] {
        let inventory = try await applications.listApplications().data.applications
        try ActionMailbox.shared.checkpoint(id)
        var receipts: [String: ApplicationProcessIdentity] = [:]
        var result: [NativeApplication] = []
        for app in inventory.prefix(200) {
            guard let identity = app.processIdentity else { continue }
            let token = UUID().uuidString
            receipts[token] = identity
            let windows = SystemIdentityResolver.windowIdentities(ownerProcessIdentifier: identity.processIdentifier)
                .filter { $0.ownerProcessStartIdentity == identity.processStartIdentity }
                .prefix(30).map { NativeWindow(window_id: $0.windowID, title: String($0.title.prefix(200)), bounds: NativeRect($0.bounds)) }
            result.append(NativeApplication(app_id: token, process_id: identity.processIdentifier, bundle_id: app.bundleIdentifier, name: String(app.name.prefix(200)), windows: windows))
        }
        appReceipts = receipts
        appDiscoveryTime = Date()
        return result
    }

    // Preserve WindowServer's front-to-back order. A global pointer operation
    // must not use the old image's pixels if another window now covers them.
    private static func visibleWindows() throws -> [SystemWindowIdentity] {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { throw ActionError("window_inventory_unavailable") }
        return list.compactMap { row in
            guard let id = row[kCGWindowNumber as String] as? UInt32,
                  let window = SystemIdentityResolver.windowIdentity(id), window.alpha > 0,
                  !window.bounds.isEmpty else { return nil }
            return window
        }
    }

    // QA action1220 / 2026-10-05: another desktop controller can still own its
    // approval overlay when focus restoration completes. Await disappearance
    // before any pointer event; never whitelist that controller or replay input.
    // Every read checks cancellation/session/receipt/identity again. Persistent
    // cover remains refused after at most two seconds of sleep; native AX reads
    // add their own latency. Other failures refuse immediately (fail closed).
    static func awaitPointerReady(maximumAttempts: Int = 21, validate: () throws -> Void,
        wait: () async throws -> Void) async throws {
        for attempt in 0..<maximumAttempts {
            do { try validate(); return }
            catch let error as ActionError {
                guard error.code.hasPrefix("exact_window_pointer_obscured "),
                      attempt + 1 < maximumAttempts else { throw error }
            }
            try await wait()
        }
        throw ActionError("pointer_validation_unavailable")
    }

    // False negatives cost a new observation/approval; false positives can
    // click another window. Require every pointer endpoint to remain uncovered.
    static func validateForegroundPointer(windowID: CGWindowID, processID: Int32,
        bounds: CGRect, points: [CGPoint], frontmostPID: Int32?,
        windows: [SystemWindowIdentity],
        dockPassThrough: (SystemWindowIdentity, CGPoint) -> Bool = { _, _ in false }) throws {
        guard frontmostPID == processID, !points.isEmpty else { throw ActionError("exact_window_not_frontmost") }
        for point in points {
            let top = windows.first(where: { $0.bounds.contains(point) && !dockPassThrough($0, point) })
            guard bounds.contains(point), let top,
                  top.windowID == windowID, top.ownerProcessIdentifier == processID,
                  top.bounds == bounds else {
                // Live acceptance event 3531: preserve evidence before deciding
                // whether this is WindowServer settling or a real competing
                // window. Titles, AX values and screen content never enter this
                // diagnostic; positive-alpha overlays remain fail-closed.
                let actual = top.map {
                    "id=\($0.windowID),pid=\($0.ownerProcessIdentifier),layer=\($0.layer),alpha=\($0.alpha),bounds=\(NSStringFromRect($0.bounds))"
                } ?? "none"
                throw ActionError("exact_window_pointer_obscured expected_id=\(windowID),expected_pid=\(processID),expected_bounds=\(NSStringFromRect(bounds)),point=\(NSStringFromPoint(point)),point_inside=\(bounds.contains(point)),frontmost_pid=\(frontmostPID ?? -1),actual={\(actual)}")
            }
        }
    }

    static func focusApprovedWindow(
        focusDispatched: inout Bool,
        activate: () async throws -> DesktopActionOutcome?,
        focus: () async throws -> DesktopActionOutcome
    ) async throws -> DesktopActionOutcome {
        let activation = try await activate()
        // Activation throws its actual dispatch disposition. Do not invent a
        // dispatch before this call: rejected native/AX requests never ran.
        focusDispatched = activation?.isConfirmed != true
        guard activation?.isConfirmed == true else {
            throw ActionError("application_activation_unconfirmed")
        }
        return try await focus()
    }

    static func failedActionReply(_ failure: DesktopActionFailure, focusRestored: Bool,
        focusDispatched: Bool, diagnostic: String) -> NativeReply {
        NativeReply(ok: false, error: (focusRestored ? "focus_restored=true; " : "") + failure.message + diagnostic,
            outcome_unknown: focusDispatched || (failure.outcome.dispatchState.mutationDispatched && !failure.outcome.isConfirmed),
            outcome: failure.outcome)
    }

    private static func focusFailureDiagnostic(windowID: CGWindowID, processID: Int32) -> String {
        let app = NSRunningApplication(processIdentifier: processID)
        let target = SystemIdentityResolver.windowIdentity(windowID)
        let application = AXUIElementCreateApplication(processID)
        AXUIElementSetMessagingTimeout(application, 0.1)
        let resolver = AXWindowResolver()
        var focused: CFTypeRef?
        let focusedRead = AXUIElementCopyAttributeValue(application, kAXFocusedWindowAttribute as CFString, &focused)
        var focusedID: CGWindowID?
        if focusedRead == .success, let focused, CFGetTypeID(focused) == AXUIElementGetTypeID() {
            focusedID = resolver.windowID(from: unsafeDowncast(focused, to: AXUIElement.self))
        }
        var all: CFTypeRef?
        var minimized = "unavailable"
        if AXUIElementCopyAttributeValue(application, kAXWindowsAttribute as CFString, &all) == .success,
           let windows = all as? [AXUIElement] {
            for window in windows.prefix(32) where resolver.windowID(from: window) == windowID {
                AXUIElementSetMessagingTimeout(window, 0.1)
                var value: CFTypeRef?
                if AXUIElementCopyAttributeValue(window, kAXMinimizedAttribute as CFString, &value) == .success,
                   let value = value as? Bool { minimized = String(value) }
                break
            }
        }
        let spaces = SpaceManagementService().getSpacesForWindow(windowID: windowID)
        let facts: [String: String] = [
            "target_window": String(windowID), "target_pid": String(processID),
            "frontmost_pid": String(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1),
            "app_active": app.map { String($0.isActive) } ?? "unavailable",
            "app_hidden": app.map { String($0.isHidden) } ?? "unavailable",
            "app_terminated": app.map { String($0.isTerminated) } ?? "unavailable",
            "ax_focused_window": focusedID.map(String.init) ?? "unavailable",
            "ax_focus_read_status": String(focusedRead.rawValue),
            "target_minimized": minimized,
            "target_on_screen": target.map { String($0.isOnScreen) } ?? "unavailable",
            "space_count": String(spaces.count),
            "space_active": spaces.isEmpty ? "unavailable" : String(spaces.contains(where: \.isActive)),
        ]
        let data = try? JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys])
        return "; focus_diagnostic=" + (data.flatMap { String(data: $0, encoding: .utf8) } ?? "unavailable")
    }

    // Live event 3648: Dock's fullscreen tracking window reports alpha=1
    // while passing input through transparent pixels. Never ignore all Dock or
    // high-layer windows: exact system identity, display coverage AND native
    // point hit-testing must prove delivery to the approved application.
    static func dockOverlayMayPassThrough(window: SystemWindowIdentity, systemDock: Bool,
        displayBounds: [CGRect], hitPID: Int32?, expectedPID: Int32, hitFocusedWindow: Bool) -> Bool {
        systemDock && window.layer == Int(CGWindowLevelForKey(.dockWindow)) &&
            displayBounds.contains(window.bounds) && hitFocusedWindow && hitPID == expectedPID &&
            hitPID != window.ownerProcessIdentifier
    }

    static func cursorExecutableMatches(_ path: String?) -> Bool {
        let systemPath = "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer"
        // QA action1246: proc_pidpath returns the framework's Versions/A path;
        // resolve only our protected System constant, never an untrusted path.
        return path == systemPath || path == URL(fileURLWithPath: systemPath).resolvingSymlinksInPath().path
    }

    static func cursorOverlayMayPassThrough(window: SystemWindowIdentity, executablePath: String?,
        hitPID: Int32?, expectedPID: Int32, hitWindowID: CGWindowID?,
        focusedWindowID: CGWindowID?, expectedWindowID: CGWindowID) -> Bool {
        cursorExecutableMatches(executablePath) && window.layer == Int(CGWindowLevelForKey(.cursorWindow)) &&
            expectedPID > 0 && expectedWindowID != 0 && hitWindowID == expectedWindowID && focusedWindowID == expectedWindowID &&
            hitPID == expectedPID && hitPID != window.ownerProcessIdentifier
    }

    // QA15 action1211 / 2026-10-05: custom canvas hit-testing may return its
    // AXWindow directly, which has no parent AXWindow attribute. The previous
    // parent-only proof refused legitimate input under Dock's tracking overlay.
    // Missing identity still costs a retry; a false match could hit another window.
    static func pointerHitMatchesFocusedWindow(role: String?, parentWindow: CGWindowID?,
        ownWindow: CGWindowID?, focusedWindow: CGWindowID?) -> Bool {
        guard let focusedWindow, focusedWindow != 0 else { return false }
        let hit = parentWindow ?? (role == kAXWindowRole as String ? ownWindow : nil)
        return hit == focusedWindow
    }

    private static func systemOverlayPassesThrough(_ window: SystemWindowIdentity, at point: CGPoint,
        expectedPID: Int32, expectedWindowID: CGWindowID) -> Bool {
        let app = NSRunningApplication(processIdentifier: window.ownerProcessIdentifier)
        let systemDock = window.layer == Int(CGWindowLevelForKey(.dockWindow)) &&
            app?.bundleIdentifier == "com.apple.dock" &&
            app?.executableURL?.standardizedFileURL.path == "/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock"
        // QA action1242 / 2026-10-06: the OS cursor was mistaken for an
        // input-blocking window. Only its reserved level and kernel-reported
        // protected System executable qualify, never its title, PID or size.
        // False negatives cost another observation; false positives could send
        // unaudited input. Missing identity or AX evidence therefore refuses.
        var path = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
        let cursorPath = window.layer == Int(CGWindowLevelForKey(.cursorWindow)) &&
            proc_pidpath(window.ownerProcessIdentifier, &path, UInt32(path.count)) > 0 ? String(cString: path) : nil
        let systemCursor = Self.cursorExecutableMatches(cursorPath)
        guard systemDock || systemCursor else { return false }
        let displayBounds = NSScreen.screens.compactMap { screen -> CGRect? in
            guard let id = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber else { return nil }
            return CGDisplayBounds(id.uint32Value)
        }
        guard systemCursor || displayBounds.contains(window.bounds) else { return false }
        let system = AXUIElementCreateSystemWide()
        AXUIElementSetMessagingTimeout(system, 0.2)
        var hit: AXUIElement?
        guard AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &hit) == .success,
              let hit else { return false }
        var pid: pid_t = 0
        guard AXUIElementGetPid(hit, &pid) == .success else { return false }
        var hitWindow: CFTypeRef?
        var focusedWindow: CFTypeRef?
        let application = AXUIElementCreateApplication(expectedPID)
        AXUIElementSetMessagingTimeout(application, 0.2)
        AXUIElementSetMessagingTimeout(hit, 0.2)
        _ = AXUIElementCopyAttributeValue(hit, kAXWindowAttribute as CFString, &hitWindow)
        guard AXUIElementCopyAttributeValue(application, kAXFocusedWindowAttribute as CFString, &focusedWindow) == .success,
              let focusedWindow, CFGetTypeID(focusedWindow) == AXUIElementGetTypeID() else { return false }
        var role: CFTypeRef?
        _ = AXUIElementCopyAttributeValue(hit, kAXRoleAttribute as CFString, &role)
        let resolver = AXWindowResolver()
        guard resolver.windowID(from: unsafeDowncast(focusedWindow, to: AXUIElement.self)) == expectedWindowID else { return false }
        let parentID = hitWindow.flatMap { CFGetTypeID($0) == AXUIElementGetTypeID() ? resolver.windowID(from: unsafeDowncast($0, to: AXUIElement.self)) : nil }
        let exactHit = pointerHitMatchesFocusedWindow(role: role as? String,
            parentWindow: parentID, ownWindow: resolver.windowID(from: hit),
            focusedWindow: resolver.windowID(from: unsafeDowncast(focusedWindow, to: AXUIElement.self)))
        // Bind the actual AX hit to the focused window, then the caller checks
        // the first remaining WindowServer window is the exact receipt ID.
        // Merely belonging to the same Chrome process is insufficient.
        return cursorOverlayMayPassThrough(window: window, executablePath: cursorPath,
            hitPID: pid, expectedPID: expectedPID,
            hitWindowID: parentID ?? (role as? String == kAXWindowRole as String ? resolver.windowID(from: hit) : nil),
            focusedWindowID: resolver.windowID(from: unsafeDowncast(focusedWindow, to: AXUIElement.self)),
            expectedWindowID: expectedWindowID) ||
            dockOverlayMayPassThrough(window: window, systemDock: systemDock,
                displayBounds: displayBounds, hitPID: pid, expectedPID: expectedPID,
                hitFocusedWindow: exactHit)
    }

    private static func validateScreenTarget(at point: CGPoint, observed: [SystemWindowIdentity]) throws -> SystemWindowIdentity {
        let current = try validateScreenTarget(at: point, observed: observed, current: visibleWindows())
        guard !protectedApplication(pid: current.ownerProcessIdentifier) else { throw ActionError("protected_application") }
        return current
    }

    static func validateScreenTarget(at point: CGPoint, observed: [SystemWindowIdentity], current windows: [SystemWindowIdentity]) throws -> SystemWindowIdentity {
        guard let expected = observed.first(where: { $0.bounds.contains(point) }),
              let current = windows.first(where: { $0.bounds.contains(point) }),
              expected.windowID == current.windowID, expected.bounds == current.bounds,
              expected.ownerProcessIdentifier == current.ownerProcessIdentifier,
              let generation = expected.ownerProcessStartIdentity, generation == current.ownerProcessStartIdentity else { throw ActionError("screen_target_changed") }
        return current
    }

    private func observeScreen(_ request: ActionRequest, id: UInt64) async throws -> NativeObservation {
        await clearReceipt()
        let windows = try Self.visibleWindows()
        let captured = try await CapturePreparation.capture(checkpoint: {
            try ActionMailbox.shared.checkpoint(id)
            try Self.checkSession()
        }) { try await capture.captureScreen(displayIndex: request.display_index) }
        try ActionMailbox.shared.checkpoint(id)
        guard let display = captured.metadata.displayInfo else { throw ActionError("capture_identity_missing") }
        var count: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &count) == .success else { throw ActionError("display_inventory_unavailable") }
        var displays = [CGDirectDisplayID](repeating: 0, count: Int(count))
        guard CGGetActiveDisplayList(count, &displays, &count) == .success,
              let displayID = displays.first(where: { CGDisplayBounds($0) == display.bounds }) else { throw ActionError("display_identity_missing") }
        let (png, size) = try Self.boundedPNG(captured.imageData)
        let token = UUID().uuidString
        screenReceipt = ScreenReceipt(id: token, displayID: displayID, bounds: display.bounds, imageSize: size, created: Date(), windows: windows)
        return NativeObservation(snapshot_id: token, scope: "screen", window_id: nil, process_id: nil, bundle_id: nil,
            title: display.name ?? "Display", bounds: NativeRect(display.bounds), image_width: Int(size.width), image_height: Int(size.height),
            mime_type: "image/png", image_base64: png.base64EncodedString(), elements: [], accessibility_warning: nil)
    }

    private static func checkSession() throws {
        guard let session = CGSessionCopyCurrentDictionary() as? [String: Any],
              session["CGSSessionScreenIsLocked"] as? Bool != true else { throw ActionError("desktop_session_locked_or_unavailable") }
    }

    // Owner Q15: arbitrary editor/terminal GUI input cannot prove role file
    // ownership. These surfaces and system credential/permission apps are
    // refused even after a one-shot approval; a model label never waives this.
    private static func protectedApplication(pid: Int32) -> Bool {
        // The executor must never click its own owner approval/settings UI.
        if pid == ProcessInfo.processInfo.processIdentifier { return true }
        guard let bundle = NSRunningApplication(processIdentifier: pid)?.bundleIdentifier?.lowercased() else { return true }
        return bundle.hasPrefix("dev.hexagon.") || ["com.apple.terminal", "com.googlecode.iterm2", "dev.warp.warp-stable", "com.microsoft.vscode", "com.todesktop.230313mzl4w4u92", "com.apple.dt.xcode", "com.apple.systempreferences", "com.apple.keychainaccess", "com.apple.passwords", "com.apple.securityagent", "com.apple.loginwindow"].contains(bundle)
            || bundle.contains("1password") || bundle.contains("bitwarden") || bundle.hasPrefix("com.jetbrains.")
    }

    // AXSecureTextField is commonly a subrole, omitted from Peekaboo's focus
    // DTO. Ticket 07 credential protection must therefore recheck native AX.
    private static func focusIsNotSecure(pid: Int32) throws -> Bool {
        var raw: CFTypeRef?
        let focusStatus = AXUIElementCopyAttributeValue(AXUIElementCreateApplication(pid), kAXFocusedUIElementAttribute as CFString, &raw)
        guard focusStatus == .success,
              let raw, CFGetTypeID(raw) == AXUIElementGetTypeID() else { throw ActionError("focused_target_required: focus_status=\(focusStatus.rawValue)") }
        let focused = unsafeDowncast(raw, to: AXUIElement.self)
        var role: CFTypeRef?
        let roleStatus = AXUIElementCopyAttributeValue(focused, kAXRoleAttribute as CFString, &role)
        guard roleStatus == .success, let role = role as? String else { throw ActionError("focused_target_required: role_status=\(roleStatus.rawValue)") }
        guard role != "AXSecureTextField" else { return false }
        var subrole: CFTypeRef?
        let status = AXUIElementCopyAttributeValue(focused, kAXSubroleAttribute as CFString, &subrole)
        guard Self.focusAttributesAreNonSecure(role: role, subrole: subrole as? String, status: status) else {
            throw ActionError("focused_target_required: subrole_status=\(status.rawValue)")
        }
        return true
    }

    static func focusAttributesAreNonSecure(role: String, subrole: String?, status: AXError) -> Bool {
        // Live action1254 (2026-10-01): an ordinary Chrome AXTextField has no
        // AXSubrole value (-25212), which is distinct from an AX read failure.
        // Keep refusing secure fields and ambiguous reads: a false negative
        // costs another observation; a false positive can type into a secret.
        guard !role.isEmpty, role != "AXSecureTextField", subrole != "AXSecureTextField" else { return false }
        switch status {
        case .success: return subrole != nil
        case .attributeUnsupported, .noValue: return subrole == nil
        default: return false
        }
    }

    private static func boundedPNG(_ bytes: Data) throws -> (Data, CGSize) {
        guard let source = NSBitmapImageRep(data: bytes), let cg = source.cgImage else { throw ActionError("invalid_capture") }
        let scale = min(1, 1600 / Double(max(cg.width, cg.height)))
        let width = max(1, Int(Double(cg.width) * scale)), height = max(1, Int(Double(cg.height) * scale))
        guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { throw ActionError("image_resize_failed") }
        context.draw(cg, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let image = context.makeImage(), let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]), png.count <= 5_000_000 else { throw ActionError("capture_too_large") }
        return (png, CGSize(width: width, height: height))
    }
}

@_cdecl("hexagon_computer_start_v1")
public func hexagonComputerStart(_ json: UnsafePointer<CChar>?) -> UInt64 {
    guard let json else { return 0 }
    let count = strnlen(json, 65_537)
    guard count <= 65_536,
          let request = try? JSONDecoder().decode(ActionRequest.self, from: Data(bytes: json, count: count)),
          (try? request.validate()) != nil else { return 0 }
    return ActionMailbox.shared.start(request)
}

@_cdecl("hexagon_computer_poll_v1")
public func hexagonComputerPoll(_ id: UInt64) -> UnsafeMutablePointer<CChar>? {
    ActionMailbox.shared.poll(id).flatMap { strdup($0) }
}

@_cdecl("hexagon_computer_free_v1")
public func hexagonComputerFree(_ pointer: UnsafeMutablePointer<CChar>?) { free(pointer) }

@_cdecl("hexagon_computer_cancel_v1")
public func hexagonComputerCancel(_ id: UInt64) { ActionMailbox.shared.cancel(id) }
