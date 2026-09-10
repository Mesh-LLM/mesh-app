//! Native file and consent fallback; no networking or admission decisions here.
use std::path::PathBuf;
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
pub fn choose_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Mesh request or reply")
        .pick_file()
}
pub fn notice(title: &str, detail: &str) {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .show();
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
