//! One serving preference, shown as a standard checkmark menu item with a
//! fixed title; the checkmark alone carries state (macOS menu idiom, like
//! "Show Sidebar"). An `NSSwitch` embedded in an `NSMenu` breaks menu
//! tracking and renders grey, so macOS uses a check here too.
use muda::CheckMenuItem;

pub const TITLE: &str = "Share compute";

pub struct ComputeMenu {
    item: CheckMenuItem,
}

impl ComputeMenu {
    pub fn new(enabled: bool) -> Self {
        Self {
            item: CheckMenuItem::with_id("compute", TITLE, true, enabled, None),
        }
    }

    pub fn item(&self) -> &CheckMenuItem {
        &self.item
    }

    /// `busy` briefly disables the item while the engine restarts, so
    /// overlapping start/stop requests cannot be queued.
    pub fn sync(&self, sharing: bool, busy: bool) {
        if self.item.is_checked() != sharing {
            self.item.set_checked(sharing);
        }
        if self.item.is_enabled() == busy {
            self.item.set_enabled(!busy);
        }
    }
}
