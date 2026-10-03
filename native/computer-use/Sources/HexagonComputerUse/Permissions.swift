@preconcurrency import ApplicationServices
import CoreGraphics
import PeekabooAutomationKit

// Owner decision 2026-10-01, desktop ticket 07: execute inside Hexagon so TCC
// grants belong to Hexagon, never an implicitly discovered third-party Bridge.
// Permission checks do not request access, capture pixels, or synthesize input.
// Peekaboo 4.7 caches positive grants for the process lifetime; use fresh native
// preflights here so a revoked permission is not reported as a cached success.
@_cdecl("hexagon_computer_permissions_v1")
public func hexagonComputerPermissions() -> UInt32 {
    var result: UInt32 = 0x1000
    if AXIsProcessTrusted() { result |= 1 }
    if CGPreflightScreenCaptureAccess() { result |= 2 }
    if CGPreflightPostEventAccess() { result |= 4 }
    return result
}

// Called only by the owner's explicit settings button, never by preflight.
// Requesting registers Hexagon with TCC; a preflight-only app may not yet have
// a row in System Settings. The return value of a request is not an approval.
@_cdecl("hexagon_computer_request_v1")
public func hexagonComputerRequest(_ permission: UInt32) {
    if permission == 0 {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        _ = CGRequestPostEventAccess()
    } else if permission == 1 {
        _ = CGRequestScreenCaptureAccess()
    }
}
