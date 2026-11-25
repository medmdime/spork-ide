//! A project's Java language level (IntelliJ's `LanguageLevel`, its list and descriptions) and
//! what Project Structure keeps of a project that no build file holds (IntelliJ's `misc.xml` and
//! module file): its name, language level and compiler output, its module's language level and
//! SDK. A Maven or Gradle project's language level is its build file's (`build_files`).

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

use crate::build_files;
use crate::java_project::Tool;

/// IntelliJ's highest released language level (`java.highest.language.level`).
const HIGHEST: u32 = 27;
/// IntelliJ's `JDK_X`, the experimental features, after every release.
const EXPERIMENTAL: u32 = 29;

/// A Java language level: a feature release, with its preview features or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LanguageLevel {
    pub feature: u32,
    #[serde(default)]
    pub preview: bool,
}

/// IntelliJ's descriptions (`JavaSyntaxBundle`'s `jdk.*.language.level.description`), by
/// feature release, the preview's after.
const DESCRIPTIONS: &[(u32, &str, Option<&str>)] = &[
    (3, "Plain old Java", None),
    (4, "'assert' keyword", None),
    (5, "'enum' keyword, generics, autoboxing, etc.", None),
    (6, "@Override in interfaces", None),
    (
        7,
        "Diamonds, try-with-resources, multi-catch, strings in switch, etc.",
        None,
    ),
    (8, "Lambdas, type annotations, etc.", None),
    (9, "Modules, private methods in interfaces, etc.", None),
    (10, "Local variable type inference", None),
    (11, "Local variable syntax for lambda parameters", None),
    (12, "No new language features", None),
    (13, "No new language features", None),
    (14, "Switch expressions", None),
    (15, "Text blocks", None),
    (16, "Records, patterns, local enums and interfaces", None),
    (
        17,
        "Sealed types, always-strict floating-point semantics",
        None,
    ),
    (18, "JavaDoc snippets", None),
    (19, "No new language features", None),
    (20, "No new language features", None),
    (
        21,
        "Record patterns, pattern matching for switch",
        Some("String templates, unnamed classes and instance main methods, etc."),
    ),
    (
        22,
        "Unnamed variables and patterns",
        Some("Statements before super(), string templates (2nd preview), etc."),
    ),
    (
        23,
        "Markdown documentation comments",
        Some("Primitive types in patterns, implicitly declared classes, etc."),
    ),
    (
        24,
        "No new language features",
        Some("Flexible constructor bodies, simple source files, etc."),
    ),
    (
        25,
        "Compact source files, module imports, etc.",
        Some("Primitive types in patterns, instanceof, and switch (3rd preview)"),
    ),
    (
        26,
        "No new language features",
        Some("Primitive types in patterns, instanceof, and switch (4th preview)"),
    ),
    (
        27,
        "No new language features",
        Some("Primitive types in patterns, instanceof, and switch (5th preview)"),
    ),
    (28, "No new language features", Some("Value classes")),
];

impl LanguageLevel {
    pub const fn new(feature: u32) -> Self {
        Self {
            feature,
            preview: false,
        }
    }

    /// IntelliJ's presentable text: "21 - Record patterns, pattern matching for switch",
    /// "21 (Preview) - …", "1.4 - 'assert' keyword".
    pub fn text(self) -> String {
        if self.feature == EXPERIMENTAL {
            return "X - Experimental features".into();
        }
        let number = match self.feature {
            0..=4 => format!("1.{}", self.feature),
            feature => feature.to_string(),
        };
        let entry = DESCRIPTIONS
            .iter()
            .find(|(feature, ..)| *feature == self.feature);
        match (self.preview, entry) {
            (false, Some((_, description, _))) => format!("{number} - {description}"),
            (true, Some((_, _, Some(description)))) => {
                format!("{number} (Preview) - {description}")
            }
            _ => format!("{number} (Preview) - Unsupported"),
        }
    }

    /// No longer supported: the previews of 17 to 20.
    pub fn unsupported(self) -> bool {
        self.preview && (17..=20).contains(&self.feature)
    }

    /// The long-term releases IntelliJ's list shows in bold.
    pub fn lts(self) -> bool {
        !self.preview && matches!(self.feature, 8 | 11 | 17 | 21 | 25)
    }

    /// As a build file and the compiler take it: `21`, `1.8`.
    pub fn option(self) -> String {
        build_files::java_option(self.feature.min(EXPERIMENTAL - 1))
    }

    /// A build file's value (`21`, `1.8`, `JavaVersion.VERSION_21`'s `21`) as a level.
    pub fn parse(value: &str) -> Option<Self> {
        crate::jdk::major(value.trim()).map(Self::new)
    }

    /// The level a JDK of that release has by default (IntelliJ's `maxLanguageLevel`).
    pub fn of_jdk(major: u32) -> Self {
        Self::new(major.min(EXPERIMENTAL - 1))
    }
}

/// IntelliJ's Language level list (`LanguageLevelCombo`) after its default item, by group:
/// "Supported Versions" (the releases up to the highest, each with its preview), "Experimental
/// Versions" and "Unsupported Versions".
pub fn levels() -> [(&'static str, Vec<LanguageLevel>); 3] {
    let preview = |feature| LanguageLevel {
        feature,
        preview: true,
    };
    let mut supported = Vec::new();
    for feature in (3..=HIGHEST).rev() {
        supported.push(LanguageLevel::new(feature));
        if feature >= 21 {
            supported.push(preview(feature));
        }
    }
    let experimental = vec![
        LanguageLevel::new(EXPERIMENTAL),
        preview(HIGHEST + 1),
        LanguageLevel::new(HIGHEST + 1),
    ];
    let unsupported = (17..=20).rev().map(preview).collect();
    [
        ("Supported Versions", supported),
        ("Experimental Versions", experimental),
        ("Unsupported Versions", unsupported),
    ]
}

/// What Project Structure keeps of the project at `root`, in the data folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// The project's name, when it isn't its folder's.
    #[serde(default)]
    pub name: Option<String>,
    /// A plain folder's language level; none is the SDK's default.
    #[serde(default)]
    pub language_level: Option<LanguageLevel>,
    #[serde(default)]
    pub compiler_output: Option<PathBuf>,
    /// A plain folder's module's language level; none is the project's.
    #[serde(default)]
    pub module_language_level: Option<LanguageLevel>,
    /// The module's JDK home; none is the project's.
    #[serde(default)]
    pub module_jdk: Option<PathBuf>,
}

fn file(root: &Path) -> PathBuf {
    crate::jdtls::project_data(root, "structure").with_extension("json")
}

impl ProjectSettings {
    pub fn load(root: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(file(root)) else {
            return Self::default();
        };
        serde_json::from_str(&text)
            .context("reading the project's structure")
            .log_err()
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let file = file(root);
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&file, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing {file:?}"))
    }

    /// IntelliJ's default compiler output: the project's `out` folder.
    pub fn compiler_output_or_default(&self, root: &Path) -> PathBuf {
        self.compiler_output
            .clone()
            .unwrap_or_else(|| root.join("out"))
    }
}

/// The build file a Maven or Gradle project sets its language level in.
pub fn build_file(root: &Path, tool: Tool) -> Option<PathBuf> {
    match tool {
        Tool::Maven => Some(root.join("pom.xml")),
        Tool::Gradle => ["build.gradle", "build.gradle.kts"]
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.is_file()),
    }
}

/// The language level the project's build sets, a plain folder's as Project Structure keeps it;
/// none is the SDK's default.
pub fn language_level(root: &Path, tool: Option<Tool>) -> Option<LanguageLevel> {
    let Some(tool) = tool else {
        let settings = ProjectSettings::load(root);
        return settings.module_language_level.or(settings.language_level);
    };
    let text = std::fs::read_to_string(build_file(root, tool)?).ok()?;
    let value = match tool {
        Tool::Maven => build_files::maven_language_level(&text),
        Tool::Gradle => build_files::gradle_language_level(&text),
    }?;
    LanguageLevel::parse(&value)
}

/// Writes `level` to the project's build file as it sets one (IntelliJ's "Set language
/// level"); the build file written.
pub fn write_language_level(root: &Path, tool: Tool, level: LanguageLevel) -> Result<PathBuf> {
    let file = build_file(root, tool).context("the project has no build file")?;
    let text = std::fs::read_to_string(&file).with_context(|| format!("reading {file:?}"))?;
    // shortcut: a preview level writes its release without `--enable-preview`; add the
    // compiler argument when a project asks for preview features
    let edited = match tool {
        Tool::Maven => build_files::set_maven_language_level(&text, &level.option())?,
        Tool::Gradle => build_files::set_gradle_language_level(&text, &level.option()),
    };
    if edited != text {
        std::fs::write(&file, edited).with_context(|| format!("writing {file:?}"))?;
    }
    Ok(file)
}

/// JDT LS's compiler options for a plain folder's level (`java.settings.url`'s file), the
/// Eclipse preferences its compliance follows.
pub fn compiler_preferences(level: LanguageLevel) -> String {
    let option = level.option();
    let preview = match level.preview {
        true => "enabled",
        false => "disabled",
    };
    format!(
        "org.eclipse.jdt.core.compiler.compliance={option}\n\
         org.eclipse.jdt.core.compiler.source={option}\n\
         org.eclipse.jdt.core.compiler.codegen.targetPlatform={option}\n\
         org.eclipse.jdt.core.compiler.problem.enablePreviewFeatures={preview}\n\
         org.eclipse.jdt.core.compiler.problem.reportPreviewFeatures=ignore\n"
    )
}

#[cfg(test)]
mod tests {
    use super::{LanguageLevel, levels};

    #[test]
    fn lists_intellijs_levels_with_their_descriptions() {
        let [
            (supported, levels),
            (experimental, experiments),
            (unsupported, old),
        ] = levels();
        assert_eq!(
            (supported, experimental, unsupported),
            (
                "Supported Versions",
                "Experimental Versions",
                "Unsupported Versions"
            )
        );
        let texts: Vec<String> = levels.iter().map(|level| level.text()).collect();
        assert_eq!(texts[0], "27 - No new language features");
        assert_eq!(
            texts[1],
            "27 (Preview) - Primitive types in patterns, instanceof, and switch (5th preview)"
        );
        assert!(texts.contains(&"21 - Record patterns, pattern matching for switch".into()));
        assert!(texts.contains(&"8 - Lambdas, type annotations, etc.".into()));
        assert_eq!(
            texts.last().map(String::as_str),
            Some("1.3 - Plain old Java")
        );
        assert_eq!(experiments[0].text(), "X - Experimental features");
        assert_eq!(old[0].text(), "20 (Preview) - Unsupported");
        assert!(old[0].unsupported());
        assert_eq!(LanguageLevel::new(8).option(), "1.8");
        assert_eq!(LanguageLevel::parse("1.8"), Some(LanguageLevel::new(8)));
        assert_eq!(LanguageLevel::parse("21"), Some(LanguageLevel::new(21)));
    }
}
