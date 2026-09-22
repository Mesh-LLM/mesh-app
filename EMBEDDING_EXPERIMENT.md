# Embedded SDK experiment — draft, not a release candidate

This branch evaluates running the engine inside the native tray. Mic authorized
this exception to PRODUCT.md's child-process-only constraint, and requested a
review PR after the local inference trial. Other product requirements remain
the parity baseline; this draft does not change the shipping architecture yet.

Base: mesh-app main `78847359fbd8169e189201d1bf14ba8a3d4bf70e`.
SDK and identity: engine main as inspected on 2026-09-22,
`d2fc780f670e603c0ea78e0b39150ef8a2c9e8b5`. The immutable Git dependency makes
review builds reproducible; it does not prevent moving to newer main revisions.
Build the Metal runtime from that same revision, not an arbitrary release bundle.

## What changes

The menu owns an embedded worker instead of launching/signalling an engine child.
SDK configuration supplies ports, console UI, model, user config path, public
auto-join or private owner/require-owned/signed-invite requirements. Existing
bounded loopback status and invite validation remain; self PID is now the tray
PID. Native menu presentation is unchanged.

Stop requests are retained during startup. The worker finishes SDK startup and
then stops the resulting handle; replacements wait for worker completion.
There is no forced thread kill. SDK worker errors and disconnected workers latch
restart-unsafe: subsequent starts require restarting the whole app. The SDK can
return a startup error after its five-second cleanup wait expires, detaching a
runtime thread. An error is therefore not proof that replacement is safe. The
latch deliberately also covers errors that occurred before thread creation.
Successful normal stops permit restart.

## Local execution evidence

A release-mode local harness imported this branch's actual `src/lifecycle.rs`
and linked the SDK from the clean engine checkout at the revision above.
`just release-runtime-build metal` built the matching native library from source.
Two cycles completed in the **same process**:

1. Embedded private serve loaded a local Gemma 3 1B Q4_K_M model on Metal.
2. `/v1/chat/completions` returned `Hello there!`.
3. Cooperative stop completed and the inference port was released.
4. Start, inference and stop succeeded again.

Both status responses reported the same verified owner ID and private policy
hash. Both management/inference listeners were absent after process exit. No
separate engine executable was launched. The harness used an explicit 4K context
and a disposable plaintext test keystore: the inspected runtime loader bypasses
Keychain for unencrypted keystores. This proves actual inference and sequential
restart, **not** production credential integration or the complete native app.
The harness is local lab material, not a shipped executable or CI fixture.

The earlier pin `d4ffbbac` lacked the durable private-membership restart fix;
the new dependency includes it. The two-cycle trial is not an expiry or
multi-node admission matrix.

## Remaining gates — do not ship this draft

- Automatic model recipes request 64K context when the user has no override.
  The SDK builder still exposes no context override. This draft explicitly
  blocks that case rather than altering user config or silently changing it.
- Packaging scripts still package a child engine and need migration to a tray
  executable plus compatible native runtime. Signed-app resource discovery is
  not wired for the embedded layout. The trial supplied the runtime directory
  explicitly; it was not a packaged-app launch.
- Child stderr capture into `mesh.log` is gone. Integrate embedded startup
  diagnostics with the existing error-viewer experience.
- A running SDK handle is not yet supervised for unexpected runtime exit.
  Early startup cancellation and native-hang recovery remain limitations.
- Credential environment overrides cannot be scrubbed per-thread. Startup
  refuses the two overrides previously filtered on the child command.
- Windows startup remains gated. Cooperative shutdown is portable in principle;
  Windows DLL loading and native UI behavior have not been exercised.
- No menu-bar click-through, bundled browser UI, public/private switch or
  stop-during-startup live trial. The local harness had no built web UI assets.
- Production machine identity and Keychain integration are unchanged in intent,
  but were not exercised by the disposable-identity trial.

## Complexity assessment

Typed configuration is clearer and removes executable lookup, CLI launch and
OS-specific child termination from the active startup path. The adapter adds a
worker, stop channel and completion tracking. Status/invite HTTP remains because
its existing bounds and validation are useful; embedding does not eliminate it.

This draft is not a net source reduction. Packaging and old CLI helpers remain
while parity is unfinished. The Rust dependency graph is much larger. Conversely,
there is no separate engine process to package/manage once that migration is
finished. A native crash now affects the tray, and a stuck runtime can require
app restart. This is a reasonable tradeoff for a dedicated engine-hosting app,
with demonstrated local inference/restart, but not yet a shipping replacement.

## Validation

With Git dependencies on `d2fc780f6`, full `just verify` passed: format, check,
54 package tests and all-target Clippy with warnings denied. These tests do not
start Mesh or access Keychain. The local release harness and Metal build both
exited successfully; engine main emitted dependency compiler warnings during
the harness build. Do not describe that as a warning-free engine validation.
Cargo outputs were cleaned after verification; installed apps were untouched.

## Updates (not implemented)

Embedding the Rust SDK does not prevent distributing the native library
separately later. Initially it can ship and be signed with the app. SDK changes
require an app update; native runtime updates require compatibility/integrity
checks and should not assume live library replacement is supported.

Platform updater candidates remain Sparkle 2 on macOS and MSIX/App Installer
on Windows if MSIX packaging is adopted. Traditional Windows packaging needs an
external updater/helper after exit. No updater, release or notarization change
is part of this draft.
