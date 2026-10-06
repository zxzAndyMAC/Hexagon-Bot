import Foundation
import XCTest
@testable import HexagonComputerUse

final class CapturePreparationTests: XCTestCase {
    private final class Preparation: @unchecked Sendable {
        let entered = XCTestExpectation(description: "background preparation entered")
        let finished = XCTestExpectation(description: "background preparation drained")
        let release = DispatchSemaphore(value: 0)
        func run() {
            XCTAssertFalse(Thread.isMainThread)
            entered.fulfill()
            _ = release.wait(timeout: .now() + 5)
            finished.fulfill()
        }
    }
    @MainActor private final class CaptureCount { var value = 0 }

    @MainActor
    func testTimeoutRemainsResponsiveAndLatePreparationNeverCaptures() async {
        let preparation = Preparation()
        let captures = CaptureCount()
        let operation = Task { @MainActor in
            try await CapturePreparation.capture(timeout: .milliseconds(100),
                prepare: { preparation.run() }, checkpoint: {}) {
                captures.value += 1
            }
        }
        await fulfillment(of: [preparation.entered], timeout: 2)
        // The caller resumes even while the noncooperative preparation is held.
        do { try await operation.value; XCTFail("timeout returned success") }
        catch { XCTAssertEqual(error.localizedDescription, "capture_preparation_timeout") }
        XCTAssertEqual(captures.value, 0)
        preparation.release.signal()
        await fulfillment(of: [preparation.finished], timeout: 2)
        await Task.yield()
        XCTAssertEqual(captures.value, 0)
    }

    @MainActor
    func testPreparationFailureAndFinalCheckpointNeverEnterCapture() async {
        for rejectedPreparation in [true, false] {
            var captures = 0
            do {
                try await CapturePreparation.capture(prepare: {
                    if rejectedPreparation { throw ActionError("owner_conflict") }
                }, checkpoint: { throw CancellationError() }) {
                    captures += 1
                }
                XCTFail("refusal returned success")
            } catch {
                if rejectedPreparation { XCTAssertEqual(error.localizedDescription, "owner_conflict") }
                else { XCTAssertTrue(error is CancellationError) }
            }
            XCTAssertEqual(captures, 0)
        }
    }

    @MainActor
    func testMailboxCancellationDrainsAndNeverStartsLateCapture() async throws {
        let preparation = Preparation()
        let mailbox = ActionMailbox()
        let captures = CaptureCount()
        let request = try JSONDecoder().decode(ActionRequest.self, from: Data(#"{"op":"observe"}"#.utf8))
        let id = mailbox.start(request) { _, id in
            do {
                return try await CapturePreparation.capture(prepare: { preparation.run() },
                    checkpoint: { try mailbox.checkpoint(id) }) {
                    captures.value += 1
                    return NativeReply(ok: true)
                }
            } catch { return NativeReply(ok: false, error: error.localizedDescription) }
        }
        XCTAssertNotEqual(id, 0)
        await fulfillment(of: [preparation.entered], timeout: 2)
        mailbox.cancel(id)
        XCTAssertEqual(mailbox.start(request), 0, "cancellation alone cannot release the lane")
        var result: String?
        let deadline = ContinuousClock.now.advanced(by: .seconds(2))
        while result == nil && ContinuousClock.now < deadline {
            result = mailbox.poll(id)
            await Task.yield()
        }
        let reply = try XCTUnwrap(result)
        let decoded = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any])
        XCTAssertEqual(decoded["cancelled"] as? Bool, true)
        XCTAssertEqual(decoded["outcome_unknown"] as? Bool, false)
        XCTAssertEqual(captures.value, 0)
        preparation.release.signal()
        await fulfillment(of: [preparation.finished], timeout: 2)
        await Task.yield()
        XCTAssertEqual(captures.value, 0)
    }

    @MainActor
    func testSuccessfulPreparationChecksBeforeCapturingExactlyOnce() async throws {
        var steps: [String] = []
        let value = try await CapturePreparation.capture(prepare: {}, checkpoint: {
            steps.append("checkpoint")
        }) {
            steps.append("capture")
            return 73
        }
        XCTAssertEqual(value, 73)
        XCTAssertEqual(steps, ["checkpoint", "capture"])
    }
}
