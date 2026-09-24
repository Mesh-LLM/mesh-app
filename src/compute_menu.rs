//! One serving preference, shown as a standard checkmark menu item whose
//! title states the current state. (An `NSSwitch` embedded in an `NSMenu`
//! breaks menu tracking and renders grey, so macOS uses a check here too.)
use muda::CheckMenuItem;

pub struct ComputeMenu {
    item: CheckMenuItem,
}

impl ComputeMenu {
    pub fn new(enabled: bool) -> Self {
        Self {
            item: CheckMenuItem::with_id("compute", title(enabled), true, enabled, None),
        }
    }

    pub fn item(&self) -> &CheckMenuItem {
        &self.item
    }

    /// `busy` briefly disables the item while the engine restarts, so
    /// overlapping start/stop requests cannot be queued.
    pub fn sync(&self, sharing: bool, busy: bool) {
        let text = title(sharing);
        if self.item.text() != text {
            self.item.set_text(text);
        }
        if self.item.is_checked() != sharing {
            self.item.set_checked(sharing);
        }
        if self.item.is_enabled() == busy {
            self.item.set_enabled(!busy);
        }
    }
}

pub fn title(sharing: bool) -> &'static str {
    if sharing {
        "Sharing compute"
    } else {
        "Not sharing compute"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_states_current_state() {
        assert_eq!(super::title(true), "Sharing compute");
        assert_eq!(super::title(false), "Not sharing compute");
    }
}
