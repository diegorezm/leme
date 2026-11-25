//! Config and themes: loading, applying, and the settings screen.

use super::*;

impl App {
    pub fn reload_config(&mut self, window: &AppWindow) {
        let (config, problem) = Config::load();
        let (themes, theme_problems) = load_themes();
        self.config = config;
        self.themes = themes;
        self.problems = problem.into_iter().chain(theme_problems).collect();
        self.apply_config(window);
    }

    pub(super) fn apply_config(&mut self, window: &AppWindow) {
        let catalog = i18n::set_language(&self.config.language);
        // Fails only before a window exists, or without bundled
        // translations; the interface then stays in English.
        let _ = slint::select_bundled_translation(catalog);
        window.set_palette_placeholder(tr(PALETTE_PLACEHOLDER).into());
        let language = i18n::LANGUAGES
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(self.config.language.trim()))
            .map_or(i18n::LANGUAGES[1].1, |(_, name)| name);
        self.settings.set_language(language.into());
        self.settings.set_language_options(
            Rc::new(VecModel::from(
                i18n::LANGUAGES
                    .iter()
                    .map(|(_, name)| slint::SharedString::from(*name))
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        window.set_sidebar_button(self.config.sidebar_button);
        window.set_study_timer(self.config.study_timer);
        let (keymap, key_problems) = Keymap::new(self.config.vim, &self.config.keys);
        self.keymap = keymap;
        let header_keys: Vec<slint::SharedString> =
            HINT_KEYS.iter().map(|a| self.key_hint(*a).into()).collect();
        window.set_header_keys(Rc::new(VecModel::from(header_keys)).into());
        let mut problems = self.problems.clone();
        problems.extend(key_problems);

        let theme = self
            .themes
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&self.config.theme))
            .or(self.themes.first())
            .cloned();
        let metrics = self.config.metrics();
        for t in [window.global::<Theme>(), self.settings.global::<Theme>()] {
            t.set_font_family(self.config.font.as_str().into());
            t.set_font_size(self.config.font_size());
            t.set_spacing(metrics.spacing);
            t.set_radius(metrics.radius);
            t.set_header_height(metrics.header_height);
            t.set_sidebar_width(metrics.sidebar_width);
        }
        self.viewer.set_sidebar_width(window, metrics.sidebar_width);
        window.set_side_panel_width(self.config.side_panel_width);
        let mut fonts: Vec<slint::SharedString> =
            config::FONTS.iter().map(|f| (*f).into()).collect();
        if !config::FONTS.contains(&self.config.font.as_str()) {
            fonts.insert(0, self.config.font.as_str().into());
        }
        let s = &self.settings;
        s.set_font_options(Rc::new(VecModel::from(fonts)).into());
        s.set_font_name(self.config.font.as_str().into());
        s.set_font_size(self.config.font_size().round() as i32);
        s.set_density(label(self.config.density).into());
        s.set_corners(label(self.config.corners).into());
        s.set_thumbnail_size(label(self.config.thumbnail_size).into());
        self.title_colors = theme.as_ref().map(|t| {
            (
                to_color(&t.background),
                to_color(&t.text),
                to_color(&t.border),
                t.dark,
            )
        });
        paint_title_bars_soon(window.as_weak());
        if let Some(theme) = &theme {
            apply_theme(&window.global::<Theme>(), theme);
            apply_theme(&self.settings.global::<Theme>(), theme);
        } else {
            problems.push(trf("Theme \"{}\" not found", &[&self.config.theme]));
        }

        // The house themes are previewed; the rest are listed by name.
        let house: Vec<String> = built_in_themes()
            .into_iter()
            .take(config::HOUSE_THEMES)
            .map(|t| t.name)
            .collect();
        let highlight = to_color(&self.config.color_hex(crate::annotations::COLORS[0].0));
        let cards: Vec<ThemeCard> = house
            .iter()
            .filter_map(|name| self.themes.iter().find(|t| t.name == *name))
            .map(|t| ThemeCard {
                name: t.name.as_str().into(),
                detail: THEME_TAGLINES
                    .iter()
                    .find(|(name, _)| *name == t.name)
                    .map(|(_, tagline)| tr(tagline))
                    .unwrap_or_default()
                    .into(),
                background: to_color(&t.background),
                surface: to_color(&t.surface),
                border: to_color(&t.border),
                accent: to_color(&t.accent),
                page: to_color(&t.page),
                // Ink on the paper: the text of a light theme, the
                // darkness around the page of a dark one.
                ink: to_color(if t.dark { &t.background } else { &t.text }),
                highlight,
            })
            .collect();
        let others: Vec<slint::SharedString> = self
            .themes
            .iter()
            .filter(|t| !house.contains(&t.name))
            .map(|t| t.name.as_str().into())
            .collect();
        let name = theme.as_ref().map(|t| t.name.clone()).unwrap_or_default();
        self.settings.set_other_theme(
            if house.contains(&name) {
                tr("Other themes")
            } else {
                name.clone()
            }
            .into(),
        );
        self.settings
            .set_other_themes(Rc::new(VecModel::from(others)).into());
        self.settings
            .set_theme_cards(Rc::new(VecModel::from(cards)).into());
        self.settings.set_theme_name(name.into());
        self.settings.set_page_colors(
            match self.config.page_colors {
                PageColors::Original => "original",
                PageColors::Paper => "paper",
                PageColors::Inverted => "inverted",
            }
            .into(),
        );
        self.settings.set_default_zoom(
            match self.config.default_zoom {
                DefaultZoom::FitWidth => "Fit width",
                DefaultZoom::FitPage => "Fit page",
            }
            .into(),
        );
        self.settings
            .set_restore_session(self.config.restore_session);
        self.settings.set_sidebar_default(self.config.sidebar);
        self.settings.set_vim(self.config.vim);
        self.settings.set_study_timer(self.config.study_timer);
        self.settings.set_problems(problems.join("\n").into());

        self.update_key_rows();

        self.apply_highlight_colors(window);
        self.viewer.set_default_zoom(self.config.default_zoom);
        let paper = theme
            .as_ref()
            .and_then(|t| config::parse_color(&t.page))
            .map(|[r, g, b, _]| [r, g, b]);
        self.viewer.set_page_tint(
            window,
            match (self.config.page_colors, paper) {
                (PageColors::Inverted, _) => PageTint::Inverted,
                (PageColors::Paper, Some(paper)) => PageTint::Paper(paper),
                _ => PageTint::None,
            },
        );
        if self.active.is_none() {
            self.update_subjects(window);
        }
    }

    /// The first shortcut of an action as shown in the interface: "Ctrl K".
    pub(super) fn key_hint(&self, action: Action) -> String {
        self.keymap
            .keys_for(action)
            .first()
            .map(|keys| key_label(keys))
            .unwrap_or_default()
    }

    pub(super) fn apply_highlight_colors(&mut self, window: &AppWindow) {
        let rows: Vec<HighlightRow> = crate::annotations::COLORS
            .iter()
            .map(|(color, _)| {
                let hex = self.config.color_hex(color);
                HighlightRow {
                    key: (*color).into(),
                    name: self.config.color_name(color).into(),
                    color: to_color(&hex),
                    hex: hex.into(),
                }
            })
            .collect();
        let names = rows.iter().map(|r| r.name.to_string()).collect();
        let shades = rows.iter().map(|r| r.color).collect();
        self.viewer.set_colors(window, names, shades);
        let presets: Vec<slint::Color> = HIGHLIGHT_PRESETS.iter().map(|h| to_color(h)).collect();
        self.settings
            .set_highlight_presets(Rc::new(VecModel::from(presets)).into());
        self.settings
            .set_highlight_rows(Rc::new(VecModel::from(rows)).into());
    }

    pub(super) fn update_key_rows(&self) {
        let query = self.settings.get_query();
        let rows: Vec<KeyRow> = Action::ALL
            .iter()
            .filter(|a| {
                query.trim().is_empty()
                    || palette::score(&query, &tr(a.title())).is_some()
                    || palette::score(&query, a.title()).is_some()
                    || palette::score(&query, a.name()).is_some()
            })
            .map(|a| KeyRow {
                title: tr(a.title()).into(),
                name: a.name().into(),
                keys: self.keymap.keys_for(*a).join(",  ").into(),
            })
            .collect();
        self.settings
            .set_key_rows(Rc::new(VecModel::from(rows)).into());
    }

    /// Filters the settings sections by the search box and jumps to the
    /// first match.
    pub fn settings_searched(&mut self, query: &str) {
        let [general, reading, theme, appearance, keys, files, libraries] =
            matching_sections(query);
        let s = &self.settings;
        s.set_show_general(general);
        s.set_show_reading(reading);
        s.set_show_theme(theme);
        s.set_show_appearance(appearance);
        s.set_show_keys(keys);
        s.set_show_files(files);
        s.set_show_libraries(libraries);
        let first = [
            (general, "general"),
            (reading, "reading"),
            (theme, "theme"),
            (appearance, "appearance"),
            (keys, "keys"),
            (files, "files"),
            (libraries, "libraries"),
        ]
        .into_iter()
        .find(|(shown, _)| *shown);
        if let Some((_, section)) = first {
            if !query.trim().is_empty() {
                s.set_section(section.into());
            }
        }
        self.update_key_rows();
    }

    pub(super) fn save_config(&self, window: &AppWindow) {
        if let Err(err) = self.config.save() {
            window.set_status(trf("Couldn't save config: {}", &[&err]).into());
        }
    }

    /// The side panel was dragged to `width` (0: back to automatic).
    pub fn side_panel_resized(&mut self, window: &AppWindow, width: f32) {
        self.config.side_panel_width = if width > 0.0 {
            width.round().clamp(200.0, 720.0)
        } else {
            0.0
        };
        self.save_config(window);
        self.apply_config(window);
    }

    /// A change from the settings screen.
    pub fn setting(&mut self, window: &AppWindow, key: &str, value: &str) {
        if let Some(change) = key.strip_prefix("library-") {
            return self.library_setting(window, change, value);
        }
        // Choices arrive as shown, in the interface language. Only translate
        // back enum choices that are localized in the UI, never free text.
        let choice = || untr(value);
        let on = value == "true";
        match key {
            "theme" => self.config.theme = value.to_owned(),
            "language" => {
                if let Some((code, _)) = i18n::LANGUAGES.iter().find(|(_, name)| *name == value) {
                    self.config.language = (*code).to_owned();
                }
            }
            "page-colors" => {
                self.config.page_colors = match choice().as_str() {
                    "inverted" => PageColors::Inverted,
                    "original" => PageColors::Original,
                    _ => PageColors::Paper,
                }
            }
            "default-zoom" => {
                self.config.default_zoom = if choice() == "Fit page" {
                    DefaultZoom::FitPage
                } else {
                    DefaultZoom::FitWidth
                }
            }
            "restore-session" => self.config.restore_session = on,
            "font" => self.config.font = value.to_owned(),
            "font-size" => {
                if let Ok(size) = value.parse::<f32>() {
                    self.config.font_size =
                        size.clamp(config::MIN_FONT_SIZE, config::MAX_FONT_SIZE);
                }
            }
            "density" => {
                self.config.density = match choice().as_str() {
                    "Compact" => Density::Compact,
                    "Spacious" => Density::Spacious,
                    _ => Density::Comfortable,
                }
            }
            "corners" => {
                self.config.corners = match choice().as_str() {
                    "Sharp" => Corners::Sharp,
                    "Round" => Corners::Round,
                    _ => Corners::Rounded,
                }
            }
            "thumbnail-size" => {
                self.config.thumbnail_size = match choice().as_str() {
                    "Small" => ThumbnailSize::Small,
                    "Large" => ThumbnailSize::Large,
                    _ => ThumbnailSize::Medium,
                }
            }
            "sidebar" => {
                self.config.sidebar = on;
                window.set_sidebar_visible(on);
                self.viewer.sidebar_shown(window);
            }
            "vim" => self.config.vim = on,
            "study-timer" => {
                self.config.study_timer = on;
                self.clock.reset();
            }
            // "yellow=Important" and "yellow=#f7d44c".
            "color-name" | "color-hex" => {
                let Some((color, value)) = value.split_once('=') else {
                    return;
                };
                let value = value.trim();
                if crate::annotations::color_index(color).is_none() {
                    return;
                }
                if key == "color-name" {
                    if value.is_empty() || self.config.color_name(color) == value {
                        return;
                    }
                    self.config
                        .color_names
                        .insert(color.to_owned(), value.to_owned());
                } else if let Some(preset) = value
                    .strip_prefix("preset:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| HIGHLIGHT_PRESETS.get(i))
                {
                    self.config
                        .highlight_colors
                        .insert(color.to_owned(), (*preset).to_owned());
                } else if value == "reset" {
                    self.config.highlight_colors.remove(color);
                } else {
                    let hex = if value.starts_with('#') {
                        value.to_owned()
                    } else {
                        format!("#{value}")
                    };
                    if config::parse_color(&hex).is_none() {
                        return self.apply_highlight_colors(window);
                    }
                    self.config
                        .highlight_colors
                        .insert(color.to_owned(), hex.to_lowercase());
                }
            }
            "reset" => {
                // Custom key bindings are hand-written in config.toml;
                // restoring visual defaults shouldn't wipe them.
                self.config = Config {
                    keys: std::mem::take(&mut self.config.keys),
                    vim: self.config.vim,
                    ..Config::default()
                };
                window.set_sidebar_visible(self.config.sidebar);
            }
            "reload" => return self.reload_config(window),
            "open-config" => return self.run(window, Action::OpenConfig),
            "open-themes" => return self.run(window, Action::OpenThemesFolder),
            "open-data-dir" => {
                if let Some(dir) = config::data_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    reveal(&dir);
                }
                return;
            }
            _ => return,
        }
        self.save_config(window);
        self.apply_config(window);
    }
}
