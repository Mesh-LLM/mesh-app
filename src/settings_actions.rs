//! Confirmed launcher changes and stop-before-save transactions.
use super::*;

impl App {
    pub(super) fn change_mode(&mut self, connection: settings::Connection) {
        // muda auto-toggles the clicked item before dispatch. Restore the saved
        // choice so cancellation, same-mode clicks and failures cannot lie.
        self.sync_mode_checks();
        if self.resetting
            || self.stopping.is_some()
            || self.pending_settings.is_some()
            || std::mem::discriminant(&self.settings.connection)
                == std::mem::discriminant(&connection)
        {
            return;
        }
        let leaving_joined_mesh = matches!(
            self.settings.connection,
            settings::Connection::Private { .. }
        ) && !self.settings.joins().is_empty();
        let (title, body) = if leaving_joined_mesh {
            (
                "Leave this private Mesh?",
                "Going Public forgets the invite that put you in it, and restarts Mesh. It does not remove you for anyone else: rejoining means pasting an invite again. Your identity, your models and your settings stay as they are.",
            )
        } else if matches!(connection, settings::Connection::Private { .. }) {
            (
                "Switch to Private?",
                "Restarts Mesh in Private. To explicitly retire saved membership and policy, use Reset settings first. Invitations can be forwarded by anyone who receives them.",
            )
        } else {
            (
                "Share with anyone?",
                "Restarts Mesh in Public, where it serves whoever finds it. Anything outstanding is cancelled.",
            )
        };
        if !native::confirm(title, body, "Change connection") {
            return;
        }
        if matches!(connection, settings::Connection::Private { .. }) {
            let profile = match settings::mesh_profile() {
                Ok(profile) => profile,
                Err(e) => {
                    native::notice("Could not set up Private", &e);
                    return;
                }
            };
            if let Err(e) = identity::establish(&profile) {
                native::notice("Could not set up Private", &e);
                return;
            }
        }
        // Forget the launcher invite, not engine membership or issued invites.
        let next = mesh_tray::reset::switching_to(&self.settings, connection);
        self.queue_settings(next);
    }

    pub(super) fn queue_settings(&mut self, next: settings::Settings) {
        if self.resetting || self.stopping.is_some() || self.pending_settings.is_some() {
            return;
        }
        self.pending_settings = Some(next);
        self.open_when_ready = None;
        if let Some(child) = &mut self.child {
            match lifecycle::request_stop(child) {
                Ok(()) => self.stopping = Some(Instant::now()),
                Err(e) => {
                    self.error = Some(e);
                    self.pending_settings = None;
                    self.retire_private = false;
                }
            }
        } else {
            self.apply_pending();
        }
    }

    pub(super) fn apply_pending(&mut self) {
        if let Some(next) = self.pending_settings.take() {
            let saved = if self.retire_private {
                self.retire_private = false;
                self.finish_private_reset(&next)
            } else {
                next.save(&self.root)
            };
            match saved {
                Ok(()) => {
                    self.settings = next;
                    self.sync_mode_checks();
                    self.snapshot = status::Snapshot::default();
                    self.error = None;
                    self.start();
                }
                Err(e) => {
                    self.error = Some(format!("Could not save connection or finish reset: {e}. No replacement engine started."));
                }
            }
        }
    }

    fn finish_private_reset(&self, next: &settings::Settings) -> Result<(), String> {
        Engine::verify_restart_safe()?;
        let profile = mesh_tray::private_reset::validate_default_profile()?;
        mesh_tray::private_reset::ensure_no_runtime()?;
        mesh_tray::private_reset::retire(&profile, &self.root)?;
        next.save(&self.root)?;
        mesh_tray::private_reset::finish(&self.root)
    }

    pub(super) fn reset_settings(&mut self) {
        if self.resetting || self.stopping.is_some() || self.pending_settings.is_some() {
            return;
        }
        let confirmed = native::confirm(
            "Reset settings?",
            "Stops this app’s engine and interrupts active requests.\n\n• Restores Public, Share Compute ON, ports 3232/9447. Public discovery and model downloads may resume.\n• Turns paying/charging OFF; clears allowance and model prices. Requires a healthy engine to verify this.\n• Retires private membership and policy. Your next private Mesh requires new invitations; old members may continue their old Mesh, not enter your new one. Public remains open to everyone.\n• Keeps owner credentials, wallet funds/history/settlements, engine config, models and logs.\n\nKeep other apps using this Mesh profile stopped. If shutdown fails, quit and reopen Mesh before retrying.",
            "Reset settings",
        );
        self.reset_if_confirmed(confirmed);
    }

    fn reset_if_confirmed(&mut self, confirmed: bool) {
        if !confirmed {
            return;
        }
        if mesh_tray::private_reset::pending(&self.root) && self.child.is_none() {
            self.retire_private = true;
            self.queue_settings(settings::Settings::default());
            return;
        }
        if let Err(error) = mesh_tray::private_reset::validate_default_profile() {
            native::notice("Reset unavailable", &error);
            return;
        }
        let Some(target) = self.pay_target() else {
            native::notice("Reset unavailable", "The owned Mesh engine must be ready so paying and charging can be disabled and verified. No tray settings were reset. Quit and reopen Mesh if startup failed.");
            return;
        };
        match self.pay.begin_reset(target) {
            Ok(()) => self.resetting = true,
            Err(error) => native::notice("Reset unavailable", &error),
        }
    }

    pub(super) fn complete_reset(&mut self, result: Result<(), String>) {
        self.resetting = false;
        match result {
            Ok(()) if self.child.is_none() => {
                self.error = Some("Engine exited during payment reset; private state was not retired. Quit and reopen Mesh.".into());
            }
            Ok(()) => {
                self.retire_private = true;
                self.queue_settings(settings::Settings::default());
            },
            Err(error) => native::notice("Reset incomplete", &format!("{error}. Some payment preferences may already have changed. Tray settings were not reset; wallet funds and history were not erased. Check Payments and try Reset again.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_cancel_preserves_settings_and_does_not_stop_engine() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new(root.path().into(), settings::Settings::default());
        app.settings.accept_seed("saved-mesh").unwrap();
        app.settings.save(root.path()).unwrap();
        let before = std::fs::read(root.path().join("launcher.json")).unwrap();
        let (engine, mut stop, _done) = Engine::fixture();
        app.child = Some(engine);
        app.reset_if_confirmed(false);
        assert!(stop.try_recv().is_err());
        assert!(app.pending_settings.is_none());
        assert_eq!(
            std::fs::read(root.path().join("launcher.json")).unwrap(),
            before
        );
    }

    #[test]
    fn reset_queues_all_defaults_but_waits_for_owned_engine_exit() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = settings::Settings::default();
        settings.accept_seed("saved-mesh").unwrap();
        settings.share_compute = false;
        settings.api_port = 4242;
        settings.console_port = 4243;
        settings.save(root.path()).unwrap();
        let before = std::fs::read(root.path().join("launcher.json")).unwrap();
        let mut app = App::new(root.path().into(), settings);
        let (engine, mut stop, _done) = Engine::fixture();
        app.child = Some(engine);
        app.complete_reset(Ok(()));
        assert_eq!(stop.try_recv(), Ok(()));
        let pending = app.pending_settings.as_ref().unwrap();
        assert_eq!(
            serde_json::to_value(pending).unwrap(),
            serde_json::to_value(settings::Settings::default()).unwrap()
        );
        assert_eq!(
            std::fs::read(root.path().join("launcher.json")).unwrap(),
            before
        );
        assert_eq!(app.settings.joins().len(), 1);
        assert!(app.child.is_some());
    }
}

#[cfg(test)]
mod delayed_stop_tests {
    use super::*;

    #[test]
    fn failed_shutdown_never_applies_reset_or_restarts() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new(root.path().into(), settings::Settings::default());
        app.polling = true;
        let (engine, _stop, done) = Engine::fixture();
        app.child = Some(engine);
        app.pending_settings = Some(settings::Settings::default());
        app.retire_private = true;
        app.stopping = Some(Instant::now());
        done.send(Err("shutdown timed out".into())).unwrap();
        app.tick();
        assert!(app.child.is_none());
        assert!(app.pending_settings.is_none());
        assert!(!app.retire_private);
        assert!(!root.path().join("launcher.json").exists());
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("Shutdown was not verified"));
    }

    #[test]
    fn quit_during_transition_cancels_restart_and_waits_for_exit() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new(root.path().into(), settings::Settings::default());
        app.polling = true;
        let (engine, _stop, done) = Engine::fixture();
        app.child = Some(engine);
        app.pending_settings = Some(settings::Settings::default());
        app.retire_private = true;
        app.stopping = Some(Instant::now());
        app.quit();
        assert!(app.pending_settings.is_none());
        assert!(!app.retire_private);
        assert!(!app.exit);
        done.send(Ok(())).unwrap();
        app.tick();
        assert!(app.exit);
        assert!(!root.path().join("launcher.json").exists());
    }

    #[test]
    fn slow_stop_keeps_pending_change_until_completion() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new(root.path().into(), settings::Settings::default());
        app.polling = true;
        let (engine, _stop, done) = Engine::fixture();
        app.child = Some(engine);
        app.pending_settings = Some(settings::Settings::default());
        app.stopping = Some(Instant::now() - Duration::from_secs(25));
        app.tick();
        assert!(app.stopping.is_some());
        assert!(app.pending_settings.is_some());
        // Force save failure, so completion cannot launch any real runtime.
        std::fs::create_dir(root.path().join("launcher.json")).unwrap();
        done.send(Ok(())).unwrap();
        app.tick();
        assert!(app.child.is_none());
        assert!(app.error.as_deref().unwrap().contains("Could not save"));
        assert!(!app.exit);
    }
}
