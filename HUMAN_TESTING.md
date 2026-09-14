# Mesh candidate — read before launching

This local macOS arm64 candidate bundles the unchanged official Mesh 0.76.1
executable and native runtime. It is not an installer, signed/notarized release,
or replacement of the installed preview. **Do not double-click yet:** encrypted
private identity access may prompt for Keychain permission. That gate remains
unresolved; packaging and mock tests do not prove prompt-free operation.

## Verified without credentials or app launches

The package command checks the official archive's pinned SHA-256 and extracted
host digest, preserves the adjacent native runtime, records all packaged file
hashes and the source commit, and refuses to overwrite an existing destination.
Full Rust verification exercises fake identity stores, signed membership
transcripts, persistence and child lifecycle fixtures, not native credentials.
No bundled executable is run by packaging. No Finder file association is claimed:
use Members → Open invitation or reply once interactive testing is authorized.

## Human test procedure (after the credential gate is cleared)

Use a fresh, explicitly chosen absolute `MESH_TRAY_DATA_DIR` and unused
`MESH_LLM_CONSOLE_PORT` / `MESH_LLM_API_PORT` for each candidate instance.
Launching without a data override uses the existing `~/.mesh-tray`; do not do that
for this candidate. Never point a test at an established profile or CLI identity.
Keep the runtime next to `mesh-tray` inside Contents/MacOS. Do not move or modify
its native-runtimes directory. Start the executable directly from a terminal with
those overrides when authorized; the app does not automatically open Chat.

1. Choose Private on A and B. Wait for serving readiness; first use may download
   model weights. `ready_idle` is not evidence of inference readiness.
2. A: Members → Invite a member. Send the invitation via the native share sheet.
3. B: Members → Open invitation or reply → Accept & reply. Send the reply back.
   B's existing serving connection and grants must remain unchanged.
4. A opens B's reply. Check with your friend outside Mesh that the request is
   theirs, then Allow. A call, existing chat or in-person check is up to you.
   The optional reference code is a helper, not a required ceremony. The app binds
   approval to the exact replying device; it cannot prove the human's identity.
5. A: after restart, Members → Share reply or approval. B opens the final approval
   to join. Until that file arrives, B has not joined. Transport is manual.
6. B invites C through the same flow. Deliver B's final approval to A as well as
   C. A imports it without another pairwise approval. All remain serving.
7. Restart each owned instance. Confirm identity, earlier seeds and member grants
   survive. Send an actual chat request and record model, response and peer state.
8. Negative cases: Decline grants nothing; a forwarded invitation alone grants
   nothing; another identity cannot use B's approval; duplicate approval is
   rejected; expired invitations require a fresh exchange.

Only the latest outgoing reply/approval is retained. Finish its delivery before
starting another exchange. Shares are retained until this app exits (32 per
session). Cancel pending exchanges does not remove established member grants.

## Build and distribution decision

Build the standalone tray with `just verify` and `just build`. Package locally:

```
python3 scripts/package-preview.py target/release/mesh-tray \
  /absolute/mesh-llm-v0.76.1-aarch64-apple-darwin.tar.gz /absolute/new-candidate
```

Python 3.12+ is required for safe tar extraction. Preserve the resulting candidate
and verification logs, then `just clean`. No signing command or credential probe
is part of this process. Signing/notarization, stable credential access, installers,
update policy and Windows/Linux native testing are release decisions still to
review. Do not publish this candidate as a finished product.
