# Native Mesh tray

The explicit Public/Private + native-share direction supersedes the companion
webpage prototype. This is an extension of the existing native tray, not a new
visual identity. Mode: Operate.

## Implemented composition
- Existing jellyfish menu-bar icon and mutable status line.
- Public/Private check items, then Request to join, Open Mesh file, Cancel pending
  requests. Existing Chat, Settings and owned-child Quit remain below.
- macOS `NSSharingServicePicker` anchored to the status button; `NSOpenPanel` for
  a local file; `NSAlert` for explicit confirmations and verification results.
- OS fonts, colors, focus and light/dark behavior; no custom typography/chrome.
- Cancel is the default confirmation button. A verified self-claimed name is
  labeled as claimed; the owner fingerprint is separate. Verification-only
  dialogs explicitly say approval/join is unavailable and nothing was granted.

## States and limitations
No auto-chat. Missing/unreadable/encrypted private-profile identity produces an
explicit blocker rather than generating another identity or reading the user's
CLI keys. Mode changes and request actions are refused during a pending restart.
Other platforms expose no working native-sharing claim in this draft.

## Verification status
Source/API compilation and unit checks only. No app launch, share-service delivery,
screenshot, keyboard/accessibility, cold/warm Finder-open or visual verification in
this checkpoint. These are draft acceptance gates, not passed design review.
