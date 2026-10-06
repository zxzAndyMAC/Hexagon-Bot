import Foundation
import ApplicationServices
import XCTest
import PeekabooAutomationKit
import PeekabooFoundation
@testable import HexagonComputerUse

final class ActionsTests: XCTestCase {
    @MainActor
    func testApprovedClickPreparesApplicationBeforeExactWindowFocus() async throws {
        // QA action574: foregrounding the app must precede exact-window focus.
        var steps: [String] = []
        var focusDispatched = false
        let result = try await ActionExecutor.focusApprovedWindow(focusDispatched: &focusDispatched, activate: {
            steps.append("activate")
            return .confirmedNoChange()
        }, focus: {
            steps.append("exact-window")
            return .confirmedNoChange()
        })
        XCTAssertTrue(result.isConfirmed)
        XCTAssertEqual(steps, ["activate", "exact-window"])
        XCTAssertFalse(focusDispatched)
    }

    @MainActor
    func testUnconfirmedOrRejectedActivationNeverProceedsToWindowFocus() async {
        var focused = false
        var focusDispatched = false
        do {
            _ = try await ActionExecutor.focusApprovedWindow(focusDispatched: &focusDispatched, activate: { nil }, focus: {
                focused = true
                return .confirmedNoChange()
            })
            XCTFail("unconfirmed activation accepted")
        } catch { }
        XCTAssertFalse(focused)
        XCTAssertTrue(focusDispatched)
        focusDispatched = false
        do {
            _ = try await ActionExecutor.focusApprovedWindow(focusDispatched: &focusDispatched, activate: {
                throw ActionError("target_generation_changed")
            }, focus: {
                focused = true
                return .confirmedNoChange()
            })
            XCTFail("rejected activation accepted")
        } catch { }
        XCTAssertFalse(focused)
        XCTAssertFalse(focusDispatched)
    }

    @MainActor
    func testActivationRefusalAndUnknownDeliveryKeepTheirActualDisposition() async {
        let failures: [DesktopActionFailure] = [
            .preDispatchRefusal(reason: .targetUnavailable, message: "activation rejected", hint: "observe"),
            .indeterminate(delivery: nil, evidence: .completionUnknown, unitCount: .one,
                message: "activation unknown", hint: "observe")
        ]
        for (index, failure) in failures.enumerated() {
            var focusDispatched = false
            do {
                _ = try await ActionExecutor.focusApprovedWindow(focusDispatched: &focusDispatched, activate: { throw failure }, focus: {
                    XCTFail("failed activation proceeded to focus")
                    return .confirmedNoChange()
                })
                XCTFail("failed activation returned success")
            } catch let caught as DesktopActionFailure {
                let reply = ActionExecutor.failedActionReply(caught, focusRestored: false,
                    focusDispatched: focusDispatched, diagnostic: "")
                XCTAssertFalse(reply.ok)
                XCTAssertEqual(reply.outcome_unknown, index == 1)
                XCTAssertEqual(reply.outcome, failure.outcome)
            } catch { XCTFail("typed disposition lost: \(error)") }
        }
    }
    @MainActor
    func testAbsentOptionalSubroleAllowsOrdinaryFieldButNeverSecureOrReadErrors() {
        // Live action1254: Chrome's ordinary title field returned noValue for
        // optional AXSubrole. It was blocked even though exact focus matched.
        for status: AXError in [.attributeUnsupported, .noValue] {
            XCTAssertTrue(ActionExecutor.focusAttributesAreNonSecure(role: "AXTextField", subrole: nil, status: status))
        }
        for status: AXError in [.success, .failure, .illegalArgument, .invalidUIElement,
                               .cannotComplete, .attributeUnsupported, .apiDisabled, .noValue] {
            XCTAssertFalse(ActionExecutor.focusAttributesAreNonSecure(role: "AXSecureTextField", subrole: nil, status: status))
            XCTAssertFalse(ActionExecutor.focusAttributesAreNonSecure(role: "AXTextField", subrole: "AXSecureTextField", status: status))
        }
        for status: AXError in [.failure, .illegalArgument, .invalidUIElement, .cannotComplete, .apiDisabled] {
            XCTAssertFalse(ActionExecutor.focusAttributesAreNonSecure(role: "AXTextField", subrole: nil, status: status))
        }
        XCTAssertFalse(ActionExecutor.focusAttributesAreNonSecure(role: "AXTextField", subrole: nil, status: .success))
        XCTAssertTrue(ActionExecutor.focusAttributesAreNonSecure(role: "AXTextField", subrole: "AXSearchField", status: .success))
    }

    private func request(_ json: String) throws -> ActionRequest {
        try JSONDecoder().decode(ActionRequest.self, from: Data(json.utf8))
    }

    func testUntrustedInputRejectedBeforeDispatch() throws {
        for json in [
            #"{"op":"shell"}"#,
            #"{"op":"observe","window_id":0}"#,
            #"{"op":"type","text":"hello"}"#,
            #"{"op":"click","snapshot_id":"x","x":-1,"y":0}"#,
            #"{"op":"key","snapshot_id":"x","keys":"cmd,;open"}"#,
            #"{"op":"scroll","snapshot_id":"x","direction":"down","amount":21,"element_id":"e1"}"#,
            #"{"op":"scroll","snapshot_id":"x","direction":"down","amount":1}"#
            ,#"{"op":"activate","app_id":""}"#
            ,#"{"op":"observe_screen","display_index":-1}"#
            ,#"{"op":"drag","snapshot_id":"x","x":0,"y":0,"to_x":3,"to_y":4,"duration_ms":10000}"#
        ] {
            XCTAssertThrowsError(try request(json).validate(), json)
            XCTAssertEqual(json.withCString { hexagonComputerStart($0) }, 0)
        }
        let oversized = String(repeating: " ", count: 65_537)
        XCTAssertEqual(oversized.withCString { hexagonComputerStart($0) }, 0)
        XCTAssertEqual(hexagonComputerStart(nil), 0)
    }

    @MainActor
    func testScreenReceiptRejectsOcclusionMovedWindowAndPIDReuse() throws {
        func window(id: UInt32 = 1, generation: UInt64? = 9, offset: CGFloat = 0) -> SystemWindowIdentity {
            SystemWindowIdentity(windowID: id, ownerProcessIdentifier: 100,
                ownerProcessStartIdentity: generation, title: "test",
                bounds: CGRect(x: offset, y: 0, width: 200, height: 200),
                layer: 0, alpha: 1, isOnScreen: true, sharingState: nil)
        }
        let expected = window()
        for coordinate in 1..<100 {
            let point = CGPoint(x: coordinate, y: coordinate)
            XCTAssertEqual(try ActionExecutor.validateScreenTarget(at: point, observed: [expected], current: [expected]).windowID, 1)
            XCTAssertThrowsError(try ActionExecutor.validateScreenTarget(at: point, observed: [expected], current: [window(id: 2), expected]))
            XCTAssertThrowsError(try ActionExecutor.validateScreenTarget(at: point, observed: [expected], current: [window(generation: 10)]))
            XCTAssertThrowsError(try ActionExecutor.validateScreenTarget(at: point, observed: [expected], current: [window(offset: 1)]))
            XCTAssertThrowsError(try ActionExecutor.validateScreenTarget(at: point, observed: [window(generation: nil)], current: [window(generation: nil)]))
        }
    }

    @MainActor
    func testApprovedPointerRejectsAnotherWindowInSameAppAndOcclusion() throws {
        let bounds = CGRect(x: 10, y: 20, width: 200, height: 200)
        func window(_ id: UInt32, bounds frame: CGRect? = nil) -> SystemWindowIdentity {
            SystemWindowIdentity(windowID: id, ownerProcessIdentifier: 100,
                ownerProcessStartIdentity: 9, title: "Chrome", bounds: frame ?? bounds,
                layer: 0, alpha: 1, isOnScreen: true, sharingState: nil)
        }
        for x in stride(from: 11.0, to: 200.0, by: 13) {
            let points = [CGPoint(x: x, y: 30), CGPoint(x: x, y: 190)]
            func validate(_ windows: [SystemWindowIdentity], pid: Int32? = 100) throws {
                try ActionExecutor.validateForegroundPointer(windowID: 1, processID: 100,
                    bounds: bounds, points: points, frontmostPID: pid, windows: windows)
            }
            XCTAssertNoThrow(try validate([window(1)]))
            XCTAssertThrowsError(try validate([window(2), window(1)])) { error in
                let message = error.localizedDescription
                XCTAssertTrue(message.contains("expected_id=1"))
                XCTAssertTrue(message.contains("actual={id=2,pid=100,layer=0"))
                XCTAssertTrue(message.contains("point_inside=true"))
                XCTAssertFalse(message.contains("Chrome"), "window titles must not enter diagnostics")
            }
            XCTAssertThrowsError(try validate([window(1)], pid: 200))
            XCTAssertThrowsError(try validate([window(1, bounds: bounds.offsetBy(dx: 1, dy: 0))]))
            XCTAssertThrowsError(try validate([window(2, bounds: CGRect(x: 10, y: 180, width: 200, height: 20)), window(1)]))
        }
    }

    @MainActor
    func testCanvasWindowHitNeedsExactFocusedWindowIdentity() {
        for id in 1...100 as ClosedRange<UInt32> {
            XCTAssertTrue(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXWindow", parentWindow: nil, ownWindow: id, focusedWindow: id))
            XCTAssertTrue(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXButton", parentWindow: id, ownWindow: nil, focusedWindow: id))
            XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXWindow", parentWindow: nil, ownWindow: id, focusedWindow: id + 1))
            XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXButton", parentWindow: nil, ownWindow: id, focusedWindow: id))
            XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: nil, parentWindow: nil, ownWindow: id, focusedWindow: id))
            XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXWindow", parentWindow: id + 1, ownWindow: id, focusedWindow: id))
            XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXWindow", parentWindow: nil, ownWindow: id, focusedWindow: nil))
        }
        XCTAssertFalse(ActionExecutor.pointerHitMatchesFocusedWindow(role: "AXWindow", parentWindow: nil, ownWindow: 0, focusedWindow: 0))
    }

    @MainActor
    func testPointerPreparationOnlyWaitsForCoverAndRemainsBounded() async throws {
        for coveredReads in 0...24 {
            var reads = 0; var waits = 0
            do {
                try await ActionExecutor.awaitPointerReady(validate: {
                    reads += 1
                    if reads <= coveredReads { throw ActionError("exact_window_pointer_obscured actual=overlay") }
                }, wait: { waits += 1 })
                XCTAssertLessThan(coveredReads, 21)
            } catch {
                XCTAssertGreaterThanOrEqual(coveredReads, 21)
            }
            XCTAssertEqual(reads, min(coveredReads + 1, 21))
            XCTAssertEqual(waits, min(coveredReads, 20))
        }
        for changed in ["snapshot_stale_after_focus", "exact_window_not_frontmost", "session_locked", "window_inventory_unavailable"] {
            var waits = 0
            do {
                try await ActionExecutor.awaitPointerReady(validate: { throw ActionError(changed) }, wait: { waits += 1 })
                XCTFail("changed target accepted")
            } catch { XCTAssertEqual(error.localizedDescription, changed) }
            XCTAssertEqual(waits, 0)
        }
        var reads = 0
        do {
            try await ActionExecutor.awaitPointerReady(validate: {
                reads += 1
                throw ActionError("exact_window_pointer_obscured actual=overlay")
            }, wait: { throw CancellationError() })
            XCTFail("cancelled preparation accepted")
        } catch { XCTAssertTrue(error is CancellationError) }
        XCTAssertEqual(reads, 1)
    }

    @MainActor
    func testDockOverlayRequiresExactSystemAndPixelHitEvidence() throws {
        let bounds = CGRect(x: 0, y: 0, width: 1512, height: 982)
        func overlay(layer: Int = 20, frame: CGRect? = nil) -> SystemWindowIdentity {
            SystemWindowIdentity(windowID: 13, ownerProcessIdentifier: 200,
                ownerProcessStartIdentity: 9, title: "", bounds: frame ?? bounds,
                layer: layer, alpha: 1, isOnScreen: true, sharingState: nil)
        }
        func allows(_ window: SystemWindowIdentity, system: Bool = true, hit: Int32? = 100, focused: Bool = true) -> Bool {
            ActionExecutor.dockOverlayMayPassThrough(window: window, systemDock: system,
                displayBounds: [bounds], hitPID: hit, expectedPID: 100, hitFocusedWindow: focused)
        }
        XCTAssertTrue(allows(overlay()))
        XCTAssertFalse(allows(overlay(), system: false))
        XCTAssertFalse(allows(overlay(), hit: nil))
        XCTAssertFalse(allows(overlay(), hit: 200)) // Actual Dock item hit.
        XCTAssertFalse(allows(overlay(), hit: 300)) // Another application.
        XCTAssertFalse(allows(overlay(), focused: false)) // Another same-process window.
        XCTAssertFalse(allows(overlay(layer: 25)))
        XCTAssertFalse(allows(overlay(frame: CGRect(x: 0, y: 900, width: 1512, height: 82))))
    }

    func testDiscoveryAndBoundedDragRequestsAccepted() throws {
        for json in [
            #"{"op":"list_apps"}"#,
            #"{"op":"activate","app_id":"discovered-token"}"#,
            #"{"op":"observe_screen","display_index":0}"#,
            #"{"op":"drag","snapshot_id":"x","x":0,"y":0,"to_x":30,"to_y":40,"duration_ms":500}"#
        ] { XCTAssertNoThrow(try request(json).validate()) }
    }

    @MainActor
    func testCursorOverlayNeedsSystemIdentityExactLayerAndTargetHit() {
        // Live action1242: the system cursor itself obscured the endpoint.
        // A controller's approval overlay or a similarly named process is not
        // a cursor; passing through still requires the exact target AX hit.
        let systemPath = "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer"
        let realPath = URL(fileURLWithPath: systemPath).resolvingSymlinksInPath().path
        for layer in [Int(CGWindowLevelForKey(.cursorWindow)), 0, 20, 2147483629] {
            for path in [String?.none, systemPath, realPath, "/tmp/WindowServer", realPath + "-copy"] {
                for hit in [Int32?.none, 100, 200, 300] {
                    for hitWindow in [UInt32?.none, 0, 10, 11] {
                      for focused in [UInt32?.none, 0, 10, 11] {
                        let window = SystemWindowIdentity(windowID: 3, ownerProcessIdentifier: 200,
                            title: "", bounds: CGRect(x: 545, y: 447, width: 28, height: 40),
                            layer: layer, alpha: 1, isOnScreen: true, sharingState: nil)
                        XCTAssertEqual(ActionExecutor.cursorOverlayMayPassThrough(window: window,
                            executablePath: path, hitPID: hit, expectedPID: 100,
                            hitWindowID: hitWindow, focusedWindowID: focused, expectedWindowID: 10),
                            (path == systemPath || path == realPath) &&
                              layer == Int(CGWindowLevelForKey(.cursorWindow)) && hit == 100 && hitWindow == 10 && focused == 10)
                      }
                    }
                }
            }
        }
    }

    @MainActor
    func testCursorPassThroughStillRejectsGuardianAndAnotherWindow() throws {
        let bounds = CGRect(x: 300, y: 240, width: 500, height: 392)
        let point = CGPoint(x: 550, y: 452)
        func window(_ id: UInt32, pid: Int32, layer: Int, frame: CGRect = CGRect(x: 300, y: 240, width: 500, height: 392)) -> SystemWindowIdentity {
            SystemWindowIdentity(windowID: id, ownerProcessIdentifier: pid, ownerProcessStartIdentity: 9,
                title: "", bounds: frame, layer: layer, alpha: 1, isOnScreen: true, sharingState: nil)
        }
        let cursor = window(3, pid: 200, layer: Int(CGWindowLevelForKey(.cursorWindow)),
            frame: CGRect(x: 545, y: 447, width: 28, height: 40))
        let target = window(10, pid: 100, layer: 0)
        func validate(_ windows: [SystemWindowIdentity]) throws {
            try ActionExecutor.validateForegroundPointer(windowID: 10, processID: 100,
                bounds: bounds, points: [point], frontmostPID: 100, windows: windows,
                dockPassThrough: { candidate, _ in
                    ActionExecutor.cursorOverlayMayPassThrough(window: candidate,
                        executablePath: candidate.ownerProcessIdentifier == 200 ?
                          "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer" : nil,
                        hitPID: 100, expectedPID: 100, hitWindowID: 10,
                        focusedWindowID: 10, expectedWindowID: 10)
                })
        }
        XCTAssertNoThrow(try validate([cursor, target]))
        XCTAssertThrowsError(try validate([cursor, window(11, pid: 100, layer: 0), target]))
        for layer in [Int(CGWindowLevelForKey(.cursorWindow)), 2147483629] {
            let guardian = window(4, pid: 201, layer: layer)
            XCTAssertThrowsError(try validate([cursor, guardian, target]))
            XCTAssertThrowsError(try validate([guardian, cursor, target]))
        }
    }

    func testKeyboardChordMustHaveExactlyOnePrimaryKey() throws {
        // QA15: malformed chords must be refused before foreground preparation.
        for keys in ["cmd", "cmd,cmd,a", "a,b", "return,shift"] {
            let input = try request("{\"op\":\"key\",\"snapshot_id\":\"fresh\",\"keys\":\"\(keys)\"}")
            XCTAssertThrowsError(try input.validate(), keys)
        }
    }

    func testKeyboardReceiverRejectsSiblingWindowsSheetsAndAmbiguousReads() throws {
        for window in 1...200 {
            XCTAssertNoThrow(try ApprovedKeyboard.validateReceiver(expected: UInt32(window),
                actual: UInt32(window), role: "AXWindow", sheetsStatus: .success, sheetCount: 0))
            for actual: UInt32? in [nil, UInt32(window + 1)] {
                XCTAssertThrowsError(try ApprovedKeyboard.validateReceiver(expected: UInt32(window),
                    actual: actual, role: "AXWindow", sheetsStatus: .success, sheetCount: 0))
            }
            for (role, status, count): (String?, AXError, Int?) in [
                ("AXSheet", .success, 0), ("AXWindow", .success, 1),
                ("AXWindow", .cannotComplete, nil), ("AXWindow", .success, nil),
                (nil, .success, 0), ("AXWindow", .noValue, 1)
            ] {
                XCTAssertThrowsError(try ApprovedKeyboard.validateReceiver(expected: UInt32(window),
                    actual: UInt32(window), role: role, sheetsStatus: status, sheetCount: count))
            }
        }
    }

    func testKeyboardChordPropertiesAcrossPrimaryKeysAndModifierSubsets() {
        let primary = Array("abcdefghijklmnopqrstuvwxyz0123456789").map(String.init)
            + ["return", "tab", "space", "delete", "escape", "home", "end", "pageup", "pagedown",
               "arrow_left", "arrow_right", "arrow_up", "arrow_down"]
        let modifiers = ["cmd", "shift", "alt", "ctrl"]
        for key in primary {
            for mask in 0..<16 {
                let prefix = modifiers.enumerated().filter { mask & (1 << $0.offset) != 0 }.map(\.element)
                let chord = (prefix + [key]).joined(separator: ",")
                XCTAssertEqual(ApprovedKeyboard.validChord(chord), prefix.count <= 3, chord)
                XCTAssertFalse(ApprovedKeyboard.validChord(([key] + prefix + [key]).joined(separator: ",")))
                for modifier in prefix {
                    XCTAssertFalse(ApprovedKeyboard.validChord(([modifier] + prefix + [key]).joined(separator: ",")))
                }
            }
        }
    }

    @MainActor
    func testKeyboardCancellationAtDispatchAndPIDReuseAfterKeyDown() async throws {
        var posted: [CGEventType] = []
        var dispatched = false
        do {
            _ = try await ApprovedKeyboard.execute(text: nil, keys: "escape", processID: 100,
                validate: {}, generationMatches: { true }, onDispatch: { throw CancellationError() },
                post: { event, _ in posted.append(event.type) })
            XCTFail("cancelled dispatch accepted")
        } catch { XCTAssertTrue(error is CancellationError) }
        XCTAssertTrue(posted.isEmpty)
        var generationMatches = true
        do {
            _ = try await ApprovedKeyboard.execute(text: "AB", keys: nil, processID: 100,
                validate: {}, generationMatches: { generationMatches }, onDispatch: { dispatched = true },
                post: { event, pid in
                    XCTAssertEqual(pid, 100)
                    posted.append(event.type)
                    generationMatches = false
                })
            XCTFail("recycled PID accepted for release")
        } catch { XCTAssertEqual(error.localizedDescription, "keyboard_process_changed_after_dispatch") }
        XCTAssertTrue(dispatched)
        XCTAssertEqual(posted, [.keyDown], "do not send release or next unit to a recycled PID")
    }

    @MainActor
    func testApprovedKeyboardValidatesEveryUnitAndStopsWithoutReplay() async throws {
        var events: [CGEventType] = []
        var checks = 0
        var dispatched = 0
        do {
            _ = try await ApprovedKeyboard.execute(text: "AB", keys: nil, processID: 100,
                validate: {
                    checks += 1
                    if checks == 2 { throw ActionError("changed_focus") }
                }, generationMatches: { true }, onDispatch: { dispatched += 1 },
                post: { event, pid in XCTAssertEqual(pid, 100); events.append(event.type) })
            XCTFail("changed focus accepted")
        } catch { XCTAssertEqual(error.localizedDescription, "changed_focus") }
        XCTAssertEqual(checks, 2)
        XCTAssertEqual(dispatched, 1)
        XCTAssertEqual(events, [.keyDown, .keyUp], "first key must release; second must not dispatch")
    }

    @MainActor
    func testKeyboardRefusalOrCancelledCheckpointNeverPostsInput() async {
        for reason in ["cancelled", "secure_field", "window_changed", "generation_changed"] {
            var posted = false
            do {
                _ = try await ApprovedKeyboard.execute(text: nil, keys: "escape", processID: 100,
                    validate: { throw ActionError(reason) }, generationMatches: { true },
                    onDispatch: { XCTFail("refusal marked dispatched") }, post: { _, _ in posted = true })
                XCTFail("refusal accepted")
            } catch { XCTAssertEqual(error.localizedDescription, reason) }
            XCTAssertFalse(posted)
        }
        do {
            _ = try await ApprovedKeyboard.execute(text: "A", keys: nil, processID: 100,
                validate: {}, generationMatches: { false }, onDispatch: { XCTFail("reused PID accepted") },
                post: { _, _ in XCTFail("reused PID dispatched") })
            XCTFail("generation change accepted")
        } catch { }
    }

    @MainActor
    func testImageCoordinatesRespectScaleAndNegativeMonitorOrigin() throws {
        let bounds = CGRect(x: -1920, y: -200, width: 1920, height: 1080)
        let size = CGSize(width: 960, height: 540)
        for x in stride(from: 0.0, to: 960.0, by: 31) {
            for y in stride(from: 0.0, to: 540.0, by: 17) {
                let point = try ActionExecutor.mapPoint(x: x, y: y, imageSize: size, bounds: bounds)
                XCTAssertTrue(bounds.contains(point))
                XCTAssertEqual(point.x, -1920 + x * 2)
                XCTAssertEqual(point.y, -200 + y * 2)
            }
        }
        for (x, y) in [(960.0, 0.0), (0, 540), (-1, 0), (.infinity, 0), (.nan, 0)] {
            XCTAssertThrowsError(try ActionExecutor.mapPoint(x: x, y: y, imageSize: size, bounds: bounds))
        }
    }

    @MainActor
    func testCancelDoesNotReleaseLaneUntilNativeWorkReturnsAndResultConsumed() async throws {
        let mailbox = ActionMailbox()
        let input = try request(#"{"op":"observe"}"#)
        // Deliberately ignores task cancellation, modelling a synchronous AX
        // call. A cancelled task is not proof that desktop work has stopped.
        let id = mailbox.start(input) { _, _ in
            await withCheckedContinuation { continuation in
                DispatchQueue.global().asyncAfter(deadline: .now() + 0.05) { continuation.resume() }
            }
            return NativeReply(ok: true)
        }
        XCTAssertNotEqual(id, 0)
        mailbox.cancel(id)
        XCTAssertThrowsError(try mailbox.checkpoint(id, dispatch: true))
        XCTAssertEqual(mailbox.start(input), 0)
        XCTAssertNil(mailbox.poll(id))
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(mailbox.start(input), 0)
        let result = try XCTUnwrap(mailbox.poll(id))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(result.utf8)) as? [String: Any])
        XCTAssertEqual(json["cancelled"] as? Bool, true)
        XCTAssertEqual(json["outcome_unknown"] as? Bool, false)
        let next = mailbox.start(input) { _, _ in NativeReply(ok: true) }
        XCTAssertNotEqual(next, 0)
        try await Task.sleep(for: .milliseconds(10))
        XCTAssertNotNil(mailbox.poll(next))
    }

    @MainActor
    func testCancellationAfterDispatchCannotClaimNoEffects() async throws {
        let mailbox = ActionMailbox()
        let input = try request(#"{"op":"observe"}"#)
        let id = mailbox.start(input) { _, id in
            try? mailbox.checkpoint(id, dispatch: true)
            mailbox.cancel(id)
            return NativeReply(ok: true)
        }
        try await Task.sleep(for: .milliseconds(10))
        let result = try XCTUnwrap(mailbox.poll(id))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(result.utf8)) as? [String: Any])
        XCTAssertEqual(json["ok"] as? Bool, false)
        XCTAssertEqual(json["cancelled"] as? Bool, true)
        XCTAssertEqual(json["outcome_unknown"] as? Bool, true)
    }
}
