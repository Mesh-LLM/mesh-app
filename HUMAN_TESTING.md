# Mesh Candidate — attended human trial

**Ready to try on macOS arm64.** This is a local, unsigned/not-notarized candidate,
not an installer or replacement for the installed preview. The bundled tray is
built at `41d4314` and uses the unchanged official Mesh **0.76.1** and its adjacent
native runtime. Mic has cleared the prior zero-Keychain-prompts blocker; no engine
change is required. Ordinary first-run macOS authorization may appear. If prompts
repeat or access fails, cancel and Quit this candidate; do not repeatedly Retry,
change ACLs, export credentials, or delete identities.

## Launch one candidate, deliberately

Use two Macs for A/B, and a third for C if available (8+ GiB RAM each; 24+ GiB
selects a larger model). First start downloads weights into the candidate's own
profile; allow disk space, network and download time. A separate profile does not
isolate GPU/RAM: do not run three serving candidates on a busy Mac. Leave any
existing preview/live Mesh instance alone. This app never automatically opens Chat.

In Terminal, paste the following **once per test Mac**, adjusting `CANDIDATE` to
the folder containing this guide and bundle. This creates a new named test profile
and starts directly in Private, avoiding a temporary Public connection. No existing
profile is overwritten. If a port is occupied, the tray reports it and leaves its
service alone; choose different ports rather than stopping that service.

```sh
CANDIDATE="$HOME/.buzz/OUTBOX/MESH_TRAY_CANDIDATE_41D4314"
PROFILE="$HOME/Library/Application Support/Mesh Candidate Trial A"
(
  umask 077
  mkdir -p "$(dirname "$PROFILE")"
  mkdir "$PROFILE" || exit 1
  printf '%s\n' '{"connection":{"mode":"private","invite":null}}' > "$PROFILE/launcher.json"
  env MESH_TRAY_DATA_DIR="$PROFILE" \
    MESH_LLM_CONSOLE_PORT=33232 MESH_LLM_API_PORT=39447 \
    MESH_LLM_BIN="$CANDIDATE/Mesh Candidate.app/Contents/MacOS/mesh-llm" \
    "$CANDIDATE/Mesh Candidate.app/Contents/MacOS/mesh-tray"
)
```

Keep the terminal open. The jellyfish appears in the menu bar (no main window).
If macOS blocks opening an unsigned downloaded app, use the normal macOS
Privacy & Security review only if you trust this artifact; do not strip quarantine
or re-sign binaries as a workaround. Bundle and profile paths must stay stable.
Do not double-click for this trial: that omits overrides and uses `~/.mesh-tray`.
Do not move `native-runtimes` away from the bundled executables.

For B/C on separate Macs use names `Trial B`/`Trial C` in `PROFILE`. If intentionally
sharing a Mac, each needs different console/API ports as well as a different profile.
Stop only your candidate via its menu → Quit. To **restart the same identity**,
set the same `CANDIDATE` and `PROFILE` variables and rerun just the `env ...`
command above, with the same ports; do not repeat `mkdir`/`printf` and do not delete
anything. Do not point this candidate at an established preview or CLI profile.

## Invite → reply → Allow → deliver approval

Complete each full exchange, including delivery to existing members, **within
30 minutes of creating its invitation**. Even final approval imports currently
expire at that deadline. Only the latest outgoing reply/approval is retained.

1. A/B: wait for Private startup. Members → Invite a member requires a verified
   owned private runtime; model download may still be pending. Settings opens the
   existing console. `ready_idle` is not inference readiness; wait for a real model
   before testing Chat. Inspect `$PROFILE/mesh.log` if the console is unavailable.
2. A: Members → **Invite a member** → Create invitation. Choose an available native
   share service and send the file to B. Save received files locally. No automatic
   messaging, notification, link/QR or Finder file association is implemented.
3. B: Members → **Open invitation or reply** → select A's file → **Accept & reply**.
   Send the reply to A using the share picker. Acceptance alone must not change B's
   serving connection, seeds or grants. If delivery was cancelled, Members →
   **Share reply or approval** reopens the picker; do not create another exchange.
4. A: open B's reply through that same menu. Optionally check with B outside Mesh
   (call, existing chat or in person), best efforts. The reference code is optional,
   not a ceremony. Choose **Allow** for the displayed exact device identity, or
   Decline. A claimed human name is not proof. Cancel is the default and decides
   nothing; Decline consumes the invitation and grants nothing.
5. Allow restarts only A's runtime to apply its new grant. Once ready, A must
   explicitly use **Share reply or approval** and send the final file to B. B opens
   it through Members to join, restarting only B's runtime. Until it arrives, B has
   not joined. Confirm both list each other's identity under Members and still serve.
6. B invites C by repeating steps 2–5. B sends that final approval to **both C and A**.
   A imports it without a second pairwise Allow dialog. C learns the signed existing
   roster, and A admits C via trusted B. There is no automatic roster propagation;
   deliver each new approval to all existing members. All retain serving.
7. Restart owned candidates using their same profiles. Confirm identities, earlier
   seeds and member grants survive. Open **Chat** explicitly, select a real available
   model, and send “Reply with a short greeting.” Record model, response and peer
   state in Settings. A local response alone does not prove remote inference; record
   which node served it if the console exposes that information.
8. Try negative cases using separate fresh invitations: Decline grants nothing;
   a forwarded invitation alone grants nothing; an unapproved identity cannot use
   B's final approval; duplicate imports are rejected; expired files require a new
   complete exchange. Finish delivering the positive trial before negative cases.

Native share-service availability/delivery is part of this trial, not already
verified. If no suitable service appears or delivery fails, report that exact step;
do not assume Mesh sent it. Shares remain until app exit (32 per session). Quit and
restart to reset the share limit; saved replies/approvals survive. Cancel pending
exchanges discards pending work, not established member grants.

## What to report / what is already checked

Report A/B/C hardware, macOS, step reached, exact error, model readiness and response,
member/peer state before and after restart, and whether authorization repeated.
Review/redact logs before sharing: do not send keystores, private profile contents,
credentials or invitation tokens. Preserve profiles for restart testing; no wiping
or automated cleanup of identities/models is part of this guide.

Noninteractive verification: full Rust tests with fake identity stores, signed
membership/persistence/lifecycle fixtures, fmt/check/Clippy `-D warnings`; package
failure tests and all 33 bundle hashes. Packaging never runs either executable.
The corrected live three-node flow, native share delivery and actual inference are
**human acceptance checks still to do**, not claimed successes. This review found
no additional state-machine code prerequisite for this manual journey.

## Rebuild / packaging

`just verify`, `just build`, then (Python 3.12+):

```sh
python3 scripts/package-preview.py target/release/mesh-tray \
  /absolute/mesh-llm-v0.76.1-aarch64-apple-darwin.tar.gz /absolute/new-candidate
just clean
```

Packaging verifies pinned archive/host digests, preserves native runtime layout,
records the source commit and file hashes in `SHA256.json`, and refuses an existing
destination. This handoff updates instructions only; the existing bundle and
manifest remain at `41d4314`. Signing/notarization, installer/update distribution,
Windows/Linux native validation and automatic file transport remain future work,
not blockers to this attended macOS trial. Never publish this as a finished release.
