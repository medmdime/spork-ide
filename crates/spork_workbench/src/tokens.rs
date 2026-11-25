//! IntelliJ's Islands design tokens: the colors of ManyIslandsDark / ManyIslandsLight
//! (intellij-community themes/islands, Apache-2.0), its type scale and its control metrics
//! (docs/reference-spec.md §10). Every control in `controls` is built from these alone.

use std::sync::LazyLock;

use gpui_kit::*;

/// The semantic colors of one theme, named after IntelliJ's keys.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    /// `main-window-bg`: behind the islands, the toolbar, the stripes, the status bar.
    pub frame: Hsla,
    /// `tool-window-bg`: the islands.
    pub island: Hsla,
    /// `popup-bg` and `popup-border`: menus and popups.
    pub popup: Hsla,
    pub popup_border: Hsla,
    /// `feedback-bg` and `feedback-border`: notification balloons (`Notification.background`,
    /// `Notification.borderColor`) and tooltips.
    pub feedback: Hsla,
    pub feedback_border: Hsla,
    /// `Notification.MoreButton.background`: the "N more notifications" stub under the balloons.
    pub balloon_more: Hsla,
    /// `Notification.Shadow.*1Color`: the inner end of a balloon's 5 px shadow.
    pub balloon_shadow: Hsla,
    /// `IconBadge.errorBackground` and `IconBadge.infoBackground` (unset in Islands: JBUI's): the
    /// dot on the Notifications stripe button while a notification is unread, red for an error.
    pub icon_badge_error: Hsla,
    pub icon_badge_info: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_secondary: Hsla,
    pub text_disabled: Hsla,
    pub text_link: Hsla,
    pub text_error: Hsla,
    /// An inspection tree's error count, `InspectionTreeTailRenderer`'s TREE_RED.
    pub text_error_count: Hsla,
    /// `JBColor.red` (`Color.red`, `DarculaColors.RED`): the wavy line under a file with problems
    /// in Recent Files and the Switcher (`SwitcherRendering`'s effect colour).
    pub problem_wave: Hsla,
    /// `NotificationsToolwindow.newNotification.background` (Islands: Gray3, Gray12): an unread
    /// notification in the Notifications tool window, and its hover.
    pub notification_unread: Hsla,
    pub text_warning: Hsla,
    pub text_success: Hsla,
    /// `icon-default-stroke`, the color of mono icons.
    pub icon: Hsla,
    pub icon_disabled: Hsla,
    /// `accent-brand-bg`: the default button, the open stripe button.
    pub accent: Hsla,
    /// `control-brand-border`: the 2 px focus ring.
    pub focus: Hsla,
    pub control_bg: Hsla,
    /// A non-editable combo box's fill.
    pub control_bg_raised: Hsla,
    /// `ProgressBar.trackColor` (`control-bg-small`): a progress bar's track; its progress is
    /// `accent` (`control-brand-bg`).
    pub progress_track: Hsla,
    pub control_border: Hsla,
    /// `control-border-raised`: a segmented button's chosen segment
    /// (`SegmentedButton.selectedStartBorderColor`).
    pub control_border_raised: Hsla,
    pub control_border_disabled: Hsla,
    /// `dialog-border`: lines between areas.
    pub separator: Hsla,
    pub toolbar_border: Hsla,
    /// `toolbar-bg-hovered` / `toolbar-bg-pressed`: icon buttons, toolbar widgets, tabs.
    pub hover: Hsla,
    pub pressed: Hsla,
    /// Lists and trees: the focused selection, the unfocused one, the hovered row.
    pub selection: Hsla,
    pub selection_inactive: Hsla,
    pub selection_hover: Hsla,
    /// A selected tab while its area has the focus, and otherwise.
    pub tab_selected: Hsla,
    pub tab_selected_border: Hsla,
    pub tab_selected_inactive: Hsla,
    pub tab_selected_border_inactive: Hsla,
    pub checkbox_fill: Hsla,
    pub checkbox_border: Hsla,
    pub checkbox_checked: Hsla,
    /// `Checkbox.Background.Disabled`, `Checkbox.Border.Disabled` and
    /// `Checkbox.Foreground.Disabled` (the tick and the radio's dot): a greyed check box or radio.
    pub checkbox_fill_disabled: Hsla,
    pub checkbox_border_disabled: Hsla,
    pub checkbox_mark_disabled: Hsla,
    /// `toggle-off-bg`: an on/off switch's track while off (on, it is `accent`; its notch white),
    /// `DarculaOnOffButtonUI-spec.md` §3.1.
    pub toggle_off_bg: Hsla,
    /// The editor scheme's `CONSOLE_NORMAL_OUTPUT`, `CONSOLE_SYSTEM_OUTPUT` (the command lines)
    /// and `CONSOLE_ERROR_OUTPUT` (Islands Dark; the light scheme inherits IntelliJ's Default).
    pub console_text: Hsla,
    pub console_system: Hsla,
    pub console_error: Hsla,
    /// `Badge.blueBackground` / `Badge.blueForeground`: the "New" badge on a tab.
    pub badge: Hsla,
    pub badge_text: Hsla,
    /// `Badge.purpleSecondaryBackground` / `Badge.purpleSecondaryForeground`: the "Beta" badge.
    pub badge_beta: Hsla,
    pub badge_beta_text: Hsla,
    /// `Badge.graySecondaryBackground` / `Badge.graySecondaryForeground`: the Settings tree's
    /// count on Plugins.
    pub badge_gray: Hsla,
    pub badge_gray_text: Hsla,
    /// `search-match-bg` (`SearchMatch.startBackground`): what a search found, behind its text
    /// (the Settings dialog's options).
    pub search_match: Hsla,
    /// The text over a search match in lists and trees (`SimpleColoredComponent`'s
    /// `JBColor(Gray._50, Gray._0)` for `STYLE_SEARCH_MATCH`): speed search's highlights.
    pub search_match_text: Hsla,
    /// The outline the Settings dialog's search draws around an option it found
    /// (`Settings.Spotlight.borderColor`, unset in Islands: `SpotlightPainter`'s Orange4 / Orange6).
    pub spotlight: Hsla,
    /// `SearchField.errorBackground`: a search field that found nothing.
    pub search_error: Hsla,
    /// `*.background`: a dialog's panels (Islands Dark's tool-window-bg, Islands Light's
    /// dialog-bg), the Settings dialog's tree (`SidePanel.background`) and pages.
    pub panel: Hsla,
    /// `Shortcut.foreground` and `Shortcut.borderColor`: the keymap's shortcut chips.
    pub shortcut_text: Hsla,
    pub shortcut_border: Hsla,
    /// The Plugins page (`Plugins.*`): its list's and details' fill, the line between them, a
    /// group's header, the card selected (`lightSelectionBackground`), the Install button's
    /// outline and text (`Button.installBorderColor`, `installForeground`).
    pub plugin_background: Hsla,
    pub plugin_border: Hsla,
    pub plugin_section: Hsla,
    pub plugin_selection: Hsla,
    pub install_border: Hsla,
    pub install_text: Hsla,
    /// `Counter.background` / `Counter.foreground`: a tab's count.
    pub counter: Hsla,
    pub counter_text: Hsla,
    /// `VersionControl.Log.Commit.currentBranchBackground`: the log's Current Branch highlight.
    pub log_current_branch: Hsla,
    /// `RunWidget.runningBackground` (`toolbar-run-bg`) and `RunWidget.stopBackground`
    /// (`toolbar-stop-bg`): the main toolbar's Merge/Rebase widget, its continue and abort
    /// buttons; `icon-over-accent`, their icons.
    pub toolbar_run_bg: Hsla,
    pub toolbar_stop_bg: Hsla,
    pub icon_over_accent: Hsla,
    /// `control-border-over-accent` (`Button.Split.default.separatorColor`): the line before the
    /// arrow of a default option button.
    pub control_border_over_accent: Hsla,
    /// `VersionControl.MarkerPopup.borderColor`: the editor's line-status popup.
    pub marker_popup_border: Hsla,
    /// The editor scheme's `ANNOTATIONS_COLOR` and `ANNOTATIONS_LAST_COMMIT_COLOR`: Annotate's
    /// text, and that of the lines of the file's last commit.
    pub annotation: Hsla,
    pub annotation_last_commit: Hsla,
    /// `VCS_ANNOTATIONS_COLOR_1` to `_5`: the anchors of Annotate's shades, newest first.
    pub annotation_order: [Hsla; 5],
    /// `WelcomeScreen.SidePanel.background`: the Welcome window's sidebar (reference-spec.md §1).
    pub welcome_sidebar: Hsla,
    /// The Welcome window's large buttons with no recent project (reference-spec.md §1, Empty
    /// state).
    pub welcome_button: Hsla,
    /// `Editor.SearchField.background` (`editor-bg-inline`) and `Editor.SearchField.borderColor`
    /// (`editor-border-inline`): the editor's find and replace bar.
    pub editor_search: Hsla,
    pub editor_search_border: Hsla,
    /// `SearchOption.selectedBackground` (`toolbar-selected-bg`): a search option turned on (Cc,
    /// W, .*), a toggled toolbar button.
    pub toolbar_selected: Hsla,
    /// `Bookmark.MnemonicAssigned.background` (`control-warning-bg`): a mnemonic already used, in
    /// the mnemonic chooser; `Bookmark.MnemonicCurrent.background` is `accent`, the text on both
    /// `icon_over_accent`, a free one's border `control_border`.
    pub mnemonic_assigned: Hsla,
    /// The editor scheme's `FOLDED_TEXT_ATTRIBUTES`: a folded region's placeholder, its text and
    /// its fill.
    pub folded_text: Hsla,
    pub folded_text_bg: Hsla,
    /// The editor scheme's `TODO_DEFAULT_ATTRIBUTES` foreground (Islands' `IslandSchemeDark` and
    /// `expUI_lightScheme`): a TODO item's text, in italics.
    pub todo: Hsla,
    /// The editor scheme's `ERROR_STRIPE_COLOR` of `ERRORS_ATTRIBUTES` and `WARNING_ATTRIBUTES`,
    /// and `INFO_ATTRIBUTES`' effect colour (weak warnings have no stripe colour): the inspection
    /// widget's icons.
    pub inspection_error: Hsla,
    pub inspection_warning: Hsla,
    pub inspection_weak_warning: Hsla,
    /// The editor scheme's `ActionButton.iconTextForeground`: the inspection widget's counts.
    pub inspection_count: Hsla,
    /// The editor scheme's `ActionButton.hoverBackground` and `ActionButton.pressedBackground`:
    /// buttons drawn over the editor (`EditorToolbarButtonLook`), the inspection widget's.
    pub editor_button_hover: Hsla,
    pub editor_button_pressed: Hsla,
    /// The editor scheme's `LOOKUP_COLOR` (and `DOCUMENTATION_COLOR`, the same): the completion
    /// list's and the documentation popup's fill.
    pub lookup: Hsla,
    /// `CompletionPopup.matchForeground`: what the typed text matched in a completion.
    pub lookup_match: Hsla,
    /// `CompletionPopup.Advertiser.background`: the completion popup's bottom row.
    pub popup_advertiser: Hsla,
    /// Parameter info in the New UI (`ParameterInfoComponent`): its border (the editor's
    /// background blended with grey at 0.1), the matching signature's fill (blended with green
    /// at 0.05 / 0.15) and the other signatures' text (blended at 0.5 / 0.75).
    pub parameter_info_border: Hsla,
    pub parameter_info_current: Hsla,
    pub parameter_info_other: Hsla,
    /// The editor scheme's `LIVE_TEMPLATE_ATTRIBUTES` effect color (Islands Dark `467ff2`, Default
    /// `0055ff`): the box around the name an in-place rename edits; a translucent fill, as Zed's
    /// editor draws no boxed highlight.
    pub live_template: Hsla,
    /// The editor scheme's `ERROR_HINT` and `INFORMATION_HINT` (`HintUtil`) and `HINT_BORDER`:
    /// the hints shown over the caret ("No extract action is available here").
    pub error_hint: Hsla,
    pub information_hint: Hsla,
    pub hint_border: Hsla,
    /// The editor scheme's `DIFF_INSERTED` and `DIFF_MODIFIED` backgrounds (Darcula's under
    /// Islands Dark; Default's and IntelliJ Light's): text an intention's preview adds or changes.
    pub diff_inserted: Hsla,
    pub diff_modified: Hsla,
    /// `Banner.warningBackground` and `Banner.warningBorderColor` (`feedback-warning-bg` and
    /// `-border`): a warning editor banner (`EditorNotificationPanel.Status.Warning`).
    pub banner_warning: Hsla,
    pub banner_warning_border: Hsla,
}

/// An opaque color, `0xRRGGBB`.
fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// A translucent color, `0xRRGGBBAA` (a separate function: `0x00000020`, black at 12 %, can't
/// be told from an opaque `0x000020` by its value).
fn alpha(hex: u32) -> Hsla {
    rgba(hex).into()
}

static DARK: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    frame: color(0x26282C),
    island: color(0x191A1C),
    popup: color(0x26282C),
    popup_border: color(0x33353B),
    feedback: color(0x33353B),
    feedback_border: color(0x33353B),
    balloon_more: color(0x191A1C),
    balloon_shadow: alpha(0x00000010),
    icon_badge_error: color(0xDB5C5C),
    icon_badge_info: color(0x548AF7),
    text: color(0xD1D3D9),
    text_muted: color(0x9FA2A8),
    text_secondary: color(0x73767C),
    text_disabled: color(0x4C4F56),
    text_link: color(0x71A1FE),
    text_error: color(0xF57E84),
    text_error_count: color(0xCC6666),
    problem_wave: color(0xFF6464),
    notification_unread: color(0x393B40),
    text_warning: color(0xD59637),
    text_success: color(0x6DB083),
    icon: color(0xC3C5CB),
    icon_disabled: color(0x5F6269),
    accent: color(0x3871E1),
    focus: color(0x3871E1),
    control_bg: color(0x191A1C),
    control_bg_raised: color(0x26282C),
    progress_track: color(0x40434A),
    control_border: color(0x40434A),
    control_border_raised: color(0x5F6269),
    control_border_disabled: color(0x33353B),
    separator: color(0x26282C),
    toolbar_border: alpha(0xFFFFFF21),
    hover: alpha(0xFFFFFF17),
    pressed: alpha(0xFFFFFF29),
    selection: color(0x2A4371),
    selection_inactive: color(0x33353B),
    selection_hover: alpha(0xFFFFFF10),
    tab_selected: color(0x233558),
    tab_selected_border: color(0x2E4D89),
    tab_selected_inactive: color(0x26282C),
    tab_selected_border_inactive: color(0x40434A),
    checkbox_fill: color(0x2B2D30),
    checkbox_border: color(0x5A5D63),
    checkbox_checked: color(0x3574F0),
    checkbox_fill_disabled: color(0x393B40),
    checkbox_border_disabled: color(0x5A5D63),
    checkbox_mark_disabled: color(0x6F737A),
    toggle_off_bg: alpha(0xFFFFFF29),
    console_text: color(0xBCBEC4),
    console_system: color(0xBCBEC4),
    console_error: color(0xF75464),
    badge: color(0x538AF9),
    badge_text: color(0x212326),
    badge_beta: alpha(0x6C4EBBB3),
    badge_beta_text: color(0xE2DBFC),
    badge_gray: alpha(0xFFFFFF29),
    badge_gray_text: alpha(0xFFFFFFCC),
    search_match: color(0xBA9752),
    search_match_text: color(0x000000),
    spotlight: color(0xA36B4E),
    search_error: color(0x56272B),
    panel: color(0x191A1C),
    shortcut_text: color(0x9FA2A8),
    shortcut_border: alpha(0xFFFFFF3B),
    plugin_background: color(0x191A1C),
    plugin_border: color(0x26282C),
    plugin_section: color(0x212326),
    plugin_selection: color(0x233558),
    install_border: color(0x338555),
    install_text: color(0x6DB083),
    counter: color(0x73767C),
    counter_text: color(0x000000),
    log_current_branch: color(0x1D2336),
    toolbar_run_bg: color(0x338555),
    toolbar_stop_bg: color(0xC54E58),
    icon_over_accent: color(0xFFFFFF),
    control_border_over_accent: alpha(0xFFFFFF80),
    marker_popup_border: color(0x43454A),
    annotation: color(0x8D9199),
    annotation_last_commit: color(0xCED0D6),
    annotation_order: [
        color(0x2F5194),
        color(0x2D416B),
        color(0x283754),
        color(0x242D42),
        color(0x22252E),
    ],
    welcome_sidebar: color(0x191A1C),
    welcome_button: color(0x2B2D30),
    editor_search: color(0x212326),
    editor_search_border: color(0x26282C),
    toolbar_selected: color(0x2E4D89),
    mnemonic_assigned: color(0xA56906),
    folded_text: color(0x868991),
    folded_text_bg: color(0x393B40),
    todo: color(0x8BB33D),
    inspection_error: color(0xD64D5B),
    inspection_warning: color(0xC29E4A),
    inspection_weak_warning: color(0x857042),
    inspection_count: color(0xBBBBBB),
    editor_button_hover: color(0x4C5052),
    editor_button_pressed: color(0x5C6164),
    lookup: color(0x27282B),
    lookup_match: color(0x538AF9),
    popup_advertiser: color(0x26282C),
    parameter_info_border: color(0x2F2F30),
    parameter_info_current: color(0x212A21),
    parameter_info_other: color(0x86888C),
    live_template: alpha(0x467FF247),
    error_hint: color(0x402929),
    information_hint: color(0x2B2D30),
    hint_border: color(0x393B40),
    diff_inserted: color(0x294436),
    diff_modified: color(0x385570),
    banner_warning: color(0x44321D),
    banner_warning_border: color(0x694820),
});

static LIGHT: LazyLock<Tokens> = LazyLock::new(|| Tokens {
    frame: color(0xE9EAEE),
    island: color(0xFFFFFF),
    popup: color(0xFFFFFF),
    popup_border: color(0xE9EAEE),
    feedback: color(0xFFFFFF),
    feedback_border: color(0xD1D3D9),
    balloon_more: color(0xF7F8F9),
    balloon_shadow: alpha(0x80808010),
    icon_badge_error: color(0xE55765),
    icon_badge_info: color(0x588CF3),
    text: color(0x000000),
    text_muted: color(0x5F6269),
    text_secondary: color(0x73767C),
    text_disabled: color(0x9FA2A8),
    text_link: color(0x2F5EB9),
    text_error: color(0xC54E58),
    text_error_count: color(0xB84237),
    problem_wave: color(0xFF0000),
    notification_unread: color(0xEBECF0),
    text_warning: color(0xA56906),
    text_success: color(0x338555),
    icon: color(0x73767C),
    icon_disabled: color(0xC3C5CB),
    accent: color(0x3871E1),
    focus: color(0x3871E1),
    control_bg: color(0xFFFFFF),
    control_bg_raised: color(0xFFFFFF),
    progress_track: color(0xDDDFE4),
    control_border: color(0xD1D3D9),
    control_border_raised: color(0xB5B7BD),
    control_border_disabled: color(0xE9EAEE),
    separator: color(0xE9EAEE),
    toolbar_border: alpha(0x00000020),
    hover: alpha(0x00000012),
    pressed: alpha(0x00000020),
    selection: color(0xD0DFFE),
    selection_inactive: color(0xE9EAEE),
    selection_hover: alpha(0x00000008),
    tab_selected: color(0xE3EBFE),
    tab_selected_border: color(0xA7C5FF),
    tab_selected_inactive: color(0xE9EAEE),
    tab_selected_border_inactive: color(0xD1D3D9),
    checkbox_fill: color(0xFFFFFF),
    checkbox_border: color(0xA8ADBD),
    checkbox_checked: color(0x3574F0),
    checkbox_fill_disabled: color(0xF7F8FA),
    checkbox_border_disabled: color(0xDFE1E5),
    checkbox_mark_disabled: color(0xC9CCD6),
    toggle_off_bg: alpha(0x00000045),
    console_text: color(0x000000),
    console_system: color(0x00007F),
    console_error: color(0x7F0000),
    badge: color(0x3871E1),
    badge_text: color(0xFFFFFF),
    badge_beta: alpha(0x8060DB29),
    badge_beta_text: color(0x6C4EBB),
    badge_gray: alpha(0x191A1C1A),
    badge_gray_text: color(0x5F6269),
    search_match: color(0xFEE6B1),
    search_match_text: color(0x323232),
    spotlight: color(0xE08855),
    search_error: color(0xFFF6F5),
    panel: color(0xF7F8F9),
    shortcut_text: color(0x5F6269),
    shortcut_border: alpha(0x00000045),
    plugin_background: color(0xFFFFFF),
    plugin_border: color(0xE9EAEE),
    plugin_section: color(0xF7F8F9),
    plugin_selection: color(0xE3EBFE),
    install_border: color(0x4E9D6C),
    install_text: color(0x338555),
    counter: color(0x8B8E94),
    counter_text: color(0xFFFFFF),
    log_current_branch: color(0xEDF3FF),
    toolbar_run_bg: color(0x4E9D6C),
    toolbar_stop_bg: color(0xE4656E),
    icon_over_accent: color(0xFFFFFF),
    control_border_over_accent: alpha(0xFFFFFF80),
    marker_popup_border: color(0xDFE1E5),
    annotation: color(0x5A5D6B),
    annotation_last_commit: color(0x000000),
    annotation_order: [
        color(0xA1BEF7),
        color(0xC9DAFB),
        color(0xDEE8FC),
        color(0xEDF2FA),
        color(0xF7F8FA),
    ],
    welcome_sidebar: color(0xF7F8F9),
    welcome_button: color(0xF7F8F9),
    editor_search: color(0xF7F8F9),
    editor_search_border: color(0xE9EAEE),
    toolbar_selected: color(0xD0DFFE),
    mnemonic_assigned: color(0xC28013),
    folded_text: color(0x414D41),
    folded_text_bg: color(0xE9F5E6),
    todo: color(0x008DDE),
    inspection_error: color(0xCF5B56),
    inspection_warning: color(0xF2BF57),
    inspection_weak_warning: color(0xD9CFAD),
    inspection_count: color(0x6E6E6E),
    editor_button_hover: color(0xDFDFDF),
    editor_button_pressed: color(0xCFCFCF),
    lookup: color(0xFFFFFF),
    lookup_match: color(0x2F5EB9),
    popup_advertiser: color(0xF7F8F9),
    parameter_info_border: color(0xF5F5F5),
    parameter_info_current: color(0xEBFFEB),
    parameter_info_other: color(0x808080),
    live_template: alpha(0x0055FF2E),
    error_hint: color(0xFFDCDC),
    information_hint: color(0xF7F7F7),
    hint_border: color(0xE0E0E0),
    diff_inserted: color(0xBEE6BE),
    diff_modified: color(0xC2D8F2),
    banner_warning: color(0xFFF6E9),
    banner_warning_border: color(0xF4CD9A),
});

impl Tokens {
    /// The tokens of the current theme; the dark one's in an app without gpui-kit's themes (the
    /// IDE crates' tests).
    pub fn get(cx: &App) -> &'static Tokens {
        let dark = cx
            .try_global::<gpui_kit::component::Theme>()
            .is_none_or(|theme| theme.mode.is_dark());
        match dark {
            true => &DARK,
            false => &LIGHT,
        }
    }

    /// The tokens of the dark or the light theme, for the IDE's own crates, which read the
    /// appearance from the IDE's theme rather than gpui-kit's.
    pub fn for_appearance(dark: bool) -> &'static Tokens {
        match dark {
            true => &DARK,
            false => &LIGHT,
        }
    }

    /// Whether these are the dark theme's tokens, for icons that have a dark variant.
    pub fn is_dark(&self) -> bool {
        std::ptr::eq(self, &*DARK)
    }
}

/// IntelliJ's control metrics, in pixels.
pub mod metrics {
    /// Lists, trees and popup rows.
    pub const ROW_HEIGHT: f32 = 24.;
    /// Buttons, text fields and combo boxes.
    pub const CONTROL_HEIGHT: f32 = 28.;
    pub const BUTTON_MIN_WIDTH: f32 = 72.;
    pub const BUTTON_PADDING_X: f32 = 14.;
    /// Controls' corners (IntelliJ's arc 8 is a diameter).
    pub const RADIUS: f32 = 4.;
    /// Main toolbar widgets, stripe buttons and editor tabs.
    pub const RADIUS_LARGE: f32 = 6.;
    /// A tool-window stripe's width; the islands start right after it.
    pub const STRIPE_WIDTH: f32 = 41.;
    pub const FOCUS_RING: f32 = 2.;
    /// Icon buttons in tool-window toolbars, and their icons.
    pub const TOOLBAR_BUTTON: f32 = 22.;
    pub const TOOLBAR_ICON: f32 = 16.;
    /// Icon buttons in the main toolbar, and their icons.
    pub const MAIN_TOOLBAR_BUTTON: f32 = 30.;
    pub const MAIN_TOOLBAR_ICON: f32 = 20.;
    /// Between the main toolbar's groups (`MainToolbar`'s `layoutGap` 10) and between its icon
    /// buttons (Islands' `MainToolbar.Icon.insets` 6,5,4,5: 5 on each side).
    pub const MAIN_TOOLBAR_GAP: f32 = 10.;
    /// From the main toolbar's last button to the window's right edge.
    pub const MAIN_TOOLBAR_END_INSET: f32 = 6.;
    /// Between the run widget's buttons, measured on the catalogue's screenshots (a 32 px pitch).
    pub const RUN_WIDGET_GAP: f32 = 2.;
    /// The widest a main toolbar dropdown's text runs (the VCS widget's branch, the run widget's
    /// configuration) before its end is cut: `MainToolbar.Dropdown.maxWidth` (reference-spec.md,
    /// Main toolbar).
    pub const MAIN_TOOLBAR_DROPDOWN_MAX_WIDTH: f32 = 350.;
    /// The widest the project widget's name runs (reference-spec.md, Main toolbar).
    pub const PROJECT_WIDGET_MAX_WIDTH: f32 = 500.;
    pub const CHECKBOX: f32 = 16.;
    /// An on/off switch's track (`DarculaOnOffButtonUI-spec.md` §2).
    pub const SWITCH_WIDTH: f32 = 26.;
    pub const SWITCH_HEIGHT: f32 = 16.;
    /// A progress bar's stripe (`DarculaProgressBarUI`'s default 4) and length (Swing's
    /// `ProgressBar.horizontalSize`, 146).
    pub const PROGRESS_BAR_HEIGHT: f32 = 4.;
    pub const PROGRESS_BAR_WIDTH: f32 = 146.;
}

/// IntelliJ's type scale from its 13 px default, following the interface text size the user
/// picked (it sets the rem: 13 px is 0.875 rem).
pub trait Typography: Styled + Sized {
    /// Regular text: 13 px by default.
    fn text_ui(self) -> Self {
        self.text_size(ui_size(13.))
    }
    /// "Medium": popup footers, help descriptions, ref labels; 12 px.
    fn text_ui_small(self) -> Self {
        self.text_size(ui_size(12.))
    }
    /// "Small": version numbers, counters; 11 px.
    fn text_ui_xsmall(self) -> Self {
        self.text_size(ui_size(11.))
    }
    /// h2 (JBFont.h2): the Clone dialog's "Log In to GitHub"; 18 px.
    fn text_h2(self) -> Self {
        self.text_size(ui_size(18.))
    }
    /// h3: section headers in the Welcome window's Customize page; 16 px.
    fn text_h3(self) -> Self {
        self.text_size(ui_size(16.))
    }
    /// h0: the Welcome window's title; 25 px bold (the reference app draws 26).
    fn text_h0(self) -> Self {
        self.text_size(ui_size(26.)).font_weight(FontWeight::BOLD)
    }
}

impl<T: Styled> Typography for T {}

/// `points` of IntelliJ's 13 px scale, as rems.
fn ui_size(points: f32) -> Rems {
    rems(0.875 * points / 13.)
}

#[cfg(test)]
mod tests {
    use super::{DARK, LIGHT, ui_size};

    #[test]
    fn colors_keep_their_alpha() {
        assert!((DARK.hover.a - 0x17 as f32 / 255.).abs() < 0.01);
        // black at 12 %, not the opaque navy #000020
        assert!((LIGHT.pressed.a - 0x20 as f32 / 255.).abs() < 0.01);
        assert_eq!(LIGHT.pressed.l, 0.);
        assert_eq!(LIGHT.island.a, 1.);
    }

    #[test]
    fn the_scale_starts_at_13_px() {
        assert_eq!(ui_size(13.).0, 0.875);
    }
}
