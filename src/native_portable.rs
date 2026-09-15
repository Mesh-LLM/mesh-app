//! Native file and consent fallback; no networking or admission decisions here.
#[derive(Default)]
pub struct Native {
    _private: (),
}
impl Native {
    pub fn share(
        &mut self,
        file: tempfile::NamedTempFile,
        _: &tray_icon::TrayIcon,
    ) -> Result<(), String> {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save Mesh file to send in your chat app")
            .set_file_name("Private Mesh.meshfile")
            .save_file()
        else {
            return Ok(());
        };
        // Read our own bounded staging file; save only public or recipient-encrypted data.
        let bytes = mesh_tray::share_file::read(file.path())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}
/// GTK owns the clipboard on the platform this adapter actually ships on; the
/// adapter is compiled on macOS only to keep the two APIs in step, and macOS
/// uses `NSPasteboard` in `native.rs`.
#[cfg(target_os = "linux")]
pub fn copy_text(text: &str) -> Result<(), String> {
    gtk::prelude::GtkClipboardExtManual::set_text(
        &gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD),
        text,
    );
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn paste_text() -> Result<String, String> {
    gtk::prelude::GtkClipboardExtManual::wait_for_text(&gtk::Clipboard::get(
        &gtk::gdk::SELECTION_CLIPBOARD,
    ))
    .map(|text| text.to_string())
    .ok_or_else(|| {
        "The clipboard has no text on it. Copy the card they sent, then try again.".into()
    })
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn copy_text(_: &str) -> Result<(), String> {
    Err("Copying is not available on this platform. Send the card as a file instead.".into())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn paste_text() -> Result<String, String> {
    Err("Pasting is not available on this platform. Open the card as a file instead.".into())
}
pub fn notice(title: &str, detail: &str) {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .show();
}
/// Linux has a real field; elsewhere fall back to reading the clipboard, which
/// is what the caller did before this dialog existed.
#[cfg(target_os = "linux")]
pub fn prompt_card(title: &str, detail: &str, action: &str) -> Option<String> {
    use gtk::prelude::*;
    let dialog = gtk::Dialog::with_buttons(
        Some(title),
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        &[
            (action, gtk::ResponseType::Accept),
            ("Cancel", gtk::ResponseType::Cancel),
        ],
    );
    let entry = gtk::Entry::new();
    if let Ok(text) = paste_text() {
        if text.contains("MESH1") {
            entry.set_text(&text);
        }
    }
    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_border_width(12);
    content.add(&gtk::Label::new(Some(detail)));
    content.add(&entry);
    dialog.show_all();
    let response = dialog.run();
    let text = entry.text().to_string();
    unsafe { dialog.destroy() };
    (response == gtk::ResponseType::Accept).then_some(text)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn prompt_card(title: &str, detail: &str, action: &str) -> Option<String> {
    if !confirm(title, detail, action) {
        return None;
    }
    paste_text().ok()
}

pub fn confirm(title: &str, detail: &str, action: &str) -> bool {
    let result = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            action.into(),
            "Cancel".into(),
        ))
        .show();
    confirmation_result(result, action)
}
pub fn decision(title: &str, detail: &str, action: &str) -> Option<bool> {
    let result = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            action.into(),
            "Decline".into(),
            "Cancel".into(),
        ))
        .show();
    decision_result(result, action)
}
fn confirmation_result(result: rfd::MessageDialogResult, action: &str) -> bool {
    matches!(result, rfd::MessageDialogResult::Ok)
        || result == rfd::MessageDialogResult::Custom(action.into())
}
fn decision_result(result: rfd::MessageDialogResult, action: &str) -> Option<bool> {
    match result {
        rfd::MessageDialogResult::Yes => Some(true),
        rfd::MessageDialogResult::No => Some(false),
        rfd::MessageDialogResult::Custom(value) if value == action => Some(true),
        rfd::MessageDialogResult::Custom(value) if value == "Decline" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfd::MessageDialogResult::*;
    #[test]
    fn native_and_custom_results_preserve_consent_and_decline() {
        for result in [Yes, Custom("Allow".into())] {
            assert_eq!(decision_result(result, "Allow"), Some(true));
        }
        for result in [No, Custom("Decline".into())] {
            assert_eq!(decision_result(result, "Allow"), Some(false));
        }
        for result in [
            Cancel,
            Ok,
            Custom("Cancel".into()),
            Custom("unexpected".into()),
        ] {
            assert_eq!(decision_result(result, "Allow"), None);
        }
        assert!(confirmation_result(Ok, "Remove"));
        assert!(confirmation_result(Custom("Remove".into()), "Remove"));
        assert!(!confirmation_result(Cancel, "Remove"));
        assert!(!confirmation_result(No, "Remove"));
    }
}
