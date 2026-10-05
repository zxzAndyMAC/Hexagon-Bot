import AppKit
import ApplicationServices
import PeekabooFoundation

@_silgen_name("_AXUIElementGetWindow")
private func keyboardAXWindowID(_ element: AXUIElement, _ windowID: inout CGWindowID) -> AXError

// QA15 / owner Q1, 2026-10-05: observation uses AXFocusedUIElement, while the
// SDK's background exact route searches AXChildren and can lose that element.
// After approved exact foreground restoration, keep the same direct focus
// reader at every input unit. Never fall back to unscoped global keyboard input.
enum ApprovedKeyboard {
    private static let codes: [String: CGKeyCode] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7,
        "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14, "r": 15,
        "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22,
        "5": 23, "9": 25, "7": 26, "8": 28, "0": 29, "o": 31, "u": 32,
        "i": 34, "p": 35, "return": 36, "l": 37, "j": 38, "k": 40,
        "n": 45, "m": 46, "tab": 48, "space": 49, "delete": 51, "escape": 53,
        "home": 115, "pageup": 116, "end": 119, "pagedown": 121,
        "arrow_left": 123, "arrow_right": 124, "arrow_down": 125, "arrow_up": 126
    ]

    static func validChord(_ keys: String) -> Bool {
        let parts = keys.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        let modifiers = parts.dropLast()
        return (1...4).contains(parts.count) && codes[parts.last ?? ""] != nil
            && Set(modifiers).count == modifiers.count
            && modifiers.allSatisfy { ["cmd", "shift", "alt", "ctrl"].contains($0) }
    }

    static func validateReceiver(expected: UInt32, actual: UInt32?, role: String?,
                                 sheetsStatus: AXError, sheetCount: Int?) throws {
        // A stale focused-element reference can outlive a sibling key window.
        // Refusing ambiguity costs a new observation; accepting it can send an
        // approved key to an unapproved sibling window or modal sheet.
        guard actual == expected, role == kAXWindowRole as String,
              (sheetsStatus == .success && sheetCount == 0)
                || ([.attributeUnsupported, .noValue].contains(sheetsStatus) && sheetCount == nil)
        else { throw ActionError("keyboard_receiver_changed_or_modal") }
    }

    @MainActor static func requireKeyWindow(processID: Int32, windowID: UInt32) throws {
        let app = AXUIElementCreateApplication(processID)
        AXUIElementSetMessagingTimeout(app, 0.2)
        var raw: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute as CFString, &raw) == .success,
              let raw, CFGetTypeID(raw) == AXUIElementGetTypeID() else { throw ActionError("keyboard_receiver_unavailable") }
        let window = unsafeDowncast(raw, to: AXUIElement.self)
        AXUIElementSetMessagingTimeout(window, 0.2)
        var pid: Int32 = 0
        var actual: UInt32 = 0
        guard AXUIElementGetPid(window, &pid) == .success, pid == processID,
              keyboardAXWindowID(window, &actual) == .success else { throw ActionError("keyboard_receiver_unavailable") }
        var role: CFTypeRef?
        guard AXUIElementCopyAttributeValue(window, kAXRoleAttribute as CFString, &role) == .success else { throw ActionError("keyboard_receiver_unavailable") }
        var sheets: CFTypeRef?
        let status = AXUIElementCopyAttributeValue(window, "AXSheets" as CFString, &sheets)
        try validateReceiver(expected: windowID, actual: actual, role: role as? String,
                             sheetsStatus: status, sheetCount: (sheets as? [AXUIElement])?.count)
    }

    @MainActor static func execute(
        text: String?, keys: String?, processID: Int32,
        validate: () async throws -> Void,
        generationMatches: () -> Bool,
        onDispatch: () throws -> Void,
        post: (CGEvent, Int32) -> Void = { $0.postToPid($1) }
    ) async throws -> DesktopActionOutcome {
        var units: [(CGKeyCode, CGEventFlags, [UniChar])] = []
        if let keys {
            guard validChord(keys) else { throw ActionError("invalid_keys") }
            let parts = keys.split(separator: ",").map(String.init)
            var flags: CGEventFlags = []
            for modifier in parts.dropLast() {
                switch modifier {
                case "cmd": flags.insert(.maskCommand)
                case "shift": flags.insert(.maskShift)
                case "alt": flags.insert(.maskAlternate)
                case "ctrl": flags.insert(.maskControl)
                default: throw ActionError("invalid_keys")
                }
            }
            units.append((codes[parts.last!]!, flags, []))
        } else if let text {
            for character in text {
                let special: CGKeyCode? = character == "\n" || character == "\r\n" || character == "\r" ? 36 : character == "\t" ? 48 : nil
                units.append((special ?? 0, [], special == nil ? Array(String(character).utf16) : []))
            }
        } else { throw ActionError("invalid_text") }
        guard let source = CGEventSource(stateID: .privateState) else { throw ActionError("keyboard_event_source_unavailable") }
        for (code, flags, unicode) in units {
            // Allocate the release before pressing. Cleanup stays with the original
            // generation even when the action changes focus or cancellation arrives.
            guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
                  let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else {
                throw ActionError("keyboard_event_unavailable")
            }
            down.flags = flags; up.flags = flags
            if !unicode.isEmpty {
                unicode.withUnsafeBufferPointer { buffer in
                    down.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress!)
                    up.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress!)
                }
            }
            try await validate()
            guard generationMatches() else { throw ActionError("keyboard_process_changed") }
            try onDispatch()
            post(down, processID)
            // No suspension between down/up; do not hold keys across cancellation.
            guard generationMatches() else { throw ActionError("keyboard_process_changed_after_dispatch") }
            post(up, processID)
        }
        return units.isEmpty ? .confirmedNoChange() : .dispatchedUnverified(
            delivery: .init(mechanism: .processTargetedEvents, mode: .foreground),
            evidence: .deliveryAccepted)
    }
}
