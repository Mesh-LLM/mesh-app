# Product

## Platform
Native desktop Rust tray. This draft implements macOS sharing only.

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
Implemented: native request share picker, local file open panel, verification-only dialogs, persistent pending requests and
cancellation, native mode selection, authoritative startup owner-list projection.

Not implemented: native owner provisioning/unlock, host approval + response share,
joiner approval + list/connection transaction, admitted-owner removal UI, reliable
service-completion cleanup, and safe OS-open integration, packaged file type registration/cold-open validation.
No native admission workflow is ready for ordinary users yet.

## Brand
Keep the existing jellyfish artwork and ordinary OS menu/dialog conventions.
No separate settings product, wizard, webview or new visual identity.
