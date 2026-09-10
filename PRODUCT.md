# Product

## Platform
Native desktop Rust tray for macOS, Windows and Linux. macOS sharing and portable
file/consent adapters are implemented; Windows runtime launch is safety-gated.

## Users and purpose
People sharing a private Mesh with friends and family, without network setup.

## Product contract
- Tray has Public or Private. Public preserves existing automatic `--auto` behavior.
- Private uses existing Mesh owner identities and Allowlist. Each node keeps its
  own authoritative admitted-owner list. No imported roster or mesh-wide authority.
- Native sharing delivers signed public requests and recipient-encrypted responses.
  Verification proves a key, not a friendship, permission, connection or model readiness.
- Every admission/removal requires an explicit local decision and effective runtime
  application. Invitations alone do not grant access.
- Keep Mesh QUIC and high-entropy invitations unchanged. No new core protocol.
- Chat is optional. Never auto-open chat on launch, file open or discovery.
- Settings opens the existing Mesh console. The rejected companion webpage and
  custom nearby TCP/Bonjour exchange are removed, not alternate supported flows.
- Preserve app data and unrelated Mesh processes. Stop only the retained child.

## Draft boundary
Implemented in source: automatic keychain-backed profile setup/reuse, shared
explicit consent/decline/join/remove transitions, replay persistence, owned-child
stop/save/project/restart, ready-only reply sharing/retry, native allowed list,
macOS share picker and portable native file/consent adapters.

Not verified: real OS credential setup, delivery, live admission/removal and native
Windows/Linux packaging. Windows runtime cannot yet isolate default trust/node
paths and launch is blocked. Safe OS file activation, service-completion cleanup,
manual-passphrase recovery and power-loss durability remain unfinished.
No end-to-end workflow has been demonstrated in a launched app yet.

## Brand
Keep the existing jellyfish artwork and ordinary OS menu/dialog conventions.
No separate settings product, wizard, webview or new visual identity.
