# Native Mesh tray

One identity per machine: the child runs as the user against `~/.mesh-llm`, and
Public/Private are flags on that node. Launcher state is `~/.mesh-app`
(`launcher.json`, `mesh.log`). Changing mode forgets the Mesh being left, so
there is no separate Start Over.

The menu structure is constructed once; Retry startup is always present.
Public/Private ticks are initialized from saved settings and synchronized only
on mode clicks and committed settings changes, never periodically while polling. Runtime guards
handle unavailable/busy actions; process supervision continues independently.

OS-native jellyfish status menu: Public/Private, Invites, Payments, Chat, Retry
startup, Quit. Invites contains exactly two items — copy an invite, or
join with one. Payments is a fixed submenu (see below). No roster, no per-person controls, no extra settings website,
wizard, Buzz UI/runtime, custom typography or webview.

## One code, both directions

Private launches `--owner-required --trust-policy require-owned`, with
`--min-node-version` when this node creates the Mesh and `--join <code>` when it
joins one. Under `require-owned` the engine consults no list: a peer carrying a
valid owner attestation and the same signed Mesh policy is trusted, mutually
(`mesh/ownership.rs:355-416`). So membership *is* the Mesh, and the invite is the
membership.

The version floor is the whole reason a requirement is declared at all: a
requirement-aware Mesh has a policy-derived ID and a signed 24-hour bootstrap
token that only the origin owner's key can sign (`mesh/node_identity.rs:106-185`).
An unrestricted Mesh has neither, and its invite falls back to an unsigned
address token. A joiner must not declare the floor — it inherits the policy from
the token it pastes.

Copy an invite reads `token` from the running child's own status endpoint, after
checking that the child is this app's PID, private, and reporting a verified
owner. An empty token is the engine declining to emit a legacy code
(`node_identity.rs:181-185`); that is the one actionable case and it is reported
as "ask the person who invited you for a new one". Join validates the pasted
token, replaces the selected invite, and restarts.

## Honest intermediate states

Joining restarts the runtime; until it is ready the node is not in the Mesh.
Joining while already private replaces the selected Mesh with the new
code. Joining from Public is a mode change, so it forgets nothing that still
applies. Going Public forgets the code, which is not a revocation: there is no
Mesh-wide eviction, and the clean kick is re-forming the Mesh.

## Native verification still required

No final screenshots or UI click/delivery proof; the menu-bar journey is
unverified for a particular packaged candidate. The trust model itself
is proven on four identities across two Macs with the unmodified 0.76.2 runtime,
on a LAN — NAT traversal between houses is a separate, unsolved problem. Portable
native paths compile but are not Windows/Linux product verification; Windows
runtime isolation remains gated. No accessibility/keyboard/QR success claimed.

Shutdown signals only the retained child: SIGTERM on macOS/Linux, forced
termination through the Child handle on Windows. Windows graceful shutdown
and product support remain deferred; no HTTP shutdown endpoint is assumed.

## Durable private restarts

With an engine containing [mesh-llm #1896](https://github.com/Mesh-LLM/mesh-llm/pull/1896)
(merged as `37fe1fa2404145ce9242892c12f61b283a83a931`), invite expiry alone does
not prevent an admitted member from restarting. The tray retains the selected
invite in `launcher.json`; the engine restores matching verified membership from
`mesh-adopted-membership.json`, including saved peer addresses. The owner restores
`mesh-genesis-policy.json`. These engine files are not the removed tray seeds list.
Missing/corrupt membership, failed persistence, deliberately leaving the Mesh, or
unreachable peers can still prevent rejoining. Fresh nodes cannot use an expired
invite through this normal join path.

The earlier engine live matrix covered expired-invite restart, owner restart,
owner-offline re-formation and fresh-node rejection. Candidate-specific UI checks
remain distinct from that engine evidence. See the merged PR for implementation.

Packaging selects one engine product: a pinned source commit or a numbered
release containing the required fixes, with its matching native runtime. The
preview packager's historical official-release pins are still 0.76.2, which
predates #1896; its explicit source option supports newer source candidates.
There is no runtime choice between two engines and no version setting for users.

## Payments

A thin controller over Mesh's wallet (mesh-llm #1926). Mesh owns the wallet,
ledger, prices, budget and settlement; the tray sends local operator API
requests (`POST /api/wallet`, pinned to the retained runtime's PID) from a
worker thread. The SDK has no typed wallet operations yet; when it does, the
calls move there and the menu does not change.

Three fixed actions. The submenu title shows the balance only after a successful
read with a wallet present; otherwise it is plain “Payments”. A failed refresh
adds a disabled “Balance unavailable” item inside the submenu, removed after
recovery. Reads happen when ready, then every 10s. The last good value is retained
internally, never presented as a fresh balance after a failed read. Each action
reads Mesh fresh when clicked.

```text
Payments · <balance>         (plain “Payments” while unknown or unavailable)
  Pay…                      [x] Pay for models + daily limit (UTC day)
  Get paid…                 [x] Charge for the served model + one price per M tokens
  Add funds…                balance + Lightning invoice
  Balance unavailable       (disabled, only after a failed read)
```

The wallet is created lazily by Mesh (first Add funds, or when paying needs
it); there is no enable step, and the tray never creates one by reading.
Add funds takes an optional amount (blank lets the payer choose) and shows the
BOLT11 invoice as a `LIGHTNING:` QR with Copy and Check payment. Check matches
this invoice's payment hash in the wallet's transactions; balance growth is not
treated as receipt. Funding never enables spending. The tray serves one model,
so Get paid prices that model (same price for input and output). No send/withdraw UI. Forms are AppKit only;
other platforms say so.

## Share compute

One persisted launcher preference, defaulting on to preserve existing behaviour.
The macOS menu row uses an AppKit NSSwitch forwarding to the existing muda
menu action; other platforms use a checked item. No model picker or payment
policy is added. The switch reflects the saved serving preference, not proof
that a model is healthy.

Changing it cooperatively stops the retained embedded engine, waits for exit,
saves the preference, and restarts via the SDK serve/client entry point. Off
skips automatic model selection and uses client mode, not an empty serve-model
list (which could still load configured models). Connection, invites, identity,
ports and payment policy are unchanged. Active requests can be interrupted by
the restart. No live engine or Keychain-touching validation is automated here;
menu interaction, accessibility and actual unload/rejoin need a human trial.

## Explicit Reset settings

Reset settings replaces Retry startup. This supersedes the earlier “no separate
Start Over” design: Mic requested a genuinely fresh private membership boundary.
It first disables paying, removes model prices and verifies both via the owned
runtime's API. Then it requests shutdown and waits for successful completion,
including past the warning deadline. An error is not evidence of shutdown.

Only after a successful stop, Reset removes `mesh-id`,
`mesh-genesis-policy.json`, `mesh-adopted-membership.json` and `last-mesh`.
The owner keystore, ownership attestation, wallet/balance/history, node key,
engine config and models are not deleted by this operation. The engine itself
may rotate its transport identity on public/private transitions. Launcher
settings become Public, compute on, ports 3232/9447, with no saved invite.
The next originated private mesh gets a new genesis policy; accepting a new
invite adopts that invitation's mesh instead. This does not evict members from
the old mesh or exclude anyone from public service.

A `private-reset-pending` marker is written before removal and retained until
launcher defaults are saved. Startup refuses to run with that marker; Reset
can resume retirement without a running engine. File-type errors fail closed.
Custom node-key namespaces are not supported by
Reset: the pinned SDK does not export its state-directory resolver. Windows Reset is unavailable because this engine does not hold runtime locks there. Reset also
refuses a detected live runtime lock; users must keep other CLI/Buzz engines
stopped during this shared-profile operation. There is no cross-application
profile transaction lock in the pinned SDK, so concurrent external startup is
not supported.

Quit during a pending stop cancels the queued restart and waits for shutdown;
it does not force-kill other processes. Payment errors may leave some preferences
disabled, but never claim to have completed the reset.

## Unexpected embedded exit

The tray schedules at most one automatic restart per app launch, after three
seconds. Quit cancels it; intentional settings stops do not schedule it. The
existing SDK safety guard still refuses replacement when worker failure leaves
runtime exit unproven. An in-process abort kills the tray too and cannot be
recovered by this timer.

## Installed wallet providers

The pinned engine includes payment policy/ledger support, but no built-in Lexe
SDK or `wallet-lexe` re-exec. The app only dispatches the built-in blobstore;
installed `wallet.v1` providers run as their own executables, managed by Mesh.
Provider discovery uses the shared user plugin store, not app bundle resources.
See README for installation and the fresh-profile restriction of Lexe v0.1.0.
No wallet migration or silent provider fallback is performed by the app.
