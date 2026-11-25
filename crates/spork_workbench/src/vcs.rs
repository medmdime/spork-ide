//! IntelliJ's version-control colors: file names in the color of their change, and the branch
//! and tag labels of the commit log.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla, rgb};

/// What happened to a file, as version control sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Unversioned,
    Ignored,
    Conflicted,
}

/// The color of a file name with `status`, from IntelliJ's light and dark color schemes.
pub fn file_status_color(status: FileStatus, cx: &App) -> Hsla {
    use FileStatus::*;
    let (dark, light) = match status {
        Modified | Renamed => (0x70AEFF, 0x0033B3),
        Added => (0x73BD79, 0x067D17),
        Deleted => (0x6F737A, 0x6C707E),
        Unversioned => (0xE88F89, 0xB23247),
        Ignored => (0xD69A6B, 0x8C4F00),
        Conflicted => (0xDE6A66, 0xDE1B2E),
    };
    rgb(if cx.theme().mode.is_dark() {
        dark
    } else {
        light
    })
    .into()
}

/// What a label in the commit log names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefLabel {
    /// The checked-out branch (HEAD).
    Head,
    Local,
    Remote,
    Tag,
}

/// The color of a ref label's icon in the commit log.
pub fn ref_label_color(label: RefLabel, cx: &App) -> Hsla {
    let (dark, light) = match label {
        RefLabel::Head => (0xF5D273, 0xFFAF0F),
        RefLabel::Local => (0x5FAD65, 0x369650),
        RefLabel::Remote => (0xB589EC, 0x834DF0),
        RefLabel::Tag => (0x868A91, 0x6C707E),
    };
    rgb(if cx.theme().mode.is_dark() {
        dark
    } else {
        light
    })
    .into()
}

/// How a block of lines changed, for its background in a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffChange {
    /// Only new lines.
    Inserted,
    /// Only removed lines.
    Deleted,
    /// Lines replaced by others: shown on both sides.
    Modified,
}

/// IntelliJ's diff colors: green for inserted lines, gray for deleted ones, blue for modified.
pub fn diff_color(change: DiffChange, cx: &App) -> Hsla {
    let (dark, light) = match change {
        DiffChange::Inserted => (0x294436, 0xC9E9CB),
        DiffChange::Deleted => (0x484A4A, 0xDDDFE4),
        DiffChange::Modified => (0x385570, 0xC2D8F2),
    };
    rgb(if cx.theme().mode.is_dark() {
        dark
    } else {
        light
    })
    .into()
}

/// The text color IntelliJ's log dims merge commits to.
pub fn log_dim_color(cx: &App) -> Hsla {
    rgb(if cx.theme().mode.is_dark() {
        0x6F737A
    } else {
        0x818594
    })
    .into()
}
