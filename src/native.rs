//! AppKit transport only: no networking and no admission side effects.
//! Inherit native menus, sheets and dialogs; preserve Mesh's jellyfish identity.
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAlert, NSApplication, NSBackingStoreType, NSEvent, NSPasteboard, NSPasteboardTypeString,
    NSSharingServicePicker, NSTextField, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString,
};

/// Put an invite on the clipboard so it can be pasted into any chat app. Replaces
/// the clipboard's contents, which is what a "Copy" action is expected to do.
pub fn copy_text(text: &str) -> Result<(), String> {
    let _ = MainThreadMarker::new().ok_or("Copying must run on the main thread")?;
    let pasteboard = NSPasteboard::generalPasteboard();
    unsafe {
        pasteboard.clearContents();
        if !pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) {
            return Err("macOS refused to put the invite on the clipboard.".into());
        }
    }
    Ok(())
}

/// Hand an invite to the macOS share sheet (`NSSharingServicePicker`), so it can
/// go out through Messages, Mail, AirDrop, Notes, etc. without a copy/paste step.
///
/// The picker must anchor to a rect in a live `NSView`, and the tray has no
/// window of its own (it is winit + muda + tray-icon). So we open a 1x1,
/// fully transparent window at the mouse location purely as an anchor. The
/// popover attaches to it; the window itself is never seen. We deliberately
/// leak the anchor window and the picker: the popover runs asynchronously after
/// this call returns, and dropping either would tear the sheet down mid-display.
/// One share leaks two tiny AppKit objects — acceptable for an occasional,
/// user-initiated action.
pub fn share_text(text: &str) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("Sharing must run on the main thread")?;

    let string = NSString::from_str(text);
    let item: &AnyObject = &string;
    let items = NSArray::from_slice(&[item]);

    // Anchor at the pointer; a 1x1 rect keeps the popover next to the click.
    let loc = NSEvent::mouseLocation();
    let frame = NSRect::new(loc, NSSize::new(1.0, 1.0));
    let (window, view) = unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window.setAlphaValue(0.0);
        // A tray click does not activate an accessory app; without this the
        // popover opens behind the frontmost window.
        foreground_dialog(mtm);
        window.makeKeyAndOrderFront(None);
        let view = window
            .contentView()
            .ok_or("macOS did not give the share anchor a view")?;
        (window, view)
    };

    let picker = unsafe { NSSharingServicePicker::initWithItems(mtm.alloc(), &items) };
    picker.showRelativeToRect_ofView_preferredEdge(view.bounds(), &view, NSRectEdge::MinY);

    // Keep the anchor and picker alive past this frame (see the note above).
    std::mem::forget(window);
    std::mem::forget(picker);
    Ok(())
}

/// Read pasted text. Anything else on the clipboard reads as "nothing pasted".
pub fn paste_text() -> Result<String, String> {
    let _ = MainThreadMarker::new().ok_or("Pasting must run on the main thread")?;
    let pasteboard = NSPasteboard::generalPasteboard();
    let text = unsafe { pasteboard.stringForType(NSPasteboardTypeString) }
        .ok_or("The clipboard has no text on it. Copy the invite they sent, then try again.")?;
    Ok(text.to_string())
}

// Tray clicks do not necessarily activate an accessory app. Bring user-requested
// dialogs forward without changing the tray-only activation policy.
fn foreground_dialog(mtm: MainThreadMarker) {
    // Match winit's activation path, including support for older macOS versions.
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

pub fn notice(title: &str, detail: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str("Close"));
    foreground_dialog(mtm);
    alert.runModal();
}

/// Ask for an invite in a window with a field in it, prefilled from the
/// clipboard when there is one on it, so the usual case is paste-already-done.
pub fn prompt_card(title: &str, detail: &str, action: &str) -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str(action));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(360.0, 24.0));
    let field = NSTextField::initWithFrame(mtm.alloc::<NSTextField>(), frame);
    if let Ok(text) = paste_text() {
        if mesh_tray::settings::looks_like_invite(&text) {
            field.setStringValue(&NSString::from_str(text.trim()));
        }
    }
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));
    foreground_dialog(mtm);
    if alert.runModal() != 1000 {
        return None;
    }
    Some(field.stringValue().to_string())
}

pub fn confirm(title: &str, detail: &str, action: &str) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    // Cancel is the default; Return must never accidentally consent.
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.addButtonWithTitle(&NSString::from_str(action));
    foreground_dialog(mtm);
    alert.runModal() == 1001
}

/// AppKit tracks open menus in this run-loop mode. Defer live menu writes
/// until tracking ends; polling and process supervision can keep running.
pub fn menu_is_tracking() -> bool {
    objc2_foundation::NSRunLoop::mainRunLoop()
        .currentMode()
        .is_some_and(|mode| *mode == *unsafe { objc2_app_kit::NSEventTrackingRunLoopMode })
}
