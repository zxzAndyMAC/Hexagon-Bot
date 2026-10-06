import Foundation
import PeekabooAutomationKit

// QA 2026-10-06 / action1235. Preparation's own 8s timeout starts AFTER
// synchronous current-owner identity lookup. Start the entire entry off-main
// and bound the host's wait independently. A late preparation may warm the SDK
// cache, but only this waiting caller may enter capture after its checkpoint.
enum CapturePreparation {
    static let progress = CapturePreparationProgress()
    @MainActor
    static func capture<T: Sendable>(
        timeout: Duration = .seconds(8),
        prepare: @escaping @Sendable () async throws -> Void = {
            try await progress.begin().value
        },
        checkpoint: () throws -> Void,
        operation: () async throws -> T
    ) async throws -> T {
        let wait = PreparationWait()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                guard wait.install(continuation) else { return }
                Task.detached(priority: .userInitiated) {
                    do { try await prepare(); wait.finish(.success(())) }
                    catch { wait.finish(.failure(error)) }
                }
                let timer = Task {
                    do { try await Task.sleep(for: timeout) }
                    catch { return }
                    wait.finish(.failure(ActionError("capture_preparation_timeout")))
                }
                wait.setTimer(timer)
            }
        } onCancel: { wait.finish(.failure(CancellationError())) }
        try Task.checkCancellation()
        try checkpoint()
        return try await operation()
    }
}

// QA 2026-10-06 / action1284: permission grants alone hid cold preparation.
// Start on owner enable/resume, share one background task, and expose progress.
// This never claims a lease or captures; every real leaf retains its live check.
final class CapturePreparationProgress: @unchecked Sendable {
    private let lock = NSLock()
    private var task: Task<Void, any Error>?
    private var state: UInt32 = 0
    private let prepare: @Sendable () async throws -> Void

    init(prepare: @escaping @Sendable () async throws -> Void = {
        try await ScreenCaptureKitOwnerLease.prepareCurrentProcessCapability(timeoutSeconds: 120)
    }) { self.prepare = prepare }

    var status: UInt32 { lock.withLock { state } }

    // QA review 2026-10-06: retry this wait, never clear the SDK's owner-identity
    // cache or uncoordinated-capture tombstone. A timed-out SDK task may finish;
    // a cached scan failure requires quitting/reopening the host (UI explains).
    @discardableResult
    func begin() -> Task<Void, any Error> {
        lock.withLock {
            if let task, state != 3 { return task }
            state = 1
            let prepare = self.prepare
            let next = Task.detached(priority: .userInitiated) {
                do {
                    try await prepare()
                    self.lock.withLock { self.state = 2 }
                } catch {
                    self.lock.withLock { self.state = 3 }
                    throw error
                }
            }
            task = next
            return next
        }
    }
}

// Closed scalar ABI: marker plus 0=unprepared/1=preparing/2=ready/3=failed.
// A read (start=0) has no side effects; only explicit owner control uses start=1.
@_cdecl("hexagon_computer_capture_preparation_v1")
public func hexagonComputerCapturePreparation(_ start: UInt32) -> UInt32 {
    if start == 1 { CapturePreparation.progress.begin() }
    return 0x1000 | CapturePreparation.progress.status
}

private final class PreparationWait: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, any Error>?
    private var terminal: Result<Void, any Error>?
    private var timer: Task<Void, Never>?

    func install(_ value: CheckedContinuation<Void, any Error>) -> Bool {
        let finished = lock.withLock { () -> Result<Void, any Error>? in
            if let terminal { return terminal }
            continuation = value
            return nil
        }
        if let finished { value.resume(with: finished); return false }
        return true
    }

    func setTimer(_ value: Task<Void, Never>) {
        let finished = lock.withLock {
            if terminal != nil { return true }
            timer = value
            return false
        }
        if finished { value.cancel() }
    }

    func finish(_ result: Result<Void, any Error>) {
        lock.lock()
        guard terminal == nil else { lock.unlock(); return }
        terminal = result
        let pending = continuation
        let clock = timer
        continuation = nil; timer = nil
        lock.unlock()
        clock?.cancel()
        pending?.resume(with: result)
    }
}
