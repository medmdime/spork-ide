//! IntelliJ's icons, by their path under `assets/icons` without `.svg`: `"vcs/push"`.

use std::collections::HashSet;
use std::sync::LazyLock;

use gpui_kit::component::{ActiveTheme as _, Icon, set_icon_resolver};
use gpui_kit::{App, Img, SharedString, Styled as _, img, px};

use crate::{Embedded, KIT, PREFIX};

/// An icon drawn in the text color, like gpui-kit's own: for one-color icons (most toolbar and
/// action icons). Fits anywhere an `Icon` goes: buttons, menu items.
pub fn icon(name: &str) -> Icon {
    Icon::empty().path(format!("{PREFIX}icons/{name}.svg"))
}

/// Whether the icon has a 20 px variant (`name@20x20`), as the main toolbar's buttons take.
pub(crate) fn has_large(name: &str) -> bool {
    static LARGE: LazyLock<HashSet<String>> = LazyLock::new(|| {
        Embedded::iter()
            .filter_map(|n| {
                Some(
                    n.strip_suffix("@20x20.svg")?
                        .strip_prefix("icons/")?
                        .to_owned(),
                )
            })
            .collect()
    });
    LARGE.contains(name)
}

/// An icon in its own colors, 16 px, its dark variant in a dark theme: file types, folders.
pub fn image(name: &str, cx: &App) -> Img {
    image_in(name, cx.theme().mode.is_dark())
}

/// `image` in the dark or the light theme, for the IDE's own crates, which read the appearance
/// from the IDE's theme rather than gpui-kit's.
pub fn image_in(name: &str, dark: bool) -> Img {
    static DARK: LazyLock<HashSet<String>> = LazyLock::new(|| {
        Embedded::iter()
            .filter_map(|n| {
                Some(
                    n.strip_suffix("_dark.svg")?
                        .strip_prefix("icons/")?
                        .to_owned(),
                )
            })
            .collect()
    });
    let dark = dark && DARK.contains(name);
    let suffix = if dark { "_dark" } else { "" };
    img(SharedString::from(format!(
        "{PREFIX}icons/{name}{suffix}.svg"
    )))
    .size(px(16.))
    .flex_none()
}

/// The file-type icon for `path`, for `image`.
pub fn file_icon(path: &str) -> &'static str {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    match name.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".ignore" => return "fileTypes/gitignore",
        "dockerfile" | "containerfile" => return "fileTypes/docker",
        "jenkinsfile" => return "fileTypes/jenkins",
        ".editorconfig" => return "fileTypes/editorConfig",
        _ => {}
    }
    let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext {
        "md" | "markdown" => "fileTypes/markdown",
        "json" | "jsonc" | "json5" => "fileTypes/json",
        "yml" | "yaml" => "fileTypes/yaml",
        "toml" => "fileTypes/toml",
        "xml" | "plist" | "xib" | "storyboard" => "fileTypes/xml",
        "html" | "htm" => "fileTypes/html",
        "css" | "scss" | "sass" | "less" => "fileTypes/css",
        "js" | "mjs" | "cjs" | "jsx" => "fileTypes/javaScript",
        "ts" | "tsx" | "mts" | "cts" => "fileTypes/typeScript",
        "java" => "fileTypes/java",
        "class" => "fileTypes/javaClass",
        "kt" => "fileTypes/kotlin",
        "kts" => "fileTypes/kotlinGradleScript",
        "py" | "pyi" => "fileTypes/python",
        "ipynb" => "fileTypes/jupyter",
        "c" => "fileTypes/c",
        "cc" | "cpp" | "cxx" | "mm" => "fileTypes/cpp",
        "h" | "hh" | "hpp" => "fileTypes/h",
        "cs" => "fileTypes/Csharp",
        "swift" => "fileTypes/swiftLang",
        "sh" | "bash" | "zsh" | "fish" => "fileTypes/shell",
        "bat" | "cmd" | "ps1" => "fileTypes/microsoftWindows",
        "sql" => "fileTypes/sql",
        "csv" | "tsv" => "fileTypes/csv",
        "gradle" => "fileTypes/gradle",
        "graphql" | "gql" => "fileTypes/graphql",
        "vue" => "fileTypes/vue",
        "tf" | "tfvars" => "fileTypes/terraform",
        "patch" | "diff" => "fileTypes/patch",
        "http" | "rest" => "fileTypes/http",
        "properties" => "fileTypes/properties",
        "ini" | "cfg" | "conf" | "env" => "fileTypes/config",
        "rst" => "fileTypes/rst",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "icns" | "bmp" | "tiff" => {
            "fileTypes/image"
        }
        "zip" | "gz" | "tgz" | "tar" | "xz" | "bz2" | "7z" | "jar" | "war" => "fileTypes/archive",
        "ttf" | "otf" | "woff" | "woff2" => "fileTypes/font",
        "txt" | "log" | "lock" | "" => "fileTypes/text",
        _ => "fileTypes/anyType",
    }
}

/// gpui-kit icon (`icons/<name>.svg`) → the IntelliJ icon drawn in its place. One-color icons
/// only: an icon is drawn as a mask in the text color, so a second color would fill in.
fn intellij_for(lucide: &str) -> Option<&'static str> {
    let stem = lucide.strip_prefix("icons/")?.strip_suffix(".svg")?;
    Some(match stem {
        "chevron-down" | "chevrons-up-down" => "general/chevronDown",
        "chevron-up" => "general/chevronUp",
        "chevron-right" => "general/chevronRight",
        "chevron-left" => "general/chevronLeft",
        "close" | "x" => "general/close",
        "circle-x" => "inline/inlineClose",
        "check" => "actions/checked",
        "search" => "general/search",
        "plus" => "general/add",
        "minus" => "general/remove",
        "info" => "status/infoOutline",
        "triangle-alert" => "status/warningOutline",
        "copy" => "general/copy",
        "ellipsis" => "general/moreHorizontal",
        "ellipsis-vertical" => "general/moreVertical",
        "refresh-cw" => "general/refresh",
        "external-link" => "ide/externalLink",
        "eye" => "general/show",
        "eye-off" => "general/hide",
        "undo-2" => "general/undo",
        "git-branch" | "git-fork" => "general/vcs",
        "git-commit-horizontal" => "vcs/commit",
        "git-pull-request" => "github/pullRequests",
        "archive" => "vcs/shelve",
        "tag" => "dvcs/branchLabel",
        "globe" => "toolwindows/web",
        "cloud-download" => "vcs/fetch",
        "clock" => "general/history",
        "settings" => "general/settings",
        "case-sensitive" => "inline/matchCase",
        "asterisk" => "inline/regex",
        "file-pen" => "toolwindows/commit",
        _ => return None,
    })
}

pub(crate) fn install(cx: &mut App) {
    set_icon_resolver(cx, |path, _| match intellij_for(path) {
        Some(name) => Some(format!("{PREFIX}icons/{name}.svg").into()),
        None => path
            .starts_with("icons/")
            .then(|| format!("{KIT}{path}").into()),
    });
}

#[cfg(test)]
mod tests {
    use super::{Embedded, file_icon, intellij_for};

    #[test]
    fn every_named_icon_exists() {
        let lucide = [
            "chevron-down",
            "chevrons-up-down",
            "chevron-up",
            "chevron-right",
            "chevron-left",
            "close",
            "x",
            "circle-x",
            "check",
            "search",
            "plus",
            "minus",
            "info",
            "triangle-alert",
            "copy",
            "ellipsis",
            "ellipsis-vertical",
            "refresh-cw",
            "external-link",
            "eye",
            "eye-off",
            "undo-2",
            "git-branch",
            "git-fork",
            "git-commit-horizontal",
            "git-pull-request",
            "archive",
            "tag",
            "globe",
            "cloud-download",
            "clock",
            "settings",
            "case-sensitive",
            "asterisk",
            "file-pen",
        ];
        for name in lucide {
            let ij = intellij_for(&format!("icons/{name}.svg")).expect(name);
            assert!(Embedded::get(&format!("icons/{ij}.svg")).is_some(), "{ij}");
        }
        for path in [
            "a/README.md",
            "x.rs",
            "Dockerfile",
            "a.tsx",
            "b.kt",
            "c.py",
            "img.png",
            ".gitignore",
        ] {
            let icon = file_icon(path);
            assert!(
                Embedded::get(&format!("icons/{icon}.svg")).is_some(),
                "{path}: {icon}"
            );
        }
    }

    #[test]
    fn gpui_kit_icons_load_from_their_prefix() {
        use gpui_kit::AssetSource as _;
        let check = |path| crate::Assets.load(path).ok().flatten();
        assert!(check("kit/icons/check.svg").is_some());
        assert_eq!(check("kit/icons/check.svg"), check("icons/check.svg"));
        assert!(crate::Assets::serves("kit/icons/check.svg"));
        assert!(!crate::Assets::serves("icons/check.svg"));
    }
}
