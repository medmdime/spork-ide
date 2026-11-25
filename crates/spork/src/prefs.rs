//! Preferences (docs/specs/M16 §4): appearance, terminal, remembered diff layout, kept in
//! `<config dir>/preferences.conf` as `key=value` lines.

use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::util::ResultExt as _;
use crate::{RepoView, repos};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prefs {
    pub(crate) appearance: Appearance,
    /// Diffs open side by side (the diff bar's toggle).
    pub(crate) split_diffs: bool,
    /// macOS: an app name (`iTerm`, `Warp`, `Ghostty`); elsewhere a command started in the
    /// repository folder. Empty: the system default.
    pub(crate) terminal: String,
    /// Fetch all remotes in the background every this many minutes; 0 = never (M09 §1).
    pub(crate) fetch_minutes: u32,
    /// Hide whitespace-only changes in commit and file-history diffs (M08 §2).
    pub(crate) ignore_whitespace: bool,
    /// Folders Quick Launch searches for repositories, `;`-separated (M01 §3).
    pub(crate) source_folders: String,
    /// Open in Editor: an app name on macOS (`Visual Studio Code`, `Zed`), elsewhere a command
    /// given the path (`code`, `zed`). Empty: not set.
    pub(crate) editor: String,
    /// Local Changes groups files under their folders.
    pub(crate) changes_tree: bool,
    /// Font of the interface; empty: the system's.
    pub(crate) ui_font: String,
    /// Text size of the interface in points (⌥⇧⌘= / ⌥⇧⌘-); 0: 13, IntelliJ's.
    pub(crate) ui_font_size: u32,
    /// Font of diffs and code, and its size in points; empty / 0: the theme's.
    pub(crate) code_font: String,
    pub(crate) code_font_size: u32,
    /// Diffs wrap long lines; show spaces and tabs; tab width in spaces (0: as is).
    pub(crate) wrap_diffs: bool,
    pub(crate) show_whitespace: bool,
    pub(crate) tab_width: u32,
    /// Hints about misspelt words under the commit message (spell.rs).
    pub(crate) spell_check: bool,
    /// Scroll bars always show, rather than only while scrolling.
    pub(crate) scrollbars_always: bool,
    /// The branches popup's gear menu (IntelliJ's GitVcsSettings): actions found by its search,
    /// branches grouped by directory, the Recent section, the Tags section.
    pub(crate) branches_search_actions: bool,
    pub(crate) branches_by_directory: bool,
    pub(crate) branches_recent: bool,
    pub(crate) branches_tags: bool,
    /// The log's view options (its eye, IntelliJ's VcsLogUiProperties): one label per commit,
    /// tag names in it, the commit time rather than the author's, labels left of the subject;
    /// the Author, Date and Hash columns; your commits in bold, merges dimmed, the current
    /// branch's commits highlighted.
    pub(crate) log_compact_refs: bool,
    pub(crate) log_tag_names: bool,
    pub(crate) log_commit_time: bool,
    pub(crate) log_refs_left: bool,
    pub(crate) log_author: bool,
    pub(crate) log_date: bool,
    pub(crate) log_hash: bool,
    pub(crate) log_mark_mine: bool,
    pub(crate) log_mark_merges: bool,
    pub(crate) log_mark_branch: bool,
    /// The commit's changes grouped by directory, and its details under them.
    pub(crate) log_changes_by_directory: bool,
    pub(crate) log_details: bool,
    /// The log's other options (log_toolbar.rs).
    pub(crate) log_view: crate::log_toolbar::LogViewPrefs,
}

impl Prefs {
    fn file() -> Option<std::path::PathBuf> {
        Some(repos::config_dir()?.join("preferences.conf"))
    }

    pub(crate) fn load() -> Self {
        // no file yet reads as an empty one, so defaults that are "on" stay on
        let text = Self::file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let get = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("")
        };
        Prefs {
            appearance: match get("appearance") {
                "light" => Appearance::Light,
                "dark" => Appearance::Dark,
                _ => Appearance::System,
            },
            split_diffs: get("split_diffs") == "true",
            terminal: get("terminal").to_owned(),
            fetch_minutes: get("fetch_minutes").parse().unwrap_or(0),
            ignore_whitespace: get("ignore_whitespace") == "true",
            source_folders: get("source_folders").to_owned(),
            editor: get("editor").to_owned(),
            changes_tree: get("changes_tree") == "true",
            ui_font: get("ui_font").to_owned(),
            ui_font_size: get("ui_font_size").parse().unwrap_or(0),
            code_font: get("code_font").to_owned(),
            code_font_size: get("code_font_size").parse().unwrap_or(0),
            wrap_diffs: get("wrap_diffs") == "true",
            show_whitespace: get("show_whitespace") == "true",
            tab_width: get("tab_width").parse().unwrap_or(0),
            spell_check: get("spell_check") != "false",
            scrollbars_always: get("scrollbars") != "auto",
            branches_search_actions: get("branches_search_actions") != "false",
            branches_by_directory: get("branches_by_directory") != "false",
            branches_recent: get("branches_recent") != "false",
            branches_tags: get("branches_tags") != "false",
            log_compact_refs: get("log_compact_refs") != "false",
            log_tag_names: get("log_tag_names") == "true",
            log_commit_time: get("log_commit_time") == "true",
            log_refs_left: get("log_refs_left") == "true",
            log_author: get("log_author") != "false",
            log_date: get("log_date") != "false",
            log_hash: get("log_hash") == "true",
            log_mark_mine: get("log_mark_mine") != "false",
            log_mark_merges: get("log_mark_merges") != "false",
            log_mark_branch: get("log_mark_branch") != "false",
            log_changes_by_directory: get("log_changes_by_directory") != "false",
            log_details: get("log_details") != "false",
            log_view: crate::log_toolbar::LogViewPrefs::parse(text),
        }
    }

    fn render(&self) -> String {
        let appearance = match self.appearance {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        };
        format!(
            "appearance={appearance}\nui_font={}\nsplit_diffs={}\nterminal={}\nfetch_minutes={}\nignore_whitespace={}\nsource_folders={}\neditor={}\nchanges_tree={}\nui_font_size={}\ncode_font={}\ncode_font_size={}\nwrap_diffs={}\nshow_whitespace={}\ntab_width={}\nspell_check={}\nscrollbars={}\nbranches_search_actions={}\nbranches_by_directory={}\nbranches_recent={}\nbranches_tags={}\nlog_compact_refs={}\nlog_tag_names={}\nlog_commit_time={}\nlog_refs_left={}\nlog_author={}\nlog_date={}\nlog_hash={}\nlog_mark_mine={}\nlog_mark_merges={}\nlog_mark_branch={}\nlog_changes_by_directory={}\nlog_details={}\n",
            self.ui_font.replace('\n', " "),
            self.split_diffs,
            self.terminal.replace('\n', " "),
            self.fetch_minutes,
            self.ignore_whitespace,
            self.source_folders.replace('\n', " "),
            self.editor.replace('\n', " "),
            self.changes_tree,
            self.ui_font_size,
            self.code_font.replace('\n', " "),
            self.code_font_size,
            self.wrap_diffs,
            self.show_whitespace,
            self.tab_width,
            self.spell_check,
            if self.scrollbars_always {
                "always"
            } else {
                "auto"
            },
            self.branches_search_actions,
            self.branches_by_directory,
            self.branches_recent,
            self.branches_tags,
            self.log_compact_refs,
            self.log_tag_names,
            self.log_commit_time,
            self.log_refs_left,
            self.log_author,
            self.log_date,
            self.log_hash,
            self.log_mark_mine,
            self.log_mark_merges,
            self.log_mark_branch,
            self.log_changes_by_directory,
            self.log_details,
        ) + &self.log_view.render()
    }

    pub(crate) fn save(&self) {
        if let Some(file) = Self::file() {
            std::fs::create_dir_all(file.parent().unwrap_or(&file)).log_err();
            std::fs::write(file, self.render()).log_err();
        }
    }

    /// Light or dark as chosen, or following the OS, and the code font.
    pub(crate) fn apply_appearance(&self, window: &mut Window, cx: &mut App) {
        // switching loads the theme anew, its own code font included: ours goes on after
        match self.appearance {
            Appearance::System => Theme::sync_system_appearance(Some(window), cx),
            Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        let bars = if self.scrollbars_always {
            ScrollbarMode::Always
        } else {
            ScrollbarMode::Scrolling
        };
        Theme::set_scrollbar_mode(bars, cx);
        spork_workbench::set_text_size(self.text_size(), cx);
        if !self.ui_font.trim().is_empty() {
            let font = self.ui_font.trim().to_owned();
            Theme::update(cx, |theme| theme.font_family = font.into());
        }
        let (font, size) = (self.code_font.trim(), self.code_font_size);
        if !font.is_empty() || size > 0 {
            Theme::update(cx, |theme| {
                if !font.is_empty() {
                    theme.mono_font_family = font.to_owned().into();
                }
                if size > 0 {
                    theme.mono_font_size = px(size as f32);
                }
            });
        }
        window.refresh();
    }

    /// The interface's text size in points.
    pub(crate) fn text_size(&self) -> f32 {
        match self.ui_font_size {
            0 => spork_workbench::DEFAULT_TEXT_SIZE,
            size => size as f32,
        }
    }

    /// Makes the interface's text one point bigger (`1`), smaller (`-1`) or the default (`0`),
    /// between 9 and 24.
    pub(crate) fn zoom(&mut self, step: i32) {
        self.ui_font_size = match step {
            0 => 0,
            step => (self.text_size() as i32 + step).clamp(9, 24) as u32,
        };
    }

    /// The source folders as paths.
    pub(crate) fn source_paths(&self) -> Vec<std::path::PathBuf> {
        self.source_folders
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .collect()
    }

    /// The command that opens `path` (a file or the repository folder) in the user's editor;
    /// `None` until one is set.
    pub(crate) fn editor_command(&self, path: &std::path::Path) -> Option<std::process::Command> {
        let editor = self.editor.trim();
        if editor.is_empty() {
            return None;
        }
        let mut c = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("open");
            c.args(["-a", editor]);
            c
        } else {
            std::process::Command::new(editor)
        };
        c.arg(path);
        Some(c)
    }

    /// The command that opens a terminal at `root`.
    pub(crate) fn terminal_command(&self, root: &std::path::Path) -> std::process::Command {
        let custom = self.terminal.trim();
        if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("open");
            c.args([
                "-a",
                if custom.is_empty() {
                    "Terminal"
                } else {
                    custom
                },
            ])
            .arg(root);
            return c;
        }
        let program = match custom {
            "" if cfg!(windows) => "cmd",
            "" => "x-terminal-emulator",
            other => other,
        };
        let mut c = std::process::Command::new(program);
        if cfg!(windows) && custom.is_empty() {
            c.args(["/C", "start", "cmd"]);
        }
        c.current_dir(root);
        c
    }
}

impl RepoView {
    /// ⌥⇧⌘= / ⌥⇧⌘- / ⌥⇧⌘0: the interface's text one point bigger, smaller, or back to 13.
    pub(crate) fn zoom(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.zoom(step);
        self.prefs.save();
        self.prefs.apply_appearance(window, cx);
    }

    /// The gear's Theme ▸: dark, light, or following the OS.
    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prefs.appearance = appearance;
        self.prefs.save();
        self.prefs.apply_appearance(window, cx);
        cx.notify();
    }

    /// Settings… (⌘,): the Settings window.
    pub(crate) fn preferences_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page(None, window, cx)
    }

    /// Settings, on `page` (its path in the tree) when given.
    pub(crate) fn settings_page(
        &mut self,
        page: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = (cx.entity(), self.tab_title());
        crate::settings_window::open(Some(project), self.prefs.clone(), page, window, cx)
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{Appearance, Prefs};

    #[test]
    fn zoom_steps_and_bounds() {
        let mut p = Prefs::default();
        p.zoom(1);
        assert_eq!(p.ui_font_size, 14); // from the default 13
        p.zoom(-1);
        p.zoom(-1);
        assert_eq!(p.ui_font_size, 12);
        (0..20).for_each(|_| p.zoom(1));
        assert_eq!(p.ui_font_size, 24);
        p.zoom(0);
        assert_eq!(p.ui_font_size, 0);
    }

    #[test]
    fn round_trips() {
        let p = Prefs {
            appearance: Appearance::Dark,
            ui_font: "Inter".into(),
            split_diffs: true,
            terminal: "Warp".into(),
            fetch_minutes: 15,
            ignore_whitespace: true,
            source_folders: "/a;/b c".into(),
            editor: "Zed".into(),
            changes_tree: true,
            ui_font_size: 15,
            code_font: "JetBrains Mono".into(),
            code_font_size: 14,
            wrap_diffs: true,
            show_whitespace: true,
            tab_width: 4,
            spell_check: false,
            scrollbars_always: false,
            branches_search_actions: false,
            branches_by_directory: true,
            branches_recent: false,
            branches_tags: true,
            log_compact_refs: false,
            log_tag_names: true,
            log_commit_time: true,
            log_refs_left: true,
            log_author: false,
            log_date: true,
            log_hash: true,
            log_mark_mine: false,
            log_mark_merges: true,
            log_mark_branch: true,
            log_changes_by_directory: false,
            log_details: true,
            log_view: crate::log_toolbar::LogViewPrefs {
                long_edges: true,
                diff_preview: true,
                ..Default::default()
            },
        };
        assert_eq!(Prefs::parse(&p.render()), p);
        // spell check and the branches popup's options are on unless turned off
        let fresh = Prefs {
            spell_check: true,
            scrollbars_always: true,
            branches_search_actions: true,
            branches_by_directory: true,
            branches_recent: true,
            branches_tags: true,
            log_compact_refs: true,
            log_author: true,
            log_date: true,
            log_mark_mine: true,
            log_mark_merges: true,
            log_mark_branch: true,
            log_changes_by_directory: true,
            log_details: true,
            ..Prefs::default()
        };
        assert_eq!(Prefs::parse(""), fresh);
        assert_eq!(
            Prefs::parse("appearance=nonsense\n").appearance,
            Appearance::System
        );
    }
}
