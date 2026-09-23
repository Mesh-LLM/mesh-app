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

Three fixed items. The only thing that changes is the submenu title, which
shows the last good balance once a wallet exists (read when the runtime becomes
ready, then every 20s; a failed read keeps the last value, never shows an
error or zero). Every item reads Mesh fresh when clicked, so no menu text can
go stale:

```text
Payments · <balance>        ("Payments" until a wallet exists)
  Pay…                      [x] Pay for models + daily limit (UTC day)
  Get paid…                 [x] Charge for the served model + one price per M tokens
  Add funds…                balance + Lightning invoice
```

The wallet is created lazily by Mesh (first Add funds, or when paying needs
it); there is no enable step, and the tray never creates one by reading.
Add funds takes an optional amount (blank lets the payer choose) and shows the
BOLT11 invoice as a `LIGHTNING:` QR with Copy and Check payment. Check matches
this invoice's payment hash in the wallet's transactions; balance growth is not
treated as receipt. Funding never enables spending. The tray serves one model,
so Get paid prices that model (same price for input and output). No send/withdraw UI. Forms are AppKit only;
other platforms say so.
