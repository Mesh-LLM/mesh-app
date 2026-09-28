//! Stop-before-save launcher settings transactions.
use super::*;
impl App {
    pub(super) fn change_mode(&mut self, connection: settings::Connection) {
        // muda auto-toggles the clicked item before dispatch. Restore the saved
        // choice so cancellation, same-mode clicks and failures cannot lie.
        self.sync_mode_checks();
        if self.stopping.is_some()
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
                "Start a private Mesh?",
                "Restarts Mesh as its own private Mesh, with nobody in it yet — copy an invite and send it to the people you want. Anyone who has it can join and pass it on.",
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
        // Switching Mesh *is* forgetting this one: the people, the outstanding
        // invitations all belong to the Mesh being left, so there
        // is no separate "start over" to find.
        let next = mesh_tray::reset::switching_to(&self.settings, connection);
        self.queue_settings(next);
    }

    pub(super) fn queue_settings(&mut self, next: settings::Settings) {
        if self.stopping.is_some() || self.pending_settings.is_some() {
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
                }
            }
        } else {
            self.apply_pending();
        }
    }

    pub(super) fn apply_pending(&mut self) {
        if let Some(next) = self.pending_settings.take() {
            match next.save(&self.root) {
                Ok(()) => {
                    self.settings = next;
                    self.sync_mode_checks();
                    self.snapshot = status::Snapshot::default();
                    self.error = None;
                    self.start();
                }
                Err(e) => {
                    self.error = Some(format!("Could not save connection: {e}"));
                }
            }
        }
    }
}
#[cfg(test)]
mod delayed_stop_tests {
    use super::*;

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
