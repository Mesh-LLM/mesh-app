# Native Mesh tray

OS-native jellyfish status menu: Public/Private, Members, optional Chat, existing
console Settings, Quit. Members contains Invite, Open invitation/reply, Share reply
or approval, legacy pending-exchange controls, and current member fingerprints.
No extra settings website, wizard, Buzz UI/runtime, custom typography or webview.

## Identity-bound consent

Invite → Accept & reply → human out-of-band check → Allow/Decline → deliver approval.
Cancel is the first/default macOS button. Accept only persists the signed reply,
not admission or a mode switch. Allow signs the exact recipient and consumes the
local pending invitation; Decline consumes it without granting. Existing members
trust a member's final approval transitively; no Mic↔Oli approval after Jo admits Oli.

The matching code is 80 bits of the canonical signed acceptance transcript,
displayed in five four-hex groups as an optional helper, not a required ceremony.
Human identity checks are best-effort outside Mesh;
identities and self-claimed names do not establish human identity on their own.

A file-open operation verifies signatures, time, correlation and replay before
changing settings. Runtime-changing decisions retain the stop/reap → save →
project → start transaction. Final approval sharing waits for fresh owned private
runtime readiness. Native sharing retains temporary files until app exit.

## Honest intermediate states

Acceptance: waiting for inviter verification; serving unchanged. Approved locally:
restart then share final approval. Received approval: join while preserving original
seeds/models/members. Existing members: import final grant without pairwise dialog.
Transport is manual files, not automatic notification; only latest outgoing
membership artifact retained. Expired replies require a fresh invitation.

## Native verification still required

No final screenshots or UI click/delivery proof. Screen capture failed in this
session. macOS credential prompts were observed; interactive validation stopped
on Mic's instruction. No preview replacement before this is resolved. Portable
native paths compile but are not Windows/Linux product verification; Windows
runtime isolation remains gated. No accessibility/keyboard/QR/Finder-registration
or share-completion-cleanup success claimed.
