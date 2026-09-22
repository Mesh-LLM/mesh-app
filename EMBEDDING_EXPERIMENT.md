# Embedded SDK experiment — not a release candidate

Authorized in the tray-app discussion on 2026-09-21. This branch intentionally
relaxes PRODUCT.md's prebuilt-child-only constraint to evaluate embedding; all
other product requirements remain the comparison baseline.

Base: mesh-app main `78847359fbd8169e189201d1bf14ba8a3d4bf70e`.
SDK: the same immutable engine revision already used for identity,
`d4ffbbacd9c8486e0b80c44452876ffa17785964` (reports 0.76.0).
Do not assume the existing 0.76.2 packaged native runtime is compatible.

## What changes

The menu owns an embedded worker instead of launching/signalling an engine child.
SDK configuration supplies ports, console UI, model, user config path, public
auto-join or private owner/require-owned/signed-invite requirements. Existing
bounded loopback status and invite validation remain; self PID is now the tray
PID. No process-name kills or adoption of other listeners.

Stop requests are retained during startup. The worker finishes SDK startup and
then calls `stop()`; the UI only starts a replacement after worker completion.
There is no forced thread kill. This deliberately trades immediate startup
cancellation for avoiding detached starts and overlapping replacement engines.

## Known gaps — do not ship this branch

- The merged tray's automatic model recipe requests 64K context when the user
  has not configured it. This SDK builder exposes no context override. Prototype
  fails explicitly for that case; it does not edit the user's config or quietly
  use a different context size. An SDK override or a carefully tested temporary
  config overlay is required before ordinary first-run parity.
- The runtime remains a separate dynamically loaded artifact. Packaging is NOT
  migrated: existing scripts still package a child engine. A future embedded
  package must supply a version/ABI-compatible runtime, with its signing and
  manifest rules intact. No existing installed app was replaced.
- Child stderr/stdout capture into mesh.log is gone. The engine has its own
  embedded logging foundation, but the tray's startup log diagnostics need a
  deliberate integration. Launcher messages still go to mesh.log.
- No early cancellation handle, thread health notification or bounded hard-stop
  guarantee is added upstream. A native hang can require restarting the app.
  The prototype worker currently waits for a stop request after startup; it is
  not a complete runtime-exit supervisor.
- Environment credential overrides cannot be scrubbed per-thread; prototype
  refuses startup if the two overrides the child launcher filtered are present.
- Windows product startup remains gated. Cooperative SDK shutdown removes the
  need for the Windows child TerminateProcess path, but DLL loading, GUI startup
  and restart are NOT validated on Windows.
- No real SDK startup was run: identity code can access macOS Keychain. Tests use
  pure configuration, channels and existing harmless child fixtures. Those child
  fixtures prove UI transaction behavior, not real embedded restart behavior.
- The existing native menus were not changed or clicked through.

## Initial assessment

Configuration reads more clearly as builder calls than CLI flags. Runtime
ownership is now portable and cooperative. This first adapter is not yet a net
line-count reduction: it retains the existing status/UI shape and adds a worker
bridge. The larger dependency graph is real (roughly 6K lockfile lines added).
Deleting packaging/supervision code before establishing parity would produce a
misleading comparison.

Next gate: context override, diagnostic integration, then an attended isolated
startup → stop while starting → stop → restart → private invite → public/private
switch trial. Preserve the machine identity, models, user config and other nodes.

## Updates (design candidates, not implemented)

Keep app/host and native runtime updates separate even if distributed together
initially. The SDK supports dynamic native runtime selection/install. Downloaded
runtimes must match host/ABI/platform and pass integrity verification; switching
native libraries in a live process should not be assumed supported.

- macOS: evaluate Sparkle 2 for signed app updates/appcast and restart handling:
  https://sparkle-project.org/documentation/
- Windows: evaluate MSIX + App Installer if adopting MSIX distribution:
  https://learn.microsoft.com/en-us/windows/msix/app-installer/auto-update-and-repair--overview
- A traditional Windows installer needs an external updater/helper for replacing
  running binaries. Avoid a bespoke self-overwriter unless platform mechanisms
  prove unsuitable. Tauri's updater is not a drop-in for this native winit app.

Updater selection, notarization and release publication are separate work.

## Local verification

On macOS arm64, working tree based on `7884735`: `just verify` passed (format,
check, full package tests: 37 library + 15 binary, Clippy all-targets with
warnings denied). `just build` passed, producing a stripped release executable
of approximately 63 MiB. This proves compilation/linking, not native runtime
loading or live mesh behavior. Build outputs are cleaned after verification.

## Follow-up safety review (2026-09-22)

A worker completion is **not** proof of SDK runtime termination on error. At the
pinned engine revision, `sdk.rs::shutdown_failed_embedded_startup` waits only five
seconds; `join_embedded_runtime_thread_with_timeout` then drops its thread handle
on timeout. The thread can remain alive. The previous adapter would allow Retry
or a pending settings change to start another runtime in that process.

The adapter now latches restart-unsafe after any SDK worker error or disconnected
worker. A subsequent start is refused until the whole app is restarted. This is
intentionally conservative even for errors before thread creation: the SDK does
not expose a structured termination guarantee. Normal successful stops permit
restart. This guard does not add early cancellation or make native hangs safe.

A second source gate: `git merge-base --is-ancestor 37fe1fa2404145ce9242892c12f61b283a83a931 d4ffbbacd9c8486e0b80c44452876ffa17785964`
returns 1. The current SDK pin does not contain the required durable private
membership fix. Do not package this pin as a replacement for the current tray.

Full `just verify` passed on the modified tree based on `5e9f50c`: 54 package
tests, formatting, check and warning-denying Clippy. No live engine was started.
The remaining gate is a newer immutable SDK/runtime pair, a proper context-size
SDK override and runtime-exit supervision, then packaging and isolated lifecycle
validation with a verified noninteractive credential backend. A temporary HOME
alone is not sufficient to make macOS credential access safe.
