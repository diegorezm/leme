// Release builds are GUI apps: don't open a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod instance;
mod titlebar;
mod viewer;

// The workspace crates, under the names the app's modules use.
use ::pdf;
use ::web;
use domain::{
    annotations, cache, config, export, i18n, keymap, layout, library, nav, palette, search, store,
    study,
};
use web::{html, update};

use std::cell::RefCell;

use app::App;
use keymap::Action;
use pdf::Worker;

slint::include_modules!();

thread_local! {
    // UI-thread state. Worker events reach it via invoke_from_event_loop,
    // whose closures must be Send and so can't capture an Rc.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|app| {
        // A busy app means this is a re-entrant property-change callback;
        // the outer call refreshes anyway.
        if let Ok(mut app) = app.try_borrow_mut() {
            if let Some(app) = app.as_mut() {
                f(app);
            }
        }
    });
}

/// Registers a window callback that runs `$body` with the app.
macro_rules! on {
    ($window:ident . $setter:ident, |$app:ident, $win:ident $(, $arg:ident)*| $body:expr) => {{
        let weak = $window.as_weak();
        $window.$setter(move |$($arg),*| {
            if let Some($win) = weak.upgrade() {
                with_app(|$app| $body);
            }
        });
    }};
}

/// Checks GitHub for a newer release off the UI thread. `manual` checks
/// (from the palette) also report "up to date" and errors.
pub fn check_for_update(window: &AppWindow, manual: bool) {
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = update::check().map_err(|err| err.to_string());
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = weak.upgrade() {
                with_app(|app| app.update_checked(&win, result, manual));
            }
        });
    });
}

/// Words below which a fetched page counts as empty: a shell that builds
/// its text with JavaScript, or an error page.
const MIN_PAGE_WORDS: usize = 20;

/// Reads a web page's text off the UI thread: downloads it, or reuses
/// `saved` (a page downloaded before), then hands it to the app.
pub fn load_page(window: &AppWindow, source: web::Source, saved: Option<Vec<u8>>) {
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let bytes = match saved {
            Some(bytes) => Ok(bytes),
            None => web::fetch(&source.url).map_err(|err| err.to_string()),
        };
        let result = bytes.and_then(|bytes| {
            let article = html::read_page(&bytes, &source.url, source.revoked);
            if article.word_count() < MIN_PAGE_WORDS {
                Err(i18n::tr(
                    "the page has no readable text (it may need JavaScript)",
                ))
            } else {
                Ok((article, bytes))
            }
        });
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = weak.upgrade() {
                with_app(|app| app.page_loaded(&win, source, result));
            }
        });
    });
}

/// Downloads a release's installer off the UI thread.
pub fn download_update(window: &AppWindow, release: update::Release) {
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = update::download(&release).map_err(|err| err.to_string());
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = weak.upgrade() {
                with_app(|app| app.update_downloaded(&win, result));
            }
        });
    });
}

/// Files from later launches open as tabs here, and the window comes
/// forward. Without the channel (e.g. no runtime folder) each launch is
/// simply its own window, as before.
fn listen_for_launches(window: &AppWindow) {
    let weak = window.as_weak();
    let _ = instance::listen(move |paths| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            for path in paths {
                with_app(|app| app.open_path(&window, path));
            }
            bring_to_front(&window);
        });
    });
}

fn bring_to_front(window: &AppWindow) {
    use slint::winit_030::WinitWindowAccessor;
    window.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

fn main() -> Result<(), slint::PlatformError> {
    // `leme file.pdf`, which is also how "Open with" passes files.
    let paths: Vec<std::path::PathBuf> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    if instance::forward(&paths) {
        return Ok(());
    }
    config::migrate_legacy_dirs();
    let window = AppWindow::new()?;

    let weak = window.as_weak();
    let worker = Worker::spawn(viewer::to_buffer, move |event| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                with_app(|app| app.event(&window, event));
            }
        });
    });
    let settings = SettingsWindow::new()?;
    let app = App::new(&window, settings.clone_strong(), worker);
    APP.with(|cell| *cell.borrow_mut() = Some(app));

    let mut checks_updates = false;
    with_app(|app| checks_updates = app.checks_updates());
    if checks_updates {
        check_for_update(&window, false);
    }

    for path in paths {
        with_app(|app| app.open_path(&window, path));
    }
    listen_for_launches(&window);

    let weak = window.as_weak();
    window.on_key_pressed(move |text, ctrl, shift, alt| {
        let Some(window) = weak.upgrade() else {
            return false;
        };
        let mut handled = false;
        with_app(|app| {
            app.activity(&window);
            handled = app.key_pressed(&window, &text, ctrl, shift, alt);
        });
        handled
    });

    on!(window.on_scrolled, |app, win| {
        app.activity(&win);
        app.viewer().scrolled(&win)
    });
    // While the window is being resized, pages are stretched and only
    // re-rendered once resizing pauses.
    let resize_timer = slint::Timer::default();
    let weak = window.as_weak();
    window.on_resized(move || {
        let Some(win) = weak.upgrade() else {
            return;
        };
        with_app(|app| app.viewer().resized(&win));
        let weak = weak.clone();
        resize_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(120),
            move || {
                if let Some(win) = weak.upgrade() {
                    with_app(|app| app.viewer().resize_settled(&win));
                }
            },
        );
    });
    on!(window.on_thumbs_scrolled, |app, win| app
        .viewer()
        .thumbs_scrolled(&win));
    on!(window.on_side_panel_clicked, |app, win, mode| app
        .show_side_panel(
            &win,
            viewer::SidePanel::from_name(&mode),
            false
        ));
    on!(window.on_side_item_clicked, |app, win, i| app
        .viewer()
        .side_item_clicked(&win, index(i)));
    on!(window.on_side_item_status_clicked, |app, win, i| app
        .viewer()
        .side_item_status_clicked(&win, index(i)));
    on!(window.on_note_filter_clicked, |app, win, i| app
        .note_filter_clicked(&win, Some(index(i))));
    on!(window.on_note_filter_all, |app, win| app
        .note_filter_clicked(&win, None));
    on!(window.on_thumb_clicked, |app, win, page| app
        .viewer()
        .go_to_page(&win, index(page)));
    on!(window.on_open_recent, |app, win, i| app
        .open_recent(&win, index(i)));
    on!(window.on_open_session, |app, win, i| app
        .switch_session(&win, index(i)));
    on!(window.on_prompt_accepted, |app, win, text| app
        .prompt_accepted(&win, &text));
    on!(window.on_library_action, |app, win, action, i| app
        .library_screen_action(
            &win,
            &action,
            usize::try_from(i).unwrap_or(usize::MAX)
        ));
    on!(window.on_library_searched, |app, win, text| app
        .library_searched(&win, &text));
    on!(window.on_open_library, |app, win, i| app.open_library(
        &win,
        usize::try_from(i).unwrap_or(usize::MAX)
    ));
    on!(window.on_side_panel_resized, |app, win, width| app
        .side_panel_resized(&win, width));
    on!(window.on_header_action, |app, win, action| {
        let action = match action.as_str() {
            "home" => Action::Home,
            "sessions" => Action::SwitchSession,
            "new-session" => Action::NewSession,
            "sidebar" => Action::ToggleSidebar,
            "study-timer" => Action::ToggleStudyTimer,
            "goto" => Action::GoToPage,
            "palette" => Action::CommandPalette,
            "export" => Action::ExportNotes,
            "new-library" => Action::NewLibrary,
            "settings" => Action::Settings,
            _ => Action::Open,
        };
        app.run(&win, action)
    });
    on!(window.on_search_edited, |app, win, text| app
        .search_edited(&win, &text));
    on!(window.on_search_step, |app, win, forward| app
        .search_step(&win, forward));
    on!(window.on_search_scope_clicked, |app, win, scope| app
        .set_search_scope(&win, scope == "session"));
    on!(window.on_search_hit_clicked, |app, win, i| app
        .search_hit_clicked(&win, index(i)));
    on!(window.on_overlay_dismissed, |app, win| app
        .run(&win, Action::Escape));
    on!(window.on_goto_accepted, |app, win, text| app
        .go_to_typed_page(&win, &text));
    on!(window.on_zoom_action, |app, win, action| {
        let action = match action.as_str() {
            "in" => Action::ZoomIn,
            "out" => Action::ZoomOut,
            "reset" => Action::ZoomReset,
            "fit-page" => Action::FitPage,
            _ => Action::FitWidth,
        };
        app.run(&win, action)
    });
    on!(window.on_tab_clicked, |app, win, i| app
        .open_tab(&win, index(i)));
    on!(window.on_tab_closed, |app, win, i| app
        .close_tab(&win, index(i)));
    on!(window.on_reader_pointer, |app, win, kind, x, y| {
        if kind != "hover" {
            app.activity(&win);
        }
        app.viewer().pointer(&win, &kind, x, y)
    });
    on!(window.on_open_subject, |app, win, i| app
        .open_tab(&win, index(i)));
    on!(window.on_popup_action, |app, win, action, i| app
        .popup_action(&win, &action, index(i)));
    on!(window.on_update_action, |app, win, action| app
        .update_action(&win, &action));
    on!(window.on_note_saved, |app, win| app.note_saved(&win));
    on!(window.on_note_deleted, |app, win| app.note_deleted(&win));
    on!(window.on_note_color_clicked, |app, win, i| app
        .note_color_clicked(&win, index(i)));
    on!(window.on_export_option, |app, win, key, value| app
        .export_option(&win, &key, &value));
    on!(window.on_export_accepted, |app, win| app
        .export_accepted(&win));
    on!(window.on_palette_edited, |app, win, text| app
        .palette_edited(&win, &text));
    on!(window.on_palette_moved, |app, win, delta| app
        .palette_moved(&win, delta));
    on!(window.on_palette_accepted, |app, win| app
        .palette_accept(&win, None));
    on!(window.on_palette_clicked, |app, win, i| app
        .palette_accept(&win, Some(index(i))));

    // The settings window reports back to the main window's app state.
    let weak = window.as_weak();
    settings.on_set(move |key, value| {
        if let Some(win) = weak.upgrade() {
            with_app(|app| app.setting(&win, &key, &value));
        }
    });
    settings.on_searched(|query| with_app(|app| app.settings_searched(&query)));
    settings.on_close_requested(|| with_app(|app| app.close_settings()));

    // Closing the main window quits, even with settings still open.
    window.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });

    // Dropping PDFs on the window opens them, like Ctrl+O.
    {
        use slint::winit_030::{winit::event::WindowEvent, EventResult, WinitWindowAccessor};
        let weak = window.as_weak();
        window.window().on_winit_window_event(move |_, event| {
            if let WindowEvent::DroppedFile(path) = event {
                let is_pdf = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
                if let (true, Some(win)) = (is_pdf, weak.upgrade()) {
                    with_app(|app| app.open_path(&win, path.clone()));
                }
            }
            EventResult::Propagate
        });
    }

    app::paint_title_bars_soon(window.as_weak());

    window.run()?;
    with_app(|app| app.quit());
    Ok(())
}

#[cfg(test)]
mod tests {
    use domain::keymap::key;
    use slint::platform::Key;

    /// `domain` names Slint's key codes without depending on Slint.
    #[test]
    fn key_codes_match_slint() {
        let pairs = [
            (key::BACKSPACE, Key::Backspace),
            (key::TAB, Key::Tab),
            (key::RETURN, Key::Return),
            (key::SHIFT, Key::Shift),
            (key::CONTROL, Key::Control),
            (key::BACKTAB, Key::Backtab),
            (key::ESCAPE, Key::Escape),
            (key::SPACE, Key::Space),
            (key::DELETE, Key::Delete),
            (key::UP, Key::UpArrow),
            (key::DOWN, Key::DownArrow),
            (key::LEFT, Key::LeftArrow),
            (key::RIGHT, Key::RightArrow),
            (key::F1, Key::F1),
            (key::HOME, Key::Home),
            (key::END, Key::End),
            (key::PAGE_UP, Key::PageUp),
            (key::PAGE_DOWN, Key::PageDown),
        ];
        for (ours, slint) in pairs {
            assert_eq!(ours, char::from(slint), "{slint:?}");
        }
        assert_eq!(char::from(Key::F12) as u32, key::F1 as u32 + 11);
    }
}
