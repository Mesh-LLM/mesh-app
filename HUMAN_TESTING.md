# Mesh Candidate — attended human trial

**Ready to try on macOS arm64.** This is a local, unsigned/not-notarized candidate,
not an installer or replacement for the installed preview. The bundled tray is
built on `tray/bearer-invite` and uses the unchanged official Mesh **0.76.2** and its adjacent
native runtime. Mic has cleared the prior zero-Keychain-prompts blocker; no engine
change is required. Ordinary first-run macOS authorization may appear. If prompts
repeat or access fails, cancel and Quit this candidate; do not repeatedly Retry,
change ACLs, export credentials, or delete identities.

## Launch one candidate, deliberately

Use two Macs for A/B, and a third for C if available (8+ GiB RAM each; 24+ GiB
selects a larger model). First start reuses the normal user Hugging Face
cache (`~/Library/Caches/huggingface` on macOS) and downloads only missing
weights there, so a model you already have costs no download. A separate profile does not
isolate GPU/RAM: do not run three serving candidates on a busy Mac. Leave any
existing preview/live Mesh instance alone. This app never automatically opens Chat.

In Terminal, paste the following **once per test Mac**, adjusting `CANDIDATE` to
the folder containing this guide and bundle. This creates a new named test profile
and starts directly in Private, avoiding a temporary Public connection. No existing
profile is overwritten. If a port is occupied, the tray reports it and leaves its
service alone; ports 3232/9447 are fixed, so free them before starting.

```sh
CANDIDATE="$HOME/.buzz/OUTBOX/MESH_TRAY_CANDIDATE_41D4314"
PROFILE="$HOME/.mesh-app"
(
  umask 077
  mkdir "$PROFILE" || exit 1
  printf '%s\n' '{"connection":{"mode":"private","invite":null}}' > "$PROFILE/launcher.json"
  "$CANDIDATE/Mesh Candidate.app/Contents/MacOS/mesh-tray"
)
```

Keep the terminal open. The jellyfish appears in the menu bar (no main window).
If macOS blocks opening an unsigned downloaded app, use the normal macOS
Privacy & Security review only if you trust this artifact; do not strip quarantine
or re-sign binaries as a workaround. Bundle and profile paths must stay stable.
Do not move `native-runtimes` away from the bundled executables.

One profile per Mac: `~/.mesh-app`. Two candidates cannot share a Mac, because
the profile path and the ports are fixed.
Stop only your candidate via its menu → Quit. To **restart the same identity**,
launch the app again; do not repeat `mkdir`/`printf` and do not delete anything.

## Invite → paste → in

There is one code and it travels one way. Nothing comes back, nothing is
confirmed, and there is no list to inspect afterwards — the console shows peers.

1. A/B: wait for Private startup. Copy an invite needs a verified owned private
   runtime, so it is refused until the menu says Ready; model download may still
   be pending. Open Chat opens the existing chat UI (or startup details on failure).
   `ready_idle` is not inference
   readiness; wait for a real model before testing Chat. Inspect
   `$PROFILE/mesh.log` if the console is unavailable.
2. A: **Invites → Copy an invite.** Send the clipboard text to B any way you
   like. It should be a long single-line token (~2,200 characters) with no
   spaces. Report anything shorter, and do not paste it into this report.
3. B: **Invites → Join with an invite**, paste, Join. B restarts. Once ready,
   confirm in B's console that A is a peer and `owner.status` is verified — and
   that A's console shows B the same way, with neither of you having approved
   anything.
4. C (third Mac, or a third identity): join with **the same code A sent B**, not
   a new one. Confirm C and B see each other, having never exchanged anything.
   This is the claim that matters; report it if it fails.
5. B: **Invites → Copy an invite.** It should be byte-identical to A's code —
   B is forwarding, not issuing. Confirm B's dialog says so.
6. Use each other: on B, open **Chat**, select a model that only C is serving,
   and send "Reply with a short greeting." Record which node served it if the
   console exposes that. A local answer alone proves nothing.
7. Restart every candidate on its same profile. Confirm identities and Mesh
   membership survive, and that nobody had to paste anything again.
8. Negative cases, last: going Public on B and back to Private should leave B in
   its **own** Mesh with nobody in it, not back in A's. After 24 hours, A's old
   code should be refused for a new joiner while existing members keep working,
   and B's Copy an invite should say to ask A for a new one. Both of these are
   untested by us — report exactly what you see.

**Do not paste an invite code into a report, a log excerpt or a chat channel.**
Within its 24 hours it is membership: anyone holding it can join and can then use
every machine in the Mesh.
## What to report / what is already checked

Report A/B/C hardware, macOS, step reached, exact error, model readiness and response,
peer state before and after restart, and whether authorization repeated.
Review/redact logs before sharing: do not send keystores, private profile contents,
credentials or invite codes. Preserve profiles for restart testing; no wiping
or automated cleanup of identities/models is part of this guide.

Noninteractive verification: full Rust tests with fake identity stores, launch-flag
and persistence fixtures, fmt/check/Clippy `-D warnings`; package failure tests and
the bundle hashes. Packaging never runs either executable. The trust model is
separately proven on four identities across two Macs with this same unmodified
runtime, on a LAN. **Still to do, not claimed:** the menu-bar journey clicked end
to end, restarting with an expired code, and NAT traversal between networks.

## Rebuild / packaging

`just verify`, `just build`, then (Python 3.12+):

```sh
python3 scripts/package-preview.py target/release/mesh-tray \
  /absolute/mesh-llm-v0.76.2-aarch64-apple-darwin.tar.gz /absolute/new-candidate
just clean
```

Packaging verifies pinned archive/host digests, preserves native runtime layout,
records the source commit and file hashes in `SHA256.json`, and refuses an existing
destination.

If the tray binary needs an ad-hoc signature for local running, sign
`target/release/mesh-tray` **before** packaging. Never run `codesign` on the
assembled `.app`: it rewrites the bundled native-runtime dylibs, and the runtime
then refuses to start with `native runtime file checksum mismatch` against its own
`manifest.json`.

This handoff updates instructions only; the existing bundle and
manifest remain at `41d4314`. Signing/notarization, installer/update distribution,
Windows/Linux native validation and automatic file transport remain future work,
not blockers to this attended macOS trial. Never publish this as a finished release.

## Start over

There is no Start Over item: switching Mesh is starting over. Choosing Public
stops the runtime and forgets the code that put you in the Mesh; choosing Private
starts your own Mesh with nobody in it. Both ask first. Neither removes you for
anyone else — there is no Mesh-wide eviction — so rejoining means pasting a code
again.
Your machine identity, downloaded models, engine config, other Mesh nodes and
Buzz data are left alone, and ordinary quit and restart keeps your pairings.
