//! Study time: activity, saving, and the header clock.

use super::*;

impl App {
    /// Counts the time since the last activity toward the open document.
    pub fn activity(&mut self, window: &AppWindow) {
        if !self.config.study_timer {
            return;
        }
        let Some(path) = self.viewer.path().map(Path::to_path_buf) else {
            self.clock.reset();
            return;
        };
        let seconds = self.clock.tick(crate::annotations::now());
        if seconds == 0 {
            return;
        }
        self.study.add(study::today(), &path, seconds);
        self.show_study_time(window);
        if !self.study_save.running() {
            self.study_save.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_secs(30),
                || crate::with_app(|app| app.save_study()),
            );
        }
    }

    pub(super) fn save_study(&self) {
        // Losing a few seconds of study time isn't worth bothering about.
        let _ = self.study.save();
    }

    pub(super) fn show_study_time(&mut self, window: &AppWindow) {
        let today = study::today();
        let text = study::format_duration(self.study.total(today, today));
        if text != self.study_shown {
            window.set_study_time(text.as_str().into());
            self.study_shown = text;
        }
    }

    /// The home screen's list of the session's documents, with progress,
    /// notes, and time.
    pub(super) fn update_subjects(&self, window: &AppWindow) {
        let today = study::today();
        let last = self.last_read_tab();
        let subjects: Vec<SubjectItem> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let notes = crate::annotations::Annotations::load(path);
                let (done, total) = (
                    notes.sections.len().min(notes.outline_sections),
                    notes.outline_sections,
                );
                let mut detail = Vec::new();
                if let Some(recent) = self.viewer.store.recent_for(path) {
                    detail.push(trf("page {}", &[&(recent.page + 1)]));
                }
                if total > 0 {
                    detail.push(trf("{} of {} sections", &[&done, &total]));
                }
                match notes.items.len() {
                    0 => {}
                    1 => detail.push(tr("1 note")),
                    n => detail.push(trf("{} notes", &[&n])),
                }
                let time = self.study.for_path(path);
                if time >= 60 {
                    detail.push(study::format_duration(time));
                }
                SubjectItem {
                    title: file_name(path).into(),
                    detail: detail.join(" · ").into(),
                    progress: if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    },
                    has_progress: total > 0,
                    // Enter opens this one.
                    active: Some(i) == last,
                }
            })
            .collect();
        window.set_subjects(Rc::new(VecModel::from(subjects)).into());
        let mut greeting = tr(match study::local_hour() {
            5..=11 => "Good morning.",
            12..=17 => "Good afternoon.",
            _ => "Good evening.",
        });
        if let Some((_, page)) = self.last_read() {
            greeting.push(' ');
            greeting.push_str(&trf("You stopped at page {}.", &[&(page + 1)]));
        }
        window.set_greeting(greeting.into());
        let mut summary = Vec::new();
        if self.config.study_timer {
            summary.push(trf(
                "today {}",
                &[&study::format_duration(self.study.total(today, today))],
            ));
            summary.push(trf(
                "this week {}",
                &[&study::format_duration(self.study.total(today - 6, today))],
            ));
        }
        window.set_study_summary(summary.join(" · ").into());
    }
}
