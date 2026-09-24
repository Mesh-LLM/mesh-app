//! One serving preference, presented using each platform's native control.
use muda::{CheckMenuItem, Menu};

pub struct ComputeMenu {
    item: CheckMenuItem,
    #[cfg(target_os = "macos")]
    switch: objc2::rc::Retained<objc2_app_kit::NSSwitch>,
}

impl ComputeMenu {
    pub fn new(enabled: bool) -> Self {
        Self {
            item: CheckMenuItem::with_id("compute", "Share compute", true, enabled, None),
            #[cfg(target_os = "macos")]
            switch: objc2_app_kit::NSSwitch::new(
                objc2::MainThreadMarker::new().expect("menu on main thread"),
            ),
        }
    }

    pub fn item(&self) -> &CheckMenuItem {
        &self.item
    }

    #[cfg(not(target_os = "macos"))]
    pub fn attach(&self, _: &Menu) -> Result<(), String> {
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn attach(&self, menu: &Menu) -> Result<(), String> {
        use muda::ContextMenu;
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSAccessibility, NSMenu, NSTextField, NSView};
        use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
        let mtm = MainThreadMarker::new().ok_or("menu requires main thread")?;
        // SAFETY: muda owns this NSMenu; attachment runs on the main thread
        // after insertion. No pointer is retained beyond the owning menu.
        let native = unsafe { &*menu.ns_menu().cast::<NSMenu>() };
        let row = native
            .itemArray()
            .iter()
            .find(|item| item.title().to_string() == "Share compute")
            .ok_or("compute menu item missing")?;
        let view = NSView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(230., 32.)),
        );
        let label = NSTextField::labelWithString(&NSString::from_str("Share compute"), mtm);
        label.setFrame(NSRect::new(NSPoint::new(18., 7.), NSSize::new(155., 20.)));
        self.switch
            .setFrame(NSRect::new(NSPoint::new(178., 3.), NSSize::new(42., 26.)));
        self.switch
            .setAccessibilityLabel(Some(&NSString::from_str("Share compute")));
        // SAFETY: forward to muda's existing retained menu item target/action.
        // Its action ignores the sender and dispatches the normal MenuEvent.
        unsafe {
            self.switch.setTarget(row.target().as_deref());
            self.switch.setAction(row.action());
        }
        view.addSubview(&label);
        view.addSubview(&self.switch);
        row.setView(Some(&view));
        self.sync(self.item.is_checked(), true);
        Ok(())
    }

    pub fn sync(&self, checked: bool, enabled: bool) {
        if self.item.is_checked() != checked {
            self.item.set_checked(checked);
        }
        if self.item.is_enabled() != enabled {
            self.item.set_enabled(enabled);
        }
        #[cfg(target_os = "macos")]
        {
            let state = if checked { 1 } else { 0 };
            if self.switch.state() != state {
                self.switch.setState(state);
            }
            if self.switch.isEnabled() != enabled {
                self.switch.setEnabled(enabled);
            }
        }
    }
}
