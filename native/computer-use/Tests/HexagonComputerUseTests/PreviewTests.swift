import Foundation
import XCTest
import PeekabooAutomationKit
@testable import HexagonComputerUse

final class PreviewTests: XCTestCase {
    @MainActor func testPreviewRequiresExactObservedReceipt() {
        let identity = WindowMutationIdentity(windowID: 12, ownerProcessIdentifier: 42, ownerProcessStartIdentity: 90)
        for window in 1...100 {
            let request = PreviewRequest(snapshot_id: "receipt", window_id: UInt32(window), process_id: 42, focus: false)
            XCTAssertEqual(PreviewExecutor.matches(request, snapshot: "receipt", identity: identity), window == 12)
            XCTAssertFalse(PreviewExecutor.matches(request, snapshot: "other", identity: identity))
            XCTAssertFalse(PreviewExecutor.matches(request, snapshot: "receipt", identity: WindowMutationIdentity(windowID: window, ownerProcessIdentifier: 43, ownerProcessStartIdentity: 90)))
            XCTAssertFalse(PreviewExecutor.matches(request, snapshot: "receipt", identity: WindowMutationIdentity(windowID: window, ownerProcessIdentifier: 42, ownerProcessStartIdentity: 0)))
        }
    }
    @MainActor func testPreviewResolutionBound() {
        for width in stride(from: 1, to: 10_000, by: 97) {
            for height in stride(from: 1, to: 5000, by: 113) {
                let size = PreviewExecutor.boundedSize(width: Double(width), height: Double(height))
                XCTAssertTrue((1...960).contains(size.0)); XCTAssertTrue((1...960).contains(size.1))
            }
        }
        XCTAssertEqual(PreviewExecutor.boundedSize(width: .infinity, height: 1).0, 1)
    }
    @MainActor func testStopDiscardsLatePixelsAndNeverQueuesWork() async throws {
        let mailbox = PreviewMailbox()
        let request = PreviewRequest(snapshot_id: "receipt", window_id: 12, process_id: 42, focus: false)
        var release: CheckedContinuation<Void, Never>?
        let id = mailbox.start(request) { _, _ in
            await withCheckedContinuation { release = $0 }
            return PreviewReply(ok: true, data_url: "secret old pixels", width: 1, height: 1, window_id: 12, process_id: 42)
        }
        XCTAssertNotEqual(id, 0)
        while release == nil { await Task.yield() }
        mailbox.stop()
        XCTAssertEqual(mailbox.start(request), 0)
        release?.resume()
        try await Task.sleep(for: .milliseconds(20))
        XCTAssertNil(mailbox.poll(id))
        XCTAssertThrowsError(try mailbox.checkpoint(id))
        // The capture has ended, but the 2fps bound still rejects another start.
        XCTAssertEqual(mailbox.start(request), 0)
    }
}

extension PreviewTests {
    @MainActor func testOwnerFocusBypassesFrameThrottleButKeepsCaptureSerialized() async throws {
        let mailbox = PreviewMailbox()
        let capture = PreviewRequest(snapshot_id: "receipt", window_id: 12, process_id: 42, focus: false)
        let focus = PreviewRequest(snapshot_id: "receipt", window_id: 12, process_id: 42, focus: true)
        var release: CheckedContinuation<Void, Never>?
        let frameID = mailbox.start(capture) { _, _ in
            await withCheckedContinuation { release = $0 }
            return PreviewReply(ok: true, window_id: 12, process_id: 42)
        }
        while release == nil { await Task.yield() }
        XCTAssertEqual(mailbox.start(focus), 0) // cannot overlap an actual capture
        release?.resume()
        var frame: String?
        while frame == nil { await Task.yield(); frame = mailbox.poll(frameID) }
        XCTAssertEqual(mailbox.start(capture), 0) // frame rate is still limited
        let focusID = mailbox.start(focus) { _, _ in PreviewReply(ok: true, window_id: 12, process_id: 42) }
        XCTAssertNotEqual(focusID, 0) // owner focus is not a preview frame
        var result: String?
        while result == nil { await Task.yield(); result = mailbox.poll(focusID) }
        XCTAssertEqual(mailbox.start(capture), 0)
    }
}
