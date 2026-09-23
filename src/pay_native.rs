//! Native Payments forms. Collects input and displays Mesh's answers only:
//! no networking, no money arithmetic beyond parsing what the user typed.

/// What the user chose on the invoice sheet.
#[derive(Debug, PartialEq, Eq)]
pub enum InvoiceAction {
    Done,
    Copy,
    Check,
}

#[cfg(target_os = "macos")]
mod mac {
    use super::InvoiceAction;
    use objc2_app_kit::{
        NSAlert, NSApplication, NSButton, NSImage, NSImageView, NSPasteboard,
        NSPasteboardTypeString, NSTextField, NSView,
    };
    use objc2_foundation::{MainThreadMarker, NSData, NSPoint, NSRect, NSSize, NSString};

    fn front(mtm: MainThreadMarker) {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
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
        if unsafe {
            pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString)
        } {
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
        front(mtm);
        (a.runModal() == 1000).then(|| f.stringValue().to_string())
    }

    /// Returns (pay automatically, daily allowance text).
    pub fn spending(enabled: bool, allowance: &str, usage: &str) -> Option<(bool, String)> {
        let mtm = MainThreadMarker::new()?;
        let a = alert(
            mtm,
            "Pay for inference",
            &format!("When on, Mesh may pay other nodes for requests it can't serve for free, up to this allowance per UTC day. Turning it off stops new paid requests; it does not cancel ones already running.\n\n{usage}"),
            &["Save", "Cancel"],
        );
        let view = NSView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 62.)),
        );
        let check = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Pay automatically when needed"),
                None,
                None,
                mtm,
            )
        };
        check.setFrame(NSRect::new(NSPoint::new(0., 36.), NSSize::new(320., 24.)));
        check.setState(if enabled { 1 } else { 0 });
        view.addSubview(&check);
        view.addSubview(&label(mtm, 0., 2., 170., "Daily allowance (sats)"));
        let f = field(mtm, 180., 2., 140., allowance);
        view.addSubview(&f);
        a.setAccessoryView(Some(&view));
        front(mtm);
        if a.runModal() != 1000 {
            return None;
        }
        Some((check.state() == 1, f.stringValue().to_string()))
    }

    /// Returns [input, output, minimum] as typed, in sats.
    pub fn price(model: &str, current: [String; 3]) -> Option<[String; 3]> {
        let mtm = MainThreadMarker::new()?;
        let a = alert(
            mtm,
            &format!("Price for {model}"),
            "Sats per million tokens. Other nodes pay this when they use your model; Mesh bills them in invoices of at least the minimum.",
            &["Save price", "Cancel"],
        );
        let view = NSView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(340., 96.)),
        );
        let mut fields = Vec::new();
        for (i, (title, value)) in [
            "Input (sats / M tokens)",
            "Output (sats / M tokens)",
            "Minimum invoice (sats)",
        ]
        .iter()
        .zip(current.iter())
        .enumerate()
        {
            let y = 68. - i as f64 * 32.;
            view.addSubview(&label(mtm, 0., y, 190., title));
            let f = field(mtm, 200., y, 140., value);
            view.addSubview(&f);
            fields.push(f);
        }
        a.setAccessoryView(Some(&view));
        a.window().setInitialFirstResponder(Some(&fields[0]));
        front(mtm);
        if a.runModal() != 1000 {
            return None;
        }
        Some([0, 1, 2].map(|i| fields[i].stringValue().to_string()))
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
        front(mtm);
        match a.runModal() {
            1001 => InvoiceAction::Copy,
            1002 => InvoiceAction::Check,
            _ => InvoiceAction::Done,
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::*;

// Other platforms: the payment controls are not built yet. Say so instead of
// pretending a form was cancelled.
#[cfg(not(target_os = "macos"))]
mod other {
    use super::InvoiceAction;
    const MSG: &str = "Payments controls are macOS-only in this build.";
    pub fn copy_text(_: &str) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn fund_amount() -> Option<String> {
        crate::native::notice("Payments", MSG);
        None
    }
    pub fn spending(_: bool, _: &str, _: &str) -> Option<(bool, String)> {
        crate::native::notice("Payments", MSG);
        None
    }
    pub fn price(_: &str, _: [String; 3]) -> Option<[String; 3]> {
        crate::native::notice("Payments", MSG);
        None
    }
    pub fn invoice(_: &str, _: &str, _: &[u8]) -> InvoiceAction {
        InvoiceAction::Done
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::*;
