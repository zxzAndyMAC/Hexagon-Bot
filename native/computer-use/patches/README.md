# Fixed capture owner patch

Incident: QA 2026-10-06, action1235; `.scratch/qa-five-followups-2026-10-05/stable-0852-sample.txt` located a MainActor owner claim inside synchronous Security/directory reads. Prewarming alone cannot cover newly appearing processes.

`manifest.json` binds the upstream commit and SHA-256 of the one source file before/after `capture-owner-background.patch`. Both build and test preparation verify the revision and exact contents, apply once, and reject drift. The patch keeps all real lease checks; only execution isolation and cancellation-before-dispatch change. The internal TaskLocal lease seam is used only by headless tests; production uses the original lease. It never overrides production ownership with a synthetic receipt.

Run `npm run test:computer-use` after changing this patch. Review the pinned upstream implementation when updating the SDK, regenerate both hashes, and prove blocked-scan responsiveness, dynamic leaf conflict rejection and cancellation with the same tests. Direct Swift invocations do not prepare clean dependencies.

A noncooperative Security call remains awaited by the actual capture lane; responsiveness does not imply a bounded Security API. First preparation can finish in the background after caller timeout, but it never captures. Process-lifetime lease retention and SCK coordinator quarantine stay unchanged.
