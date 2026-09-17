//! Native dialog fallback; no networking and no admission decisions here.
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
        "The clipboard has no text on it. Copy the invite they sent, then try again.".into()
    })
}
// Windows: the clipboard is a shared, single-owner resource opened per call.
// clipboard-win handles the OpenClipboard/EmptyClipboard/GlobalAlloc dance and
// the UTF-16 CF_UNICODETEXT round-trip, keeping this adapter free of raw Win32.
#[cfg(target_os = "windows")]
pub fn copy_text(text: &str) -> Result<(), String> {
    clipboard_win::set_clipboard_string(text)
        .map_err(|e| format!("Could not put the invite on the clipboard: {e}"))
}
#[cfg(target_os = "windows")]
pub fn paste_text() -> Result<String, String> {
    clipboard_win::get_clipboard_string().map_err(|_| {
        "The clipboard has no text on it. Copy the invite they sent, then try again.".into()
    })
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn copy_text(_: &str) -> Result<(), String> {
    Err("Copying is not available on this platform.".into())
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn paste_text() -> Result<String, String> {
    Err("Pasting is not available on this platform.".into())
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
        if mesh_tray::settings::looks_like_invite(&text) {
            entry.set_text(text.trim());
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
fn confirmation_result(result: rfd::MessageDialogResult, action: &str) -> bool {
    matches!(result, rfd::MessageDialogResult::Ok)
        || result == rfd::MessageDialogResult::Custom(action.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use rfd::MessageDialogResult::*;
    #[test]
    fn native_and_custom_results_preserve_confirmation_and_cancellation() {
        assert!(confirmation_result(Ok, "Join"));
        assert!(confirmation_result(Custom("Join".into()), "Join"));
        assert!(!confirmation_result(Cancel, "Join"));
        assert!(!confirmation_result(No, "Join"));
        assert!(!confirmation_result(Custom("unexpected".into()), "Join"));
    }
}
