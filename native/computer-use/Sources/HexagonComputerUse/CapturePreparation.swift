import Foundation
import PeekabooAutomationKit

// QA 2026-10-06 / action1235. Preparation's own 8s timeout starts AFTER
// synchronous current-owner identity lookup. Start the entire entry off-main
// and bound the host's wait independently. A late preparation may warm the SDK
// cache, but only this waiting caller may enter capture after its checkpoint.
enum CapturePreparation {
    @MainActor
    static func capture<T: Sendable>(
        timeout: Duration = .seconds(8),
        prepare: @escaping @Sendable () async throws -> Void = {
            try await ScreenCaptureKitOwnerLease.prepareCurrentProcessCapability()
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
