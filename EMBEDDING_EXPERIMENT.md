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

## Local app candidate (2026-09-22)

The local replacement removes obsolete executable lookup/CLI argument generation
and child-process test fixtures. It retains the memory-based model ladder and
first-run config setup; existing config is not rewritten. Per Mic's direction,
the former 65,536-token override is removed: context is the engine/model default
or an explicit user setting. No tray-generated configuration snapshot remains.

`just build` now builds the console in Cargo's exact resolved Git dependency
before embedding it. `package-app.py --embedded` packages the tray plus the
matching native runtime, with no child engine executable. The legacy packaging
mode remains for the existing release workflow; release automation is not yet
migrated to embedded builds.

Before creating threads, the app selects its bundled runtime and redirects
stdout/stderr to the existing mesh.log. A bounded SDK status watchdog requests
shutdown and joins the owned runtime if its management surface stops responding.
This is health supervision, not a direct runtime-exit notification: the public
SDK does not expose that notification. Uncertain shutdown still blocks restart.

Validation of the local candidate: 50 Rust package tests, format/check and
all-target Clippy with warnings denied; all 14 Python packaging tests; release
build with console assets; nested ad-hoc signing and deep strict verification.
The earlier two-cycle Metal inference proof above is prior evidence, not a live
test of this final packaged candidate. The native menu/Keychain/public-private
switch journey is for the human trial. Windows remains gated.

The installed candidate preserves `com.mesh-llm.tray.candidate`. Ad-hoc signing
is for this local trial, not notarized distribution. The app is replaced without
launching it or changing machine configuration, identities, models or trust.

## Human-launched observation and plugin correction

Mic launched the installed candidate. Its own PID 97784 reported private serving,
verified ownership and Qwen3.8 27B ready with 131072 context. `/chat` returned 200;
a text request returned "Hello, how can I help you today?" in 0.58 seconds.
The blobstore startup timed out: upstream invokes current_exe with
`--log-format json --plugin blobstore`, which the tray had not dispatched.
The follow-up build dispatches that exact built-in helper before profile lock/UI
setup, using the already-linked host-runtime entrypoint. It is a plugin process,
not a second inference node. This corrected build is staged for replacement
after Mic quits the running candidate; its plugin handshake is not yet live-tested.

## Complexity assessment

Against original base 7884735, the candidate's Rust is +346/-311, net +35 lines including tests.
Cargo.lock remains approximately +5,729 net generated lines, reported separately.
Embedding removes child ownership but native crashes now affect the tray itself.
The bounded HTTP status/invite reader remains; the SDK status method itself also
uses HTTP. A full typed management API is a separate engine change.

## Updates (not implemented)

Embedding the Rust SDK does not prevent distributing the native library
separately later. Initially it can ship and be signed with the app. SDK changes
require an app update; native runtime updates require compatibility/integrity
checks and should not assume live library replacement is supported.

Platform updater candidates remain Sparkle 2 on macOS and MSIX/App Installer
on Windows if MSIX packaging is adopted. Traditional Windows packaging needs an
external updater/helper after exit. No updater, release or notarization change
is part of this draft.
