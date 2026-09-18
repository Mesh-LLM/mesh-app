# Native Mesh tray

One identity per machine: the child runs as the user against `~/.mesh-llm`, and
Public/Private are flags on that node. Launcher state is `~/.mesh-app`
(`launcher.json`, `mesh.log`). Changing mode forgets the Mesh being left, so
there is no separate Start Over.

The menu structure is constructed once; Retry startup is always present.
Public/Private ticks are initialized from saved settings and synchronized only
on mode clicks and committed settings changes, never periodically while polling. Runtime guards
handle unavailable/busy actions; process supervision continues independently.

OS-native jellyfish status menu: Public/Private, Invites, optional Chat, existing
console Settings, Quit. Invites contains exactly two items — copy an invite, or
join with one. No roster, no per-person controls, no extra settings website,
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
token, adds it to the invites this node holds, and restarts.

## Honest intermediate states

Joining restarts the runtime; until it is ready the node is not in the Mesh.
Joining while already private keeps the Mesh this node is in and adds the new
code. Joining from Public is a mode change, so it forgets nothing that still
applies. Going Public forgets the code, which is not a revocation: there is no
Mesh-wide eviction, and the clean kick is re-forming the Mesh.

## Native verification still required

No final screenshots or UI click/delivery proof; the menu-bar journey is
unverified. Restarting with an expired code is untraced. The trust model itself
is proven on four identities across two Macs with the unmodified 0.76.2 runtime,
on a LAN — NAT traversal between houses is a separate, unsolved problem. Portable
native paths compile but are not Windows/Linux product verification; Windows
runtime isolation remains gated. No accessibility/keyboard/QR success claimed.
