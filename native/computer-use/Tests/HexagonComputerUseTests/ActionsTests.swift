import Foundation
import ApplicationServices
import XCTest
import PeekabooAutomationKit
@testable import HexagonComputerUse

final class ActionsTests: XCTestCase {
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
