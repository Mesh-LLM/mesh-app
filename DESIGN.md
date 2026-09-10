# Native Mesh tray

The explicit Public/Private + native-share direction supersedes the companion
webpage prototype. This is an extension of the existing native tray, not a new
visual identity. Mode: Operate.

## Implemented composition
- Existing jellyfish menu-bar icon and mutable status line.
- Public/Private check items, then Request to join, Open Mesh file, Cancel pending
  requests/replies, Share approved reply and People allowed submenu. Existing
  Chat, Settings and owned-child Quit remain below.
- macOS `NSSharingServicePicker` anchored to the status button; `NSOpenPanel` for
  a local file; `NSAlert` for explicit confirmations and verification results.
- OS fonts, colors, focus and light/dark behavior; no custom typography/chrome.
- Cancel is the default macOS confirmation button; portable default-button and
  keyboard behavior remain unverified. A verified self-claimed name is
  labeled as claimed; the owner fingerprint is separate. Verification-only
  dialogs request explicit Allow & reply / Join / Decline; merely opening grants
  nothing. People entries show a claimed label and fingerprint, not a certified name.

## States and limitations
No auto-chat. Private setup creates/reuses a stable app-only identity using OS
credential storage. Unreadable/locked/missing-established identities fail with a
recovery message, never replacement. Mode/consent mutations are refused while a
restart candidate is pending. Share picker opens only after ready private runtime
identity checks; a saved reply supports retry after cancellation/start failure.
Portable native dialogs/file save replace a universal-share-sheet assumption.
Windows runtime launch remains disabled pending safe known-folder isolation.

## Verification status
Source/API compilation and unit checks only. No app launch, share-service delivery,
screenshot, keyboard/accessibility, cold/warm Finder-open or visual verification in
this checkpoint. These are draft acceptance gates, not passed design review.
