//! AppKit transport only: no networking and no admission side effects.
//! Inherit native menus, sheets and dialogs; preserve Mesh's jellyfish identity.
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_app_kit::{NSAlert, NSApplication, NSOpenPanel, NSSharingServicePicker};
use objc2_foundation::{MainThreadMarker, NSArray, NSRectEdge, NSString, NSURL};
use std::path::PathBuf;

#[derive(Default)]
pub struct Native {
    // Keep picker and file alive until app exit; service completion cleanup is a draft gate.
    shares: Vec<(Retained<NSSharingServicePicker>, tempfile::NamedTempFile)>,
}
impl Native {
    pub fn share(
        &mut self,
        file: tempfile::NamedTempFile,
        tray: &tray_icon::TrayIcon,
    ) -> Result<(), String> {
        if self.shares.len() >= 32 {
            return Err("Sharing limit reached for this draft session. Cancel pending requests before quitting and reopening.".into());
        }
        let mtm = MainThreadMarker::new().ok_or("Sharing must run on the main thread")?;
        let status = tray.ns_status_item().ok_or("Tray item is unavailable")?;
        let button = status.button(mtm).ok_or("Tray button is unavailable")?;
        let path = file
            .path()
            .to_str()
            .ok_or("Share file path is not valid Unicode")?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let items = NSArray::<AnyObject>::from_slice(&[url.as_ref()]);
        // SAFETY: File NSURL objects conform to the sharing item contract.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items)
        };
        picker.showRelativeToRect_ofView_preferredEdge(button.bounds(), &button, NSRectEdge::MinY);
        self.shares.push((picker, file));
        Ok(())
    }
}

// Tray clicks do not necessarily activate an accessory app. Bring user-requested
// dialogs forward without changing the tray-only activation policy.
fn foreground_dialog(mtm: MainThreadMarker) {
    // Match winit's activation path, including support for older macOS versions.
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

pub fn choose_file() -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setTitle(Some(&NSString::from_str(
        "Open Mesh invitation or membership receipt",
    )));
    foreground_dialog(mtm);
    if panel.runModal() != 1 {
        return None;
    }
    let url = panel.URL()?;
    if !url.isFileURL() {
        return None;
    }
    Some(PathBuf::from(url.path()?.to_string()))
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

pub fn decision(title: &str, detail: &str, action: &str) -> Option<bool> {
    let mtm = MainThreadMarker::new()?;
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.addButtonWithTitle(&NSString::from_str(action));
    alert.addButtonWithTitle(&NSString::from_str("Decline"));
    foreground_dialog(mtm);
    match alert.runModal() {
        1001 => Some(true),
        1002 => Some(false),
        _ => None,
    }
}
