//! AppKit transport only: no networking and no admission side effects.
//! Inherit native menus, sheets and dialogs; preserve Mesh's jellyfish identity.
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAlert, NSApplication, NSBackingStoreType, NSButton, NSEvent, NSImage, NSImageView,
    NSPasteboard, NSPasteboardTypeString, NSSharingServicePicker, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSData, NSPoint, NSRect, NSRectEdge, NSSize, NSString,
};

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

// Payments forms. They collect input and show Mesh's answers only: no
// networking and no money arithmetic beyond what the user typed.

/// What the user chose on the invoice sheet.
#[derive(Debug, PartialEq, Eq)]
pub enum InvoiceAction {
    Done,
    Copy,
    Check,
}

fn alert(
    mtm: MainThreadMarker,
    title: &str,
    detail: &str,
    buttons: &[&str],
) -> objc2::rc::Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    for b in buttons {
        alert.addButtonWithTitle(&NSString::from_str(b));
    }
    alert
}

fn field(
    mtm: MainThreadMarker,
    x: f64,
    y: f64,
    w: f64,
    value: &str,
) -> objc2::rc::Retained<NSTextField> {
    let f = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, 24.)),
    );
    f.setStringValue(&NSString::from_str(value));
    f
}

fn label(
    mtm: MainThreadMarker,
    x: f64,
    y: f64,
    w: f64,
    text: &str,
) -> objc2::rc::Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 24.)));
    l
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let _ = MainThreadMarker::new().ok_or("Copying must run on the main thread")?;
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    if unsafe { pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) } {
        Ok(())
    } else {
        Err("macOS refused the clipboard write".into())
    }
}

/// One optional amount. `Some("")` means "let the payer choose".
pub fn fund_amount() -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let a = alert(
        mtm,
        "Add funds",
        "Amount in sats, or leave blank to let the paying wallet choose.\n\nMesh creates a Bitcoin Lightning invoice. Pay it from any Lightning wallet, or an exchange that supports Lightning withdrawals. Do not send on-chain bitcoin.",
        &["Create invoice", "Cancel"],
    );
    let f = field(mtm, 0., 0., 240., "");
    f.setPlaceholderString(Some(&NSString::from_str("e.g. 5000 (optional)")));
    a.setAccessoryView(Some(&f));
    a.window().setInitialFirstResponder(Some(&f));
    foreground_dialog(mtm);
    (a.runModal() == 1000).then(|| f.stringValue().to_string())
}

/// One on/off checkbox and one amount. Returns (checked, amount text).
pub fn toggle_amount(
    title: &str,
    detail: &str,
    check_label: &str,
    checked: bool,
    field_label: &str,
    value: &str,
) -> Option<(bool, String)> {
    let mtm = MainThreadMarker::new()?;
    let a = alert(mtm, title, detail, &["Save", "Cancel"]);
    let view = NSView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(340., 62.)),
    );
    let check = unsafe {
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(check_label), None, None, mtm)
    };
    check.setFrame(NSRect::new(NSPoint::new(0., 36.), NSSize::new(340., 24.)));
    check.setState(if checked { 1 } else { 0 });
    view.addSubview(&check);
    view.addSubview(&label(mtm, 0., 2., 190., field_label));
    let f = field(mtm, 200., 2., 140., value);
    view.addSubview(&f);
    a.setAccessoryView(Some(&view));
    foreground_dialog(mtm);
    if a.runModal() != 1000 {
        return None;
    }
    Some((check.state() == 1, f.stringValue().to_string()))
}

pub fn invoice(detail: &str, bolt11: &str, png: &[u8]) -> InvoiceAction {
    let Some(mtm) = MainThreadMarker::new() else {
        return InvoiceAction::Done;
    };
    let a = alert(
        mtm,
        "Lightning invoice",
        detail,
        &["Done", "Copy invoice", "Check payment"],
    );
    let side = 260.;
    let view = NSView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(side, side + 30.)),
    );
    let data = NSData::with_bytes(png);
    if let Some(image) = NSImage::initWithData(mtm.alloc(), &data) {
        let iv = NSImageView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0., 30.), NSSize::new(side, side)),
        );
        iv.setImage(Some(&image));
        view.addSubview(&iv);
    }
    let short = if bolt11.len() > 36 {
        format!("{}…{}", &bolt11[..20], &bolt11[bolt11.len() - 12..])
    } else {
        bolt11.to_string()
    };
    let text = label(mtm, 0., 0., side, &short);
    text.setSelectable(true);
    view.addSubview(&text);
    a.setAccessoryView(Some(&view));
    foreground_dialog(mtm);
    match a.runModal() {
        1001 => InvoiceAction::Copy,
        1002 => InvoiceAction::Check,
        _ => InvoiceAction::Done,
    }
}
