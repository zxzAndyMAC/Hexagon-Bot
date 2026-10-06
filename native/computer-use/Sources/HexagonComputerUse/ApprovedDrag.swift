import ApplicationServices
import PeekabooFoundation

// QA 2026-10-06 / issue 08: pinned Peekaboo GestureService sets pointer
// locations but leaves movement deltas zero; delta-based AppKit canvases
// receive a complete gesture without moving. Keep this bounded left-button
// route local rather than editing the resolved dependency or changing TCC.
// A target/session change must stop movement, but cleanup always releases the
// already-held button at its last delivered point. Delivery is not success.
enum ApprovedDrag {
    @MainActor static func execute(
        from: CGPoint, to: CGPoint, duration: Int,
        validate: (CGPoint) throws -> Void,
        onDispatch: () throws -> Void = {},
        post: (CGEvent) -> Void = { $0.post(tap: .cghidEventTap) },
        wait: (Int) async throws -> Void = { try await Task.sleep(for: .milliseconds($0)) }
    ) async throws -> DesktopActionOutcome {
        guard let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown,
                                 mouseCursorPosition: from, mouseButton: .left),
              let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp,
                               mouseCursorPosition: to, mouseButton: .left)
        else { throw ActionError("drag_event_unavailable") }
        var last = from
        try validate(from)
        // Like keyboard input, a refusal before down is not a dispatch.
        try onDispatch()
        post(down)
        defer { up.location = last; post(up) }
        for index in 1...20 {
            let progress = CGFloat(index) / 20
            let point = CGPoint(x: from.x + (to.x - from.x) * progress,
                                y: from.y + (to.y - from.y) * progress)
            try validate(point)
            guard let event = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDragged,
                                      mouseCursorPosition: point, mouseButton: .left)
            else { throw ActionError("drag_event_unavailable") }
            // Delta fields are integers. Difference rounded cumulative points
            // rather than rounding each step, so small moves do not disappear.
            event.setIntegerValueField(.mouseEventDeltaX, value: Int64(point.x.rounded() - last.x.rounded()))
            event.setIntegerValueField(.mouseEventDeltaY, value: Int64(point.y.rounded() - last.y.rounded()))
            post(event)
            last = point
            try await wait(duration / 20)
        }
        return .dispatchedUnverified(delivery: .init(mechanism: .globalEvents, mode: .foreground),
                                     evidence: .deliveryAccepted, unitCount: .one)
    }
}
