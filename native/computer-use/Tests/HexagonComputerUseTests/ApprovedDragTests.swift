import AppKit
import XCTest
@testable import HexagonComputerUse

final class ApprovedDragTests: XCTestCase {
    @MainActor func testMailboxReplyDistinguishesPreDownRefusalFromPartialDelivery() async throws {
        struct Reply: Decodable { let ok: Bool; let outcome_unknown: Bool }
        for failAt in [1, 3] {
            let mailbox = ActionMailbox()
            let request = try JSONDecoder().decode(ActionRequest.self, from: Data(#"{"op":"drag","snapshot_id":"test","x":10,"y":20,"to_x":130,"to_y":20}"#.utf8))
            let id = mailbox.start(request, operation: { _, operationID in
                var calls = 0
                var dispatched = false
                do {
                    _ = try await ApprovedDrag.execute(from: CGPoint(x: 10, y: 20),
                        to: CGPoint(x: 130, y: 20), duration: 600, validate: { _ in
                            calls += 1
                            if calls == failAt { throw ActionError("target_changed") }
                        }, onDispatch: {
                            try mailbox.checkpoint(operationID, dispatch: true)
                            dispatched = true
                        }, post: { _ in }, wait: { _ in })
                    return NativeReply(ok: true)
                } catch { return NativeReply(ok: false, error: error.localizedDescription, outcome_unknown: dispatched) }
            })
            var encoded: String?
            for _ in 0..<100 {
                encoded = mailbox.poll(id)
                if encoded != nil { break }
                try await Task.sleep(for: .milliseconds(5))
            }
            let payload = try XCTUnwrap(encoded)
            let reply = try JSONDecoder().decode(Reply.self, from: Data(payload.utf8))
            XCTAssertFalse(reply.ok)
            XCTAssertEqual(reply.outcome_unknown, failAt != 1)
        }
    }

    @MainActor func testSmallFractionalMovesKeepTheirCumulativeDelta() async throws {
        for distance in [-7.0, -1.0, 1.0, 7.0] {
            var dx: CGFloat = 0
            var dy: CGFloat = 0
            _ = try await ApprovedDrag.execute(from: CGPoint(x: 100.25, y: 200.25),
                to: CGPoint(x: 100.25 + distance, y: 200.25 - distance), duration: 100,
                validate: { _ in }, post: { event in
                    if event.type == .leftMouseDragged, let appKit = NSEvent(cgEvent: event) {
                        dx += appKit.deltaX; dy += appKit.deltaY
                    }
                }, wait: { _ in })
            XCTAssertEqual(dx, distance)
            XCTAssertEqual(dy, -distance)
        }
    }

    @MainActor func testDeltaBasedCanvasReceivesActualDisplacementAndBalancedRelease() async throws {
        // Live drag 2026-10-06: AppKit received twenty drag events, but all
        // deltaX/deltaY values were zero and the rectangle did not move.
        var events: [CGEvent] = []
        _ = try await ApprovedDrag.execute(from: CGPoint(x: 430, y: 452),
            to: CGPoint(x: 550, y: 392), duration: 600, validate: { _ in },
            post: { events.append($0.copy()!) }, wait: { _ in })
        let movements = events.filter { $0.type == .leftMouseDragged }.compactMap(NSEvent.init(cgEvent:))
        XCTAssertEqual(movements.count, 20)
        XCTAssertEqual(movements.reduce(0) { $0 + $1.deltaX }, 120, accuracy: 0.001)
        XCTAssertEqual(movements.reduce(0) { $0 + $1.deltaY }, -60, accuracy: 0.001)
        XCTAssertEqual(events.first?.type, .leftMouseDown)
        XCTAssertEqual(events.last?.type, .leftMouseUp)
        XCTAssertEqual(events.last?.location, CGPoint(x: 550, y: 392))
    }

    @MainActor func testCancellationReleasesAtLastDeliveredPositionWithoutReplaying() async {
        var events: [CGEvent] = []
        do {
            _ = try await ApprovedDrag.execute(from: CGPoint(x: 10, y: 20),
                to: CGPoint(x: 130, y: 20), duration: 2000, validate: { _ in },
                post: { events.append($0.copy()!) }, wait: { _ in throw CancellationError() })
            XCTFail("cancelled drag returned success")
        } catch { XCTAssertTrue(error is CancellationError) }
        XCTAssertEqual(events.map(\.type), [.leftMouseDown, .leftMouseDragged, .leftMouseUp])
        XCTAssertEqual(events.last?.location, CGPoint(x: 16, y: 20))
    }

    @MainActor func testChangedTargetBeforeDispatchEmitsNothingAndMidDragReleases() async {
        for failAt in [1, 3] {
            var calls = 0
            var events: [CGEvent] = []
            var dispatched = false
            do {
                _ = try await ApprovedDrag.execute(from: CGPoint(x: 10, y: 20),
                    to: CGPoint(x: 130, y: 20), duration: 600, validate: { _ in
                        calls += 1
                        if calls == failAt { throw ActionError("target_changed") }
                    }, onDispatch: { dispatched = true }, post: { events.append($0.copy()!) }, wait: { _ in })
                XCTFail("changed target accepted")
            } catch { }
            XCTAssertEqual(events.map(\.type), failAt == 1 ? [] : [.leftMouseDown, .leftMouseDragged, .leftMouseUp])
            XCTAssertEqual(dispatched, failAt != 1)
        }
    }
}
