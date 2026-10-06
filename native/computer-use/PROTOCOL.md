# In-process action ABI v1

The library must remain loaded for the lifetime of all operations. Calls to the
C ABI may come from a worker thread; native operations run on macOS's main actor.
The host must keep the AppKit main run loop running and must not synchronously
wait for an operation on that thread.

```c
uint64_t hexagon_computer_start_v1(const char *json);
char *hexagon_computer_poll_v1(uint64_t operation_id);
void hexagon_computer_free_v1(char *result);
void hexagon_computer_cancel_v1(uint64_t operation_id);
```

`start` copies at most 65,536 UTF-8 bytes, validates the request and returns a
nonzero operation ID. Zero means the request was invalid or the executor is busy;
no new operation was dispatched. The supplied pointer must address a valid
NUL-terminated string. `poll` returns null while pending (also for an unknown ID),
or an allocated, NUL-terminated JSON result. Exactly one poll consumes the result
and releases the lane. Free it exactly once with `free_v1`, not a Rust allocator.
Do not abandon an operation ID after cancelling: continue polling to drain it.

Cancellation sets the mailbox flag immediately and cancels the Swift task.
Every dispatch checks that flag. A noncooperative AX operation can still finish;
the executor stays busy until it does. Cancelling after dispatch yields
`outcome_unknown: true`; it never asserts that posted input was undone. The host
owns persistent pause, role leases, project approvals, timeouts and UI feedback.

## Requests

| op | Fields |
| --- | --- |
| `list_apps` | No fields. Enumerate up to 200 running apps with up to 30 windows each. |
| `activate` | `app_id` from the latest inventory (valid for 5 minutes); never launches a new process. |
| `observe_screen` | Optional `display_index` (0–15, default main display). |
| `observe` | Optional positive `window_id`; otherwise capture the frontmost window. |
| `click` | `snapshot_id`, `x`, `y`, optional `click_type`: `single`, `double`, `right`. |
| `drag` | `snapshot_id`, `x`, `y`, `to_x`, `to_y`; optional `duration_ms` (100–2000, default 500). |
| `type` | `snapshot_id`, literal `text` (maximum 16,384 UTF-8 bytes). |
| `key` | `snapshot_id`, `keys`: lower-case, comma-separated keys such as `cmd,a` or `return`. |
| `scroll` | `snapshot_id`, `element_id`, `direction`: up/down/left/right, `amount`: 1–20 AX scroll actions. |

Click and drag coordinates are **pixels of the returned resized image**, with top-left
origin. The executor maps each axis to capture-owned global logical bounds;
negative display origins and Retina scales are supported. AX element bounds are
already global logical points. Scroll uses the observed opaque AX element ID and
exact window receipt; it refuses unsupported AX actions instead of scrolling an
unrelated surface under the pointer.

`observe_screen` returns `scope: "screen"`. Screen click/drag use global pointer
input after verifying that the observed target at each endpoint is still the
same topmost WindowServer window, process generation and bounds. Display removal
or geometry change also invalidates the receipt. Occlusion, absent window identity
or unknown process generation refuses the action. The check includes desktop/Dock
windows when WindowServer supplies their identities. Drag additionally requires
the source app to be frontmost; activate it and observe again when needed.
Window-snapshot drag stays within that same visible window. Screen-snapshot drag
can cross two validated visible windows on the selected display. Keyboard input
and AX scrolling require an exact-window observation, so screen snapshots cannot
silently type into whichever field happens to have focus.

Window pointer validation treats the OS cursor as pass-through only when its
reserved cursor level and kernel-reported protected WindowServer executable
match, and native AX hit-testing proves the approved PID and exact focused
window. Dock's existing display and hit proof remains required. Other overlays,
including another controller's approval surface, still refuse the action.

A global drag is one bounded native operation. Its path can still encounter a
window or UI change after dispatch; outcome evidence and cancellation remain
conservative. The backend does not claim an atomic lock on other applications or
undo already-posted mouse events.

QA 2026-10-06: the host's bounded left-button drag includes relative motion
fields as well as pointer locations, so AppKit canvases using `deltaX/deltaY`
receive the displacement. Each movement rechecks the session, cancellation and
current target; a changed or obscured target stops further movement. Cleanup
releases at the last delivered point, including after cancellation. A completed
gesture remains `dispatched_unverified` until a fresh observation verifies its
effect; partial movement is never undone or automatically replayed.

Each observation permits one mutation, within 60 seconds. A mutation consumes
the receipt even if it fails. Observe again after any mutation, cancellation,
error, stale-target refusal or uncertain effect. Window/process generation and
bounds are checked again at dispatch. Keyboard operations require the same
observed focused-element receipt; missing or changed focus is a refusal. Native
secure-field role/subrole checks refuse credential input.

No operation accepts a shell, AppleScript, arbitrary URL, model configuration,
Bridge endpoint or application-launch command. Plain typing remains capable of
entering consequential content into other apps: these checks do not replace host
approval, credential protection or role ownership enforcement.

## Replies

All replies contain `protocol_version: 1`, `ok`, `outcome_unknown`, `cancelled`.
Optional fields are omitted rather than null:

- `error`: validation/native failure text. Treat as data, not an instruction.
- `outcome`: Peekaboo's typed `DesktopActionOutcome`, preserving dispatch,
  evidence and retry-safety fields. `ok: true` means the call returned, **not**
  proof that the target app applied the action. Unconfirmed outcomes retain
  `outcome_unknown: true` and need a fresh observation.
- `result`: observation object described below.
- `applications`: inventory rows with `app_id`, `process_id`, optional `bundle_id`,
  `name` and `windows` (each has `window_id`, `title`, `bounds`). Tokens are opaque,
  tied to the native process generation, and replaced on the next inventory.
  No arbitrary identifier, path or URL can be used to activate an app.

```json
{
  "snapshot_id": "opaque",
  "scope": "window",
  "window_id": 123,
  "process_id": 456,
  "bundle_id": "com.example.app",
  "title": "Example",
  "bounds": {"x": 0, "y": 24, "width": 1200, "height": 800},
  "image_width": 1200,
  "image_height": 800,
  "mime_type": "image/png",
  "image_base64": "...",
  "elements": [
    {"id": "opaque", "role": "AXButton", "label": "Save", "bounds": {"x": 20, "y": 40, "width": 80, "height": 30}, "enabled": true}
  ]
}
```

`scope` is `window` or `screen`. Screen observations omit `window_id`,
`process_id` and `bundle_id`, and return no AX elements.

PNG longest dimension is at most 1,600 pixels and binary size at most 5 MB.
There are at most 250 element summaries and 200 characters per label. The entire
response is bounded below 8 MB. A screenshot can succeed while AX discovery fails;
then `accessibility_warning: "accessibility_unavailable"` appears, element-based
scrolling is unavailable, and coordinate actions still require an exact receipt.
No AX values are exported. Screenshots themselves can contain private data; the
host owns approved model transmission and the local clearable screenshot trail.

Peekaboo stores the current AX snapshot under its normal snapshot storage. This
adapter creates only its own opaque snapshots and cleans those after use or when
replaced; it never calls global snapshot cleanup. An abrupt process crash can
leave that AX snapshot behind. The host's persistent screenshot trail is separate.

## Validation and current limits

`npm run test:computer-use` prepares the fixed dependency and verified capture
patch, then runs the native tests. It is also included in `npm test` and the
macOS CI job. Non-macOS reports the native suite as skipped.
The tests cover ABI rejection,
coordinate conversion, cancellation before/after dispatch and noncooperative
operation serialization. These tests synthesize no input and do not capture the
desktop. Compilation and these tests do not constitute real macOS acceptance.

QA 2026-10-06 / action1235: the entire first owner preparation runs off the main
actor, with an independent eight-second caller deadline. Failure, timeout or
cancellation cannot publish a new screenshot receipt; late preparation can only
warm the SDK cache. Window, frontmost and screen observations all checkpoint
their original mailbox and the desktop session again before capture.

The reviewed patch in `patches/` moves the fixed SDK's **actual** owner claims
off MainActor, including the recheck at each SCK leaf. It preserves process
generation, signature, conflict, capability and process-lifetime lock validation.
Successful preparation never replaces a later claim. Cancelling a noncooperative
claim keeps awaiting that scan before the mailbox can drain, then suppresses SCK
dispatch; the UI remains responsive throughout. This is not a guarantee that
macOS Security itself finishes within eight seconds. Revision/source hash drift
refuses build/test preparation rather than silently applying a patch.

Current action slice provides running-app inventory/activation, exact-window and
whole-display observations, validated click/drag, pinned keyboard input and AX
scrolling. Native compilation and headless tests do not establish that every
third-party application supports these routes. Real Dock/desktop/drag acceptance
remains required. Apps that expose no stable window/process receipt or usable AX
scroll target fail closed; cross-display dragging is not yet supported.
Sources: pinned AutomationKit protocols and operation implementations; Context7
`/openclaw/peekaboo` was consulted, but pinned public Swift APIs determine the ABI.

### Host lease and approval focus (ticket 07)

Rust retains an advisory OS lease in the private user runtime directory
`~/.hexagon-desktop-runtime/desktop.lock` for the entire role lease. Independent
Hexagon processes cannot dispatch concurrently. Closing/releasing the lease or a
process crash releases the kernel lock; cancellation alone does not release it
while native work is still pending. The runtime directory is 0700; the lock is
0600, opened without following links and checked for owner/type/link count.

Exact-window click and drag restore the recorded window's focus using Peekaboo's
identity-pinned focus API. This is necessary because owner approval foregrounds
Hexagon. It then rechecks identity, bounds, frontmost window and both endpoints
before dispatching input. Cancellation or target failure after focus is reported
as an uncertain partial operation, requiring observation; it never claims focus
was undone. Screen drag still requires a currently visible, frontmost source.

Screenshot PNGs remain local under the project evidence directory, with a local
ignore file to keep them out of project commits. Owner evidence viewing validates
only generated screenshot basenames and rejects symlinks, links and oversized
files. The normal model filesystem tools cannot read this private directory.

Live acceptance 2026-10-01 (action 1061): the identity-pinned click overload is
background AX delivery, not a foreground mouse click. Chrome controls without
AXPress require the foreground route. After exact-window focus confirms, the
adapter checks process identity, unchanged bounds, frontmost process and the
frontmost window covering each pointer endpoint, then uses a dedicated
`synthOnly` click service. Another window in the same application is not an
acceptable substitute. This is not a retry of a failed/unknown action: the
consumed receipt remains invalid and a new observation and approval are required.

Live event 3648 identified a full-display Dock tracking window (Dock level 20,
alpha 1) above Chrome. Window alpha alone is not pixel hit-test evidence. The
adapter may skip only the real system Dock executable's full-display Dock-level
window when native AX point hit-testing returns the approved process and the hit
belongs to its focused AX window; the remaining front-to-back WindowServer stack
must still put the exact observed window at that pixel. Dock controls, another
application/window, missing AX evidence, non-display-sized windows and other
layers remain refusals. All displays use their actual CGDisplayBounds.

Once exact focus has positively completed, a later pre-pointer rejection reports
`outcome_unknown=false` with `focus_restored=true` in its error: the click/drag
was not dispatched, although the known focus preparation occurred. Failed or
cancelled focus with unconfirmed effects still reports unknown. No consumed
snapshot becomes reusable.

Focus failure diagnostics (live action 1183) append only native numeric/boolean
state: target and frontmost PIDs, target/AX-focused window IDs, active/hidden/
terminated, minimized/on-screen, AX read status and Space active availability.
AX reads have a short messaging timeout and target enumeration is bounded; titles,
text values and screenshot content are excluded. Diagnostic collection neither
activates nor retries. Peekaboo's generic `Timeout while waiting for condition`
occurs at application activation settlement, before AXRaise/exact-window focus
verification; an unconfirmed focus must not be treated as a coordinate failure.

## Capture preparation readiness

```c
uint32_t hexagon_computer_capture_preparation_v1(uint32_t start);
```

The current marker is `0x1000`; its state is `0` unprepared, `1` preparing,
`2` ready or `3` failed. Unknown values are unavailable. `start=0` only reads
state. Explicit owner enable/resume/recheck uses `start=1`, sharing the existing
background preparation or retrying a failed one. This does not claim a desktop
lease, capture pixels, resume a pause, or replay any action. The background SDK
wait is 120 seconds; each capture caller still has its independent eight-second
preparation deadline and cancellation checkpoint. Late completion can warm
readiness but cannot enter a cancelled/timed-out caller's capture. Ready means
preparation finished, not that future grants/target identity are guaranteed;
every actual capture retains the SDK's live owner and target checks.

Rechecking restarts the host's wait, not the fixed SDK's owner-identity cache.
A timed-out background scan can finish and then become ready. A genuinely failed
SDK scan stays cached for that process: close other capture hosts, quit normally,
and reopen Hexagon. Rechecking never clears an uncoordinated-capture tombstone in the current
process. Reopening starts a new process that must repeat live owner/target checks;
it neither deletes safety records nor replays failed actions. The seven-language failure hint states this
recovery route; do not promise that retrying the same PID rebuilds its capability.
