import Darwin
import Foundation
import XCTest
import PeekabooFoundation
@testable import PeekabooAutomationKit
@testable import HexagonComputerUse

// QA 2026-10-06 action1235: synchronous Security/owner scans froze the UI.
// Inject only the slow process inventory into the real lease + capture gate;
// these tests never acquire the production lease or enter ScreenCaptureKit.
final class CaptureOwnershipTests: XCTestCase {
    private final class Scan: @unchecked Sendable {
        let entered = DispatchSemaphore(value: 0)
        let release = DispatchSemaphore(value: 0)
        let heartbeat = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var onMain = false
        private var responsive = false
        func inventory() -> [ScreenCaptureKitOwnerLease.UncoordinatedProcess] {
            lock.withLock { onMain = Thread.isMainThread }
            entered.signal()
            _ = release.wait(timeout: .now() + 5)
            return []
        }
        func unblockAfterHeartbeat() {
            DispatchQueue.global().async {
                guard self.entered.wait(timeout: .now() + 5) == .success else {
                    self.release.signal(); return
                }
                DispatchQueue.main.async { self.heartbeat.signal() }
                let didBeat = self.heartbeat.wait(timeout: .now() + 2) == .success
                self.lock.withLock { self.responsive = didBeat }
                self.release.signal()
            }
        }
        var scannedOnMain: Bool { lock.withLock { onMain } }
        var respondedWhileBlocked: Bool { lock.withLock { responsive } }
        func waitUntilEntered() -> Bool { entered.wait(timeout: .now() + 2) == .success }
    }

    private final class DynamicScan: @unchecked Sendable {
        private let lock = NSLock()
        private var count = 0
        var calls: Int { lock.withLock { count } }
        func inventory() -> [ScreenCaptureKitOwnerLease.UncoordinatedProcess] {
            let call = lock.withLock { count += 1; return count }
            return call == 1 ? [] : [.init(processIdentifier: 99999,
                processStartIdentity: 456, executablePath: "/fixture/uncoordinated")]
        }
    }

    @MainActor private final class CaptureCount { var value = 0 }

    private final class LeafScan: @unchecked Sendable {
        let entered = XCTestExpectation(description: "actual leaf scan entered")
        let finished = XCTestExpectation(description: "actual leaf scan drained")
        let release = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var count = 0
        func inventory() -> [ScreenCaptureKitOwnerLease.UncoordinatedProcess] {
            let call = lock.withLock { count += 1; return count }
            if call == 2 {
                entered.fulfill()
                _ = release.wait(timeout: .now() + 5)
                finished.fulfill()
            }
            return []
        }
    }

    @MainActor
    func testColdAndRepeatedOwnerScansLeaveMainActorResponsive() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: dir) }
        let scan = Scan()
        let lease = ScreenCaptureKitOwnerLease(lockURL: dir.appendingPathComponent("owner.lock"),
            ownerIdentity: .init(processIdentifier: getpid(), processStartIdentity: 123,
                                 buildIdentity: "test-fixture"),
            processStartIdentity: { _ in 123 }, uncoordinatedProcesses: { scan.inventory() })
        for _ in 0..<2 {
            scan.unblockAfterHeartbeat()
            let captured = try await ScreenCaptureKitCaptureGate.$processOwnerLeaseOverride.withValue(lease) {
                try await ScreenCaptureKitCaptureGate.withProcessOwner(operationName: "fixture") { 42 }
            }
            XCTAssertEqual(captured, 42)
            XCTAssertFalse(scan.scannedOnMain, "Security inventory must leave the main actor")
            XCTAssertTrue(scan.respondedWhileBlocked, "owner controls must respond while the scan is blocked")
        }
    }

    @MainActor
    func testDynamicConflictAtTheActualLeafStillRefusesCapture() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: dir) }
        let scan = DynamicScan()
        let lease = ScreenCaptureKitOwnerLease(lockURL: dir.appendingPathComponent("owner.lock"),
            ownerIdentity: .init(processIdentifier: getpid(), processStartIdentity: 123,
                                 buildIdentity: "test-fixture"),
            processStartIdentity: { _ in 123 }, uncoordinatedProcesses: { scan.inventory() })
        let coordination = ScreenCaptureKitCaptureGate.Coordination(
            operationLockPath: dir.appendingPathComponent("operation.lock").path,
            coordinator: ScreenCaptureKitOperationCoordinator(lockFilePath: dir.appendingPathComponent("capture.lock").path))
        var captures = 0
        do {
            try await ScreenCaptureKitCaptureGate.$processOwnerLeaseOverride.withValue(lease) {
                try await ScreenCaptureKitCaptureGate.$coordinationOverride.withValue(coordination) {
                    try await ScreenCaptureKitCaptureGate.runOwnedOperation(seconds: 2, operationName: "fixture") {
                        captures += 1
                    }
                }
            }
            XCTFail("dynamic conflict accepted")
        } catch let diagnostic as ScreenCaptureKitOwnershipDiagnostic {
            XCTAssertEqual(diagnostic.kind, .uncoordinatedProcesses)
        }
        XCTAssertEqual(scan.calls, 2, "entry success must not waive the actual leaf's recheck")
        XCTAssertEqual(captures, 0)
    }

    @MainActor
    func testCancellationOfActiveClaimKeepsLaneUntilScanDrainsAndNeverCaptures() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: dir) }
        let scan = Scan()
        let lease = ScreenCaptureKitOwnerLease(lockURL: dir.appendingPathComponent("owner.lock"),
            ownerIdentity: .init(processIdentifier: getpid(), processStartIdentity: 123,
                                 buildIdentity: "test-fixture"),
            processStartIdentity: { _ in 123 }, uncoordinatedProcesses: { scan.inventory() })
        let mailbox = ActionMailbox()
        let captures = CaptureCount()
        let request = try JSONDecoder().decode(ActionRequest.self, from: Data(#"{"op":"observe"}"#.utf8))
        let id = mailbox.start(request) { _, id in
            do {
                return try await ScreenCaptureKitCaptureGate.$processOwnerLeaseOverride.withValue(lease) {
                    try await ScreenCaptureKitCaptureGate.withProcessOwner(operationName: "fixture") {
                        try mailbox.checkpoint(id)
                        captures.value += 1
                        return NativeReply(ok: true)
                    }
                }
            } catch { return NativeReply(ok: false, error: error.localizedDescription) }
        }
        let entered = await Task.detached { scan.waitUntilEntered() }.value
        XCTAssertTrue(entered)
        mailbox.cancel(id)
        await Task.yield()
        XCTAssertNil(mailbox.poll(id))
        XCTAssertEqual(mailbox.start(request), 0)
        scan.release.signal()
        var result: String?
        let deadline = ContinuousClock.now.advanced(by: .seconds(2))
        while result == nil && ContinuousClock.now < deadline {
            result = mailbox.poll(id)
            await Task.yield()
        }
        let decoded = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(try XCTUnwrap(result).utf8)) as? [String: Any])
        XCTAssertEqual(decoded["cancelled"] as? Bool, true)
        XCTAssertEqual(decoded["outcome_unknown"] as? Bool, false)
        XCTAssertEqual(captures.value, 0)
    }

    @MainActor
    func testActualLeafCancellationAndTimeoutHoldLaneAndNeverCaptureLate() async throws {
        // Review 2026-10-06: the coordinator's independent operationTask does
        // not inherit parent cancellation. Exercise its SECOND real claim.
        for cancel in [true, false] {
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700])
            defer { try? FileManager.default.removeItem(at: dir) }
            let scan = LeafScan()
            let lease = ScreenCaptureKitOwnerLease(lockURL: dir.appendingPathComponent("owner.lock"),
                ownerIdentity: .init(processIdentifier: getpid(), processStartIdentity: 123,
                                     buildIdentity: "test-fixture"),
                processStartIdentity: { _ in 123 }, uncoordinatedProcesses: { scan.inventory() })
            let coordinator = ScreenCaptureKitOperationCoordinator(lockFilePath: dir.appendingPathComponent("capture.lock").path)
            let coordination = ScreenCaptureKitCaptureGate.Coordination(
                operationLockPath: dir.appendingPathComponent("operation.lock").path,
                coordinator: coordinator)
            let mailbox = ActionMailbox()
            let captures = CaptureCount()
            let request = try JSONDecoder().decode(ActionRequest.self, from: Data(#"{"op":"observe"}"#.utf8))
            let id = mailbox.start(request) { _, _ in
                do {
                    return try await ScreenCaptureKitCaptureGate.$processOwnerLeaseOverride.withValue(lease) {
                        try await ScreenCaptureKitCaptureGate.$coordinationOverride.withValue(coordination) {
                            try await ScreenCaptureKitCaptureGate.runOwnedOperation(seconds: cancel ? 2 : 0.1, operationName: "fixture") {
                                captures.value += 1
                                return NativeReply(ok: true)
                            }
                        }
                    }
                } catch { return NativeReply(ok: false, error: error.localizedDescription) }
            }
            await fulfillment(of: [scan.entered], timeout: 2)
            if cancel { mailbox.cancel(id) }
            let deadline = ContinuousClock.now.advanced(by: .seconds(2))
            while !coordinator.isQuarantined && ContinuousClock.now < deadline { await Task.yield() }
            XCTAssertTrue(coordinator.isQuarantined)
            // Let the terminal result reach the mailbox if it can. The blocked
            // native scan must still prevent publication and lane reuse.
            for _ in 0..<100 { await Task.yield() }
            var reply = mailbox.poll(id)
            XCTAssertNil(reply, "leaf scan still owns the lane before draining")
            let unexpected = mailbox.start(request) { _, _ in NativeReply(ok: true) }
            XCTAssertEqual(unexpected, 0)
            scan.release.signal()
            await fulfillment(of: [scan.finished], timeout: 2)
            let drained = ContinuousClock.now.advanced(by: .seconds(2))
            while reply == nil && ContinuousClock.now < drained {
                reply = mailbox.poll(id)
                await Task.yield()
            }
            XCTAssertNotNil(reply)
            while coordinator.isQuarantined && ContinuousClock.now < drained { await Task.yield() }
            XCTAssertFalse(coordinator.isQuarantined)
            XCTAssertEqual(captures.value, 0, "a cancelled/timed-out leaf must not capture after its scan returns")
            // Only relevant on the red version: don't leave its accidentally
            // admitted test action running after assertion failure.
            if unexpected != 0 {
                for _ in 0..<100 { await Task.yield() }
                _ = mailbox.poll(unexpected)
            }
        }
    }
}
