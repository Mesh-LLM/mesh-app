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
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            action.into(),
            "Cancel".into(),
        ))
        .show()
        == rfd::MessageDialogResult::Custom(action.into())
}
pub fn decision(title: &str, detail: &str, action: &str) -> Option<bool> {
    match rfd::MessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            action.into(),
            "Decline".into(),
            "Cancel".into(),
        ))
        .show()
    {
        rfd::MessageDialogResult::Custom(value) if value == action => Some(true),
        rfd::MessageDialogResult::Custom(value) if value == "Decline" => Some(false),
        _ => None,
    }
}
