//! The edits Project Structure and Settings › Kotlin Compiler make to a build file, as text edits
//! that keep the rest of it as written: the Java language level (IntelliJ's "Set language level"
//! fix, `MavenProjectModelModifier.changeLanguageLevel`, which writes the compiler plugin's
//! `source` and `target`; here the setting the build already uses is changed first), and the
//! Kotlin compiler's options and plugin version.

use std::ops::Range;

use anyhow::{Context as _, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

/// An XML element with content: its path of names from the root, its content's range and its
/// whole range (tags included), in bytes.
#[derive(Debug)]
struct Span {
    path: Vec<String>,
    content: Range<usize>,
    whole: Range<usize>,
}

impl Span {
    fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }

    fn holds(&self, other: &Span) -> bool {
        self.content.start <= other.whole.start && other.whole.end <= self.content.end
    }
}

fn position(reader: &Reader<&[u8]>) -> usize {
    usize::try_from(reader.buffer_position()).unwrap_or_default()
}

/// Every element of `xml` with content (no `<empty/>` ones), in document order.
fn spans(xml: &str) -> Result<Vec<Span>> {
    let mut reader = Reader::from_str(xml);
    // the open elements: their name, where their start tag starts and ends
    let mut open: Vec<(String, usize, usize)> = Vec::new();
    let mut spans = Vec::new();
    loop {
        let before = position(&reader);
        let event = reader.read_event().context("reading the XML")?;
        let after = position(&reader);
        match event {
            Event::Start(start) => {
                let name = String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
                open.push((name, before, after));
            }
            Event::End(_) => {
                let (name, start, content_start) =
                    open.pop().context("an end tag without its start")?;
                let mut path: Vec<String> = open.iter().map(|(name, ..)| name.clone()).collect();
                path.push(name);
                spans.push(Span {
                    path,
                    content: content_start..before,
                    whole: start..after,
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    spans.sort_by_key(|span| span.whole.start);
    Ok(spans)
}

/// A build file being edited: its text and the edits to it, applied from the end.
struct Edits<'a> {
    text: &'a str,
    edits: Vec<(Range<usize>, String)>,
}

impl<'a> Edits<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            edits: Vec::new(),
        }
    }

    fn replace(&mut self, range: Range<usize>, with: impl Into<String>) {
        self.edits.push((range, with.into()));
    }

    fn insert(&mut self, at: usize, text: impl Into<String>) {
        self.edits.push((at..at, text.into()));
    }

    fn apply(mut self) -> String {
        let mut text = self.text.to_owned();
        self.edits
            .sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, with) in self.edits {
            text.replace_range(range, &with);
        }
        text
    }
}

/// The whitespace a line starts with, for the line holding `at`.
fn indent_at(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |ix| ix + 1);
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The build file's indentation step: its first indented line's, else four spaces.
fn indent_unit(text: &str) -> String {
    text.lines()
        .map(|line| &line[..line.len() - line.trim_start_matches([' ', '\t']).len()])
        .find(|indent| !indent.is_empty())
        .unwrap_or("    ")
        .to_owned()
}

/// A Maven POM's elements, for reading and editing its settings.
struct Pom<'a> {
    xml: &'a str,
    spans: Vec<Span>,
}

impl<'a> Pom<'a> {
    fn parse(xml: &'a str) -> Result<Self> {
        Ok(Self {
            xml,
            spans: spans(xml)?,
        })
    }

    fn text(&self, span: &Span) -> &'a str {
        self.xml[span.content.clone()].trim()
    }

    fn at(&self, path: &[&str]) -> Option<&Span> {
        self.spans.iter().find(|span| span.path == path)
    }

    /// The child named `name` of `parent`.
    fn child(&self, parent: &Span, name: &str) -> Option<&Span> {
        self.spans.iter().find(|span| {
            span.path.len() == parent.path.len() + 1 && span.name() == name && parent.holds(span)
        })
    }

    fn property(&self, name: &str) -> Option<&Span> {
        self.at(&["project", "properties", name])
    }

    /// The build plugin of that artifact (`maven-compiler-plugin`), declared or managed.
    fn plugin(&self, artifact: &str) -> Option<&Span> {
        self.spans.iter().find(|span| {
            (span.path == ["project", "build", "plugins", "plugin"]
                || span.path == ["project", "build", "pluginManagement", "plugins", "plugin"])
                && self
                    .child(span, "artifactId")
                    .is_some_and(|id| self.text(id) == artifact)
        })
    }

    /// A setting of a plugin's `configuration`.
    fn plugin_setting(&self, artifact: &str, name: &str) -> Option<&Span> {
        let configuration = self.child(self.plugin(artifact)?, "configuration")?;
        self.child(configuration, name)
    }

    /// The element whose value a setting holds: the property it names (`${java.version}`), or
    /// itself.
    fn holder<'s>(&'s self, setting: &'s Span) -> &'s Span {
        let text = self.text(setting);
        text.strip_prefix("${")
            .and_then(|rest| rest.strip_suffix('}'))
            .and_then(|name| self.property(name))
            .unwrap_or(setting)
    }

    fn value(&self, setting: &Span) -> String {
        self.text(self.holder(setting)).to_owned()
    }
}

/// Inserts `text`'s lines before `parent`'s closing tag, at its indentation (theirs relative).
fn insert_in(edits: &mut Edits, xml: &str, parent: &Span, text: &str) {
    let indent = indent_at(xml, parent.content.end).to_owned();
    let mut inserted: String = text
        .lines()
        .enumerate()
        .map(|(ix, line)| match ix {
            0 => format!("{line}\n"),
            _ => format!("{indent}{line}\n"),
        })
        .collect();
    inserted.push_str(&indent);
    edits.insert(parent.content.end, inserted);
}

/// IntelliJ's compiler option for a feature release (`JpsJavaSdkType.complianceOption`): `1.8`
/// up to 8, then `9`, `21`….
pub fn java_option(feature: u32) -> String {
    crate::jdk::feature_text(feature)
}

const COMPILER_PLUGIN: &str = "maven-compiler-plugin";

/// The language level a POM sets: `maven.compiler.release`, the compiler plugin's `release`, then
/// the `source` of either, its `${property}` followed.
pub fn maven_language_level(xml: &str) -> Option<String> {
    let pom = Pom::parse(xml).ok()?;
    [
        pom.property("maven.compiler.release"),
        pom.plugin_setting(COMPILER_PLUGIN, "release"),
        pom.property("maven.compiler.source"),
        pom.plugin_setting(COMPILER_PLUGIN, "source"),
    ]
    .into_iter()
    .flatten()
    .map(|setting| pom.value(setting))
    .find(|value| !value.is_empty())
}

/// Sets the POM's language level to `value` (`21`, `1.8`) where it sets one (its `${property}`
/// when it names one), the target beside the source; a POM setting none gets IntelliJ's: the
/// compiler plugin's `source` and `target`.
pub fn set_maven_language_level(xml: &str, value: &str) -> Result<String> {
    let pom = Pom::parse(xml)?;
    let mut edits = Edits::new(xml);
    let release = pom
        .property("maven.compiler.release")
        .or_else(|| pom.plugin_setting(COMPILER_PLUGIN, "release"));
    if let Some(release) = release {
        edits.replace(pom.holder(release).content.clone(), value);
        return Ok(edits.apply());
    }
    let pairs = [
        (
            pom.property("maven.compiler.source"),
            pom.property("maven.compiler.target"),
            "maven.compiler.target",
        ),
        (
            pom.plugin_setting(COMPILER_PLUGIN, "source"),
            pom.plugin_setting(COMPILER_PLUGIN, "target"),
            "target",
        ),
    ];
    for (source, target, target_name) in pairs {
        let Some(source) = source else { continue };
        edits.replace(pom.holder(source).content.clone(), value);
        match target {
            Some(target) => edits.replace(pom.holder(target).content.clone(), value),
            None => edits.insert(
                source.whole.end,
                format!(
                    "\n{}<{target_name}>{value}</{target_name}>",
                    indent_at(xml, source.whole.start)
                ),
            ),
        }
        return Ok(edits.apply());
    }
    // IntelliJ's way: the compiler plugin's configuration, made as far as the POM lacks it
    let unit = indent_unit(xml);
    let settings = |depth: usize| {
        let indent = unit.repeat(depth);
        format!("{indent}<source>{value}</source>\n{indent}<target>{value}</target>\n")
    };
    let mut close_in = |parent: &Span, text: String| insert_in(&mut edits, xml, parent, &text);
    if let Some(plugin) = pom.plugin(COMPILER_PLUGIN) {
        match pom.child(plugin, "configuration") {
            Some(configuration) => close_in(configuration, settings(1)),
            None => close_in(
                plugin,
                format!(
                    "{unit}<configuration>\n{}{unit}</configuration>\n",
                    settings(2)
                ),
            ),
        }
    } else {
        let plugin = format!(
            "<plugin>\n{unit}<groupId>org.apache.maven.plugins</groupId>\n\
             {unit}<artifactId>{COMPILER_PLUGIN}</artifactId>\n\
             {unit}<configuration>\n{}{unit}</configuration>\n</plugin>\n",
            settings(2)
        );
        let nest = |tag: &str, inner: &str| {
            let inner: String = inner
                .lines()
                .map(|line| format!("{unit}{line}\n"))
                .collect();
            format!("<{tag}>\n{inner}</{tag}>\n")
        };
        let indented =
            |text: &str| -> String { text.lines().map(|line| format!("{unit}{line}\n")).collect() };
        if let Some(plugins) = pom.at(&["project", "build", "plugins"]) {
            close_in(plugins, indented(&plugin));
        } else if let Some(build) = pom.at(&["project", "build"]) {
            close_in(build, indented(&nest("plugins", &plugin)));
        } else {
            let project = pom.at(&["project"]).context("the POM has no project")?;
            close_in(project, indented(&nest("build", &nest("plugins", &plugin))));
        }
    }
    Ok(edits.apply())
}

/// A Gradle script's Java setting and the range of its value: the toolchain's
/// `JavaLanguageVersion.of(21)` (the number), or `sourceCompatibility`'s value as written
/// (`JavaVersion.VERSION_21`, `'21'`, `21`).
fn gradle_settings(script: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let toolchains = find_all(script, "JavaLanguageVersion.of(")
        .filter_map(|start| {
            let from = start + "JavaLanguageVersion.of(".len();
            let to = from + script[from..].find(')')?;
            Some(from..to)
        })
        .collect();
    let compatibilities = ["sourceCompatibility", "targetCompatibility"]
        .iter()
        .flat_map(|name| assignments(script, name))
        .collect();
    (toolchains, compatibilities)
}

/// The ranges of the values a Gradle script gives `name`: `name = value`, `name.set(value)`,
/// `name value`; a longer name it starts (`languageVersions`) left out.
fn assignments(script: &str, name: &str) -> Vec<Range<usize>> {
    find_all(script, name)
        .filter(|start| {
            let before = script[..*start].chars().next_back();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        })
        .map(|start| start + name.len())
        .filter(|after| {
            !script[*after..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .filter_map(|after| {
            let rest = &script[after..];
            let skipped = rest.len()
                - rest
                    .trim_start_matches([' ', '\t'])
                    .trim_start_matches(['=', '('])
                    .trim_start_matches(".set(")
                    .trim_start_matches([' ', '\t'])
                    .len();
            let from = after + skipped;
            let mut depth = 0i32;
            let len = script[from..]
                .char_indices()
                .find(|(_, c)| match c {
                    '(' => {
                        depth += 1;
                        false
                    }
                    ')' if depth == 0 => true,
                    ')' => {
                        depth -= 1;
                        false
                    }
                    '\n' | ';' | ' ' | '\t' | '}' => depth == 0,
                    _ => false,
                })
                .map_or(script.len() - from, |(ix, _)| ix);
            (len > 0).then_some(from..from + len)
        })
        .collect()
}

fn find_all<'a>(text: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(needle)
        .map(|(ix, _)| ix)
        // not in a `//` comment
        .filter(move |ix| {
            let line_start = text[..*ix].rfind('\n').map_or(0, |start| start + 1);
            !text[line_start..*ix].contains("//")
        })
}

/// The release a Gradle value names: `JavaVersion.VERSION_1_8` → `1.8`, `'21'` → `21`.
fn gradle_value(written: &str) -> String {
    let written = written.trim();
    let inner = written
        .strip_prefix("JavaVersion.toVersion(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(written);
    let inner = inner.trim_matches(['\'', '"']);
    match inner.strip_prefix("JavaVersion.VERSION_") {
        Some(constant) => constant.replace('_', "."),
        None => inner.to_owned(),
    }
}

/// `value` written as `written` is: a `JavaVersion` constant, quoted, or bare.
fn gradle_written(written: &str, value: &str) -> String {
    let written = written.trim();
    if written.starts_with("JavaVersion.VERSION_") {
        return format!("JavaVersion.VERSION_{}", value.replace('.', "_"));
    }
    if let Some(inner) = written
        .strip_prefix("JavaVersion.toVersion(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return format!("JavaVersion.toVersion({})", gradle_written(inner, value));
    }
    match written.chars().next() {
        Some(quote @ ('\'' | '"')) => format!("{quote}{value}{quote}"),
        _ => value.to_owned(),
    }
}

/// The language level a Gradle script sets: its toolchain's, else its `sourceCompatibility`.
pub fn gradle_language_level(script: &str) -> Option<String> {
    let (toolchains, compatibilities) = gradle_settings(script);
    toolchains
        .first()
        .or(compatibilities.first())
        .map(|range| gradle_value(&script[range.clone()]))
}

/// Sets a Gradle script's language level to `value` (`21`, `1.8`) as it sets it: its toolchain's
/// language version, or its `sourceCompatibility` and `targetCompatibility`; a script setting
/// neither gets `java { sourceCompatibility … }`, which needs no toolchain installed.
pub fn set_gradle_language_level(script: &str, value: &str) -> String {
    let (toolchains, compatibilities) = gradle_settings(script);
    let mut edits = Edits::new(script);
    if !toolchains.is_empty() {
        let feature = value.trim_start_matches("1.");
        for range in toolchains {
            let written = gradle_written(&script[range.clone()], feature);
            edits.replace(range, written);
        }
    } else if !compatibilities.is_empty() {
        for range in compatibilities {
            let written = gradle_written(&script[range.clone()], value);
            edits.replace(range, written);
        }
    } else {
        let constant = format!("JavaVersion.VERSION_{}", value.replace('.', "_"));
        let separator = if script.ends_with('\n') { "\n" } else { "\n\n" };
        edits.insert(
            script.len(),
            format!(
                "{separator}java {{\n    sourceCompatibility = {constant}\n    targetCompatibility = {constant}\n}}\n"
            ),
        );
    }
    edits.apply()
}

/// The Kotlin compiler's options a build sets, and the Kotlin version it builds with (its Kotlin
/// plugin's): `2.1` for the versions, `17` or `1.8` for the JVM target.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KotlinOptions {
    pub language_version: Option<String>,
    pub api_version: Option<String>,
    pub jvm_target: Option<String>,
    pub version: Option<String>,
}

const KOTLIN_PLUGIN: &str = "kotlin-maven-plugin";

/// The POM's names of an option: its property and the Kotlin plugin's setting.
const MAVEN_KOTLIN_OPTIONS: [(&str, &str); 3] = [
    ("kotlin.compiler.languageVersion", "languageVersion"),
    ("kotlin.compiler.apiVersion", "apiVersion"),
    ("kotlin.compiler.jvmTarget", "jvmTarget"),
];

impl KotlinOptions {
    fn options(&self) -> [&Option<String>; 3] {
        [&self.language_version, &self.api_version, &self.jvm_target]
    }

    fn options_mut(&mut self) -> [&mut Option<String>; 3] {
        [
            &mut self.language_version,
            &mut self.api_version,
            &mut self.jvm_target,
        ]
    }
}

/// The Kotlin options a POM sets: its `kotlin.compiler.*` properties, else its Kotlin plugin's
/// configuration; the version its `kotlin.version` or the plugin's.
pub fn maven_kotlin(xml: &str) -> KotlinOptions {
    let Ok(pom) = Pom::parse(xml) else {
        return KotlinOptions::default();
    };
    let mut options = KotlinOptions::default();
    for ((property, setting), value) in MAVEN_KOTLIN_OPTIONS.iter().zip(options.options_mut()) {
        *value = pom
            .property(property)
            .or_else(|| pom.plugin_setting(KOTLIN_PLUGIN, setting))
            .map(|span| pom.value(span));
    }
    options.version = pom
        .property("kotlin.version")
        .or_else(|| {
            pom.plugin(KOTLIN_PLUGIN)
                .and_then(|plugin| pom.child(plugin, "version"))
        })
        .map(|span| pom.value(span));
    options
}

/// Writes the options of `options` that are set to the POM where it sets them, else as
/// `kotlin.compiler.*` properties; the version where the POM names it.
pub fn set_maven_kotlin(xml: &str, options: &KotlinOptions) -> Result<String> {
    let pom = Pom::parse(xml)?;
    let mut edits = Edits::new(xml);
    let unit = indent_unit(xml);
    let mut new_properties = String::new();
    for ((property, setting), value) in MAVEN_KOTLIN_OPTIONS.iter().zip(options.options()) {
        let Some(value) = value else { continue };
        match pom
            .property(property)
            .or_else(|| pom.plugin_setting(KOTLIN_PLUGIN, setting))
        {
            Some(span) => edits.replace(pom.holder(span).content.clone(), value.as_str()),
            None => new_properties.push_str(&format!("{unit}<{property}>{value}</{property}>\n")),
        }
    }
    if let Some(version) = &options.version {
        let span = pom.property("kotlin.version").or_else(|| {
            pom.plugin(KOTLIN_PLUGIN)
                .and_then(|plugin| pom.child(plugin, "version"))
        });
        if let Some(span) = span {
            edits.replace(pom.holder(span).content.clone(), version.as_str());
        }
    }
    if !new_properties.is_empty() {
        match pom.at(&["project", "properties"]) {
            Some(properties) => insert_in(&mut edits, xml, properties, &new_properties),
            None => {
                let project = pom.at(&["project"]).context("the POM has no project")?;
                let block: String = format!("<properties>\n{new_properties}</properties>\n")
                    .lines()
                    .map(|line| format!("{unit}{line}\n"))
                    .collect();
                insert_in(&mut edits, xml, project, &block);
            }
        }
    }
    Ok(edits.apply())
}

/// The Gradle names of the options, and the type of their constants.
const GRADLE_KOTLIN_OPTIONS: [(&str, &str, &str); 3] = [
    ("languageVersion", "KotlinVersion", "KOTLIN_"),
    ("apiVersion", "KotlinVersion", "KOTLIN_"),
    ("jvmTarget", "JvmTarget", "JVM_"),
];

/// A Kotlin option's value as written (`KotlinVersion.KOTLIN_2_1`, `JvmTarget.JVM_1_8`,
/// `"17"`) as a version.
fn kotlin_value(written: &str) -> Option<String> {
    let written = written.trim().trim_matches(['"', '\'']);
    if written.contains("JavaLanguageVersion") {
        return None;
    }
    let constant = written.rsplit('.').next().unwrap_or(written);
    let version = ["KOTLIN_", "JVM_"]
        .iter()
        .find_map(|prefix| constant.strip_prefix(prefix))
        .map(|rest| rest.replace('_', "."))
        .unwrap_or_else(|| written.to_owned());
    version
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then_some(version)
}

/// `value` written as `written` is: a constant, or quoted.
fn kotlin_written(written: &str, value: &str, prefix: &str) -> String {
    let written = written.trim();
    match written.chars().next() {
        Some(quote @ ('"' | '\'')) => format!("{quote}{value}{quote}"),
        _ => match written.rfind(prefix) {
            Some(at) => format!("{}{prefix}{}", &written[..at], value.replace('.', "_")),
            None => value.to_owned(),
        },
    }
}

/// The ranges of a Gradle script's Kotlin plugin version: `kotlin("jvm") version "2.1.0"`,
/// `id("org.jetbrains.kotlin.jvm") version "2.1.0"`, `id 'org.jetbrains.kotlin.jvm' version '2.1.0'`.
fn kotlin_plugin_versions(script: &str) -> Vec<Range<usize>> {
    script
        .lines()
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .filter(|(_, line)| {
            let line = line.trim_start();
            !line.starts_with("//")
                && (line.starts_with("kotlin(") || line.contains("org.jetbrains.kotlin."))
        })
        .filter_map(|(start, line)| {
            let at = line.find(" version ")? + " version ".len();
            let rest = &line[at..];
            let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let len = rest[1..].find(quote)?;
            Some(start + at + 1..start + at + 1 + len)
        })
        .collect()
}

/// The Kotlin options a Gradle script sets (`compilerOptions` or the older `kotlinOptions`)
/// and its Kotlin plugin's version.
pub fn gradle_kotlin(script: &str) -> KotlinOptions {
    let mut options = KotlinOptions::default();
    for ((name, ..), value) in GRADLE_KOTLIN_OPTIONS.iter().zip(options.options_mut()) {
        *value = assignments(script, name)
            .into_iter()
            .find_map(|range| kotlin_value(&script[range]));
    }
    options.version = kotlin_plugin_versions(script)
        .first()
        .map(|range| script[range.clone()].to_owned());
    options
}

/// The range of a block's braces (`kotlin {`… `}`), its first opening at `name {`.
fn block(script: &str, name: &str) -> Option<Range<usize>> {
    let open = find_all(script, &format!("{name} {{")).find(|start| {
        !script[..*start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
    })? + name.len()
        + 1;
    let mut depth = 0;
    for (ix, c) in script[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open..open + ix);
                }
            }
            _ => {}
        }
    }
    None
}

/// Writes the options of `options` that are set to the script where it sets them, else into
/// `kotlin { compilerOptions { … } }` (made as far as it lacks it); the version in its Kotlin
/// plugin's declaration.
pub fn set_gradle_kotlin(script: &str, options: &KotlinOptions) -> String {
    let mut edits = Edits::new(script);
    let unit = indent_unit(script);
    let mut missing = Vec::new();
    for ((name, kind, prefix), value) in GRADLE_KOTLIN_OPTIONS.iter().zip(options.options()) {
        let Some(value) = value else { continue };
        let found: Vec<Range<usize>> = assignments(script, name)
            .into_iter()
            .filter(|range| kotlin_value(&script[range.clone()]).is_some())
            .collect();
        if found.is_empty() {
            missing.push(format!(
                "{name}.set(org.jetbrains.kotlin.gradle.dsl.{kind}.{prefix}{})",
                value.replace('.', "_")
            ));
        }
        for range in found {
            let written = kotlin_written(&script[range.clone()], value, prefix);
            edits.replace(range, written);
        }
    }
    if let Some(version) = &options.version {
        for range in kotlin_plugin_versions(script) {
            edits.replace(range, version.as_str());
        }
    }
    if !missing.is_empty() {
        // `text` before the block's closing brace, one step in from its line
        let mut close_in = |braces: Range<usize>, lines: &[String]| {
            let indent = indent_at(script, braces.start).to_owned();
            let inner: String = lines
                .iter()
                .map(|line| format!("{indent}{unit}{line}\n"))
                .collect();
            let before_close = &script[..braces.end];
            let at = before_close.trim_end_matches([' ', '\t']).len();
            let newline = if script[..at].ends_with('\n') {
                ""
            } else {
                "\n"
            };
            edits.insert(at, format!("{newline}{inner}"));
        };
        let nested = |depth: usize| -> Vec<String> {
            let mut lines = vec![format!("{}compilerOptions {{", unit.repeat(depth))];
            lines.extend(
                missing
                    .iter()
                    .map(|line| format!("{}{line}", unit.repeat(depth + 1))),
            );
            lines.push(format!("{}}}", unit.repeat(depth)));
            lines
        };
        if let Some(braces) = block(script, "compilerOptions") {
            close_in(braces, &missing);
        } else if let Some(braces) = block(script, "kotlin") {
            close_in(braces, &nested(0));
        } else {
            let separator = if script.ends_with('\n') { "\n" } else { "\n\n" };
            let body: String = nested(1).iter().map(|line| format!("{line}\n")).collect();
            edits.insert(script.len(), format!("{separator}kotlin {{\n{body}}}\n"));
        }
    }
    edits.apply()
}

#[cfg(test)]
mod tests {
    use super::{
        gradle_language_level, maven_language_level, set_gradle_language_level,
        set_maven_language_level,
    };

    const RELEASE: &str = "<project>\n    <properties>\n        <maven.compiler.release>17</maven.compiler.release>\n    </properties>\n</project>\n";

    #[test]
    fn a_pom_keeps_the_setting_it_uses() {
        assert_eq!(maven_language_level(RELEASE).as_deref(), Some("17"));
        assert_eq!(
            set_maven_language_level(RELEASE, "21").expect("edited"),
            RELEASE.replace(">17<", ">21<")
        );
        let source = "<project>\n  <properties>\n    <maven.compiler.source>1.8</maven.compiler.source>\n  </properties>\n</project>\n";
        assert_eq!(maven_language_level(source).as_deref(), Some("1.8"));
        assert_eq!(
            set_maven_language_level(source, "11").expect("edited"),
            "<project>\n  <properties>\n    <maven.compiler.source>11</maven.compiler.source>\n    <maven.compiler.target>11</maven.compiler.target>\n  </properties>\n</project>\n"
        );
    }

    #[test]
    fn a_pom_property_named_by_the_setting_is_the_one_changed() {
        let pom = "<project>\n  <properties>\n    <java.version>17</java.version>\n    <maven.compiler.release>${java.version}</maven.compiler.release>\n  </properties>\n</project>\n";
        assert_eq!(maven_language_level(pom).as_deref(), Some("17"));
        assert_eq!(
            set_maven_language_level(pom, "21").expect("edited"),
            pom.replace("<java.version>17", "<java.version>21")
        );
    }

    #[test]
    fn the_compiler_plugins_configuration_is_changed_or_made() {
        let plugin = "<project>\n  <build>\n    <plugins>\n      <plugin>\n        <artifactId>maven-compiler-plugin</artifactId>\n        <configuration>\n          <source>11</source>\n          <target>11</target>\n        </configuration>\n      </plugin>\n    </plugins>\n  </build>\n</project>\n";
        assert_eq!(maven_language_level(plugin).as_deref(), Some("11"));
        assert_eq!(
            set_maven_language_level(plugin, "17").expect("edited"),
            plugin.replace(">11<", ">17<")
        );
        let bare = "<project>\n  <modelVersion>4.0.0</modelVersion>\n</project>\n";
        assert_eq!(maven_language_level(bare), None);
        let made = set_maven_language_level(bare, "21").expect("edited");
        assert_eq!(
            made,
            "<project>\n  <modelVersion>4.0.0</modelVersion>\n  <build>\n    <plugins>\n      <plugin>\n        <groupId>org.apache.maven.plugins</groupId>\n        <artifactId>maven-compiler-plugin</artifactId>\n        <configuration>\n          <source>21</source>\n          <target>21</target>\n        </configuration>\n      </plugin>\n    </plugins>\n  </build>\n</project>\n"
        );
        assert_eq!(maven_language_level(&made).as_deref(), Some("21"));
    }

    #[test]
    fn a_gradle_script_keeps_the_setting_it_uses() {
        let toolchain = "java {\n    toolchain {\n        languageVersion = JavaLanguageVersion.of(17)\n    }\n}\n";
        assert_eq!(gradle_language_level(toolchain).as_deref(), Some("17"));
        assert_eq!(
            set_gradle_language_level(toolchain, "21"),
            toolchain.replace("of(17)", "of(21)")
        );
        let kotlin = "java {\n    sourceCompatibility = JavaVersion.VERSION_1_8\n    targetCompatibility = JavaVersion.VERSION_1_8\n}\n";
        assert_eq!(gradle_language_level(kotlin).as_deref(), Some("1.8"));
        assert_eq!(
            set_gradle_language_level(kotlin, "17"),
            kotlin.replace("VERSION_1_8", "VERSION_17")
        );
        let quoted = "sourceCompatibility = '11'\n";
        assert_eq!(gradle_language_level(quoted).as_deref(), Some("11"));
        assert_eq!(
            set_gradle_language_level(quoted, "21"),
            "sourceCompatibility = '21'\n"
        );
    }

    #[test]
    fn a_gradle_script_setting_none_gets_a_java_block() {
        let script = "plugins {\n    id 'application'\n}\n";
        assert_eq!(gradle_language_level(script), None);
        let edited = set_gradle_language_level(script, "21");
        assert_eq!(
            edited,
            "plugins {\n    id 'application'\n}\n\njava {\n    sourceCompatibility = JavaVersion.VERSION_21\n    targetCompatibility = JavaVersion.VERSION_21\n}\n"
        );
        assert_eq!(gradle_language_level(&edited).as_deref(), Some("21"));
    }

    #[test]
    fn a_poms_kotlin_options_are_kept_where_it_sets_them() {
        let pom = "<project>\n  <properties>\n    <kotlin.version>2.1.0</kotlin.version>\n    <kotlin.compiler.jvmTarget>17</kotlin.compiler.jvmTarget>\n  </properties>\n</project>\n";
        let read = super::maven_kotlin(pom);
        assert_eq!(read.version.as_deref(), Some("2.1.0"));
        assert_eq!(read.jvm_target.as_deref(), Some("17"));
        let options = super::KotlinOptions {
            language_version: Some("2.1".into()),
            jvm_target: Some("21".into()),
            version: Some("2.2.0".into()),
            ..Default::default()
        };
        assert_eq!(
            super::set_maven_kotlin(pom, &options).expect("edited"),
            "<project>\n  <properties>\n    <kotlin.version>2.2.0</kotlin.version>\n    <kotlin.compiler.jvmTarget>21</kotlin.compiler.jvmTarget>\n    <kotlin.compiler.languageVersion>2.1</kotlin.compiler.languageVersion>\n  </properties>\n</project>\n"
        );
    }

    #[test]
    fn a_gradle_scripts_kotlin_options_are_kept_where_it_sets_them() {
        let script = "plugins {\n    kotlin(\"jvm\") version \"2.1.0\"\n}\n\nkotlin {\n    compilerOptions {\n        jvmTarget.set(JvmTarget.JVM_17)\n    }\n}\n";
        let read = super::gradle_kotlin(script);
        assert_eq!(read.version.as_deref(), Some("2.1.0"));
        assert_eq!(read.jvm_target.as_deref(), Some("17"));
        assert_eq!(read.language_version, None);
        let options = super::KotlinOptions {
            language_version: Some("2.0".into()),
            jvm_target: Some("1.8".into()),
            version: Some("2.2.0".into()),
            ..Default::default()
        };
        assert_eq!(
            super::set_gradle_kotlin(script, &options),
            "plugins {\n    kotlin(\"jvm\") version \"2.2.0\"\n}\n\nkotlin {\n    compilerOptions {\n        jvmTarget.set(JvmTarget.JVM_1_8)\n        languageVersion.set(org.jetbrains.kotlin.gradle.dsl.KotlinVersion.KOTLIN_2_0)\n    }\n}\n"
        );
        let bare = "plugins {\n    id 'org.jetbrains.kotlin.jvm' version '2.1.0'\n}\n";
        let made = super::set_gradle_kotlin(bare, &options);
        assert_eq!(
            made,
            "plugins {\n    id 'org.jetbrains.kotlin.jvm' version '2.2.0'\n}\n\nkotlin {\n    compilerOptions {\n        languageVersion.set(org.jetbrains.kotlin.gradle.dsl.KotlinVersion.KOTLIN_2_0)\n        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_1_8)\n    }\n}\n"
        );
        assert_eq!(
            super::gradle_kotlin(&made).language_version.as_deref(),
            Some("2.0")
        );
        // the Java toolchain's languageVersion isn't Kotlin's
        let java = "java {\n    toolchain {\n        languageVersion = JavaLanguageVersion.of(17)\n    }\n}\n";
        assert_eq!(super::gradle_kotlin(java).language_version, None);
    }
}
