# Hexagon Computer Use execution component

Pinned Peekaboo AutomationKit source: `f3c6b1b32528290a959244d1ac12d54af3d42928` (v4.7.0), MIT. SwiftPM dependency revisions are committed in `Package.resolved`. This target deliberately excludes Peekaboo's independent Agent runtime and model providers.

Build with `npm run build:computer-use`. macOS 15+ and Swift 6.2+ are required; actual validation currently covers this owner's macOS 26.6.2 / Swift 6.3.2 machine. `npm run dev` builds the component first. `npm run build` builds its release configuration and places the dylib next to the executable. An application distributor must put the dylib in the app's `Contents/Resources/libHexagonComputerUse.dylib` and sign it with the application identity before signing/notarizing the outer app. The repository currently has Tauri bundling disabled; no signed distributable is claimed.

The library loads into Hexagon itself. System grants therefore belong to the Hexagon application rather than an automatically chosen third-party Bridge. The fixed C ABI exposes fresh, nonprompting native permission checks; it does not infer grants from a settings-window launch. The version marker distinguishes a missing grant from a broken/incompatible component. Peekaboo's process-sticky grant cache is intentionally not used for revocation checks.

The action ABI supports application discovery/activation, window and screen observation, click, keyboard text/chords, AX scrolling and bounded drag. See PROTOCOL.md for receipt lifetimes, target verification, cancellation and uncertain outcomes. Rust owns project screenshot-sharing consent, approval policy, a cross-process single-role lease, explicit pause/resume, local screenshot evidence and model image feedback. Native preflight success never authorizes an action. Headless native tests verify protocol and coordination behavior; real desktop acceptance must be recorded separately.

Licenses: Peekaboo, AXorcist, Commander: MIT; Apple Swift Algorithms, Logging, Numerics: Apache-2.0 with Swift runtime exception. Retain their license notices when distributing a binary.
