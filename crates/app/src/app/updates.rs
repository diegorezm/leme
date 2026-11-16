//! The update check and install.

use super::*;

impl App {
    pub fn checks_updates(&self) -> bool {
        self.config.check_updates
    }

    pub fn update_checked(
        &mut self,
        window: &AppWindow,
        result: Result<Option<update::Release>, String>,
        manual: bool,
    ) {
        match result {
            Ok(Some(release)) => {
                let skipped = update::skipped().as_deref() == Some(release.version.as_str());
                // Don't cover something the user is typing into.
                let busy = !window.get_overlay().is_empty();
                if manual || (!skipped && !busy) {
                    window.set_status("".into());
                    window.set_update_version(release.version.as_str().into());
                    window.set_update_installs(update::installed());
                    self.update = Some(release);
                    self.set_overlay(window, "update");
                }
            }
            Ok(None) if manual => {
                window.set_status("Leme is up to date".into());
                clear_status_later(window);
            }
            Err(err) if manual => {
                window.set_status(trf("Couldn't check for updates: {}", &[&err]).into());
                clear_status_later(window);
            }
            _ => {}
        }
    }

    /// "install", "skip", or "later", from the update prompt.
    pub fn update_action(&mut self, window: &AppWindow, action: &str) {
        self.set_overlay(window, "");
        let Some(release) = self.update.take() else {
            return;
        };
        match action {
            "install" if update::installed() => {
                window.set_status(trf("Downloading Leme {}…", &[&release.version]).into());
                crate::download_update(window, release);
            }
            // A portable copy: let the user fetch the new zip.
            "install" => update::open_page(&release),
            "skip" => {
                if let Err(err) = update::skip(&release) {
                    window.set_status(trf("Couldn't save that: {}", &[&err]).into());
                    clear_status_later(window);
                }
            }
            _ => {}
        }
    }

    pub fn update_downloaded(&mut self, window: &AppWindow, result: Result<PathBuf, String>) {
        let result = result.and_then(|path| update::install(&path).map_err(|e| e.to_string()));
        match result {
            // The installer replaces the running exe; get out of its way.
            Ok(()) => {
                let _ = slint::quit_event_loop();
            }
            Err(err) => {
                window.set_status(trf("Update failed: {}", &[&err]).into());
                clear_status_later(window);
            }
        }
    }

    /// Saves positions and the session on exit.
    pub fn quit(&mut self) {
        self.viewer.remember_position();
        self.save_study();
    }
}
