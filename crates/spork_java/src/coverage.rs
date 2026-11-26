//! IntelliJ's own JVM coverage engine (`IDEACoverageRunner`) for Spork IDE's coverage
//! (`spork_coverage`): IntelliJ's coverage agent (`intellij-coverage-agent`, Apache-2.0, from
//! Maven Central, its SHA-256 checked, never bundled) on the JVM of a run with coverage, as
//! `-javaagent` and its arguments file as IntelliJ writes them; the run's data (the agent's `.ic`
//! file, its source map and per-test traces) read by Spork IDE's helper (`coverage-helper/`) on
//! the project's JDK; the HTML report written by the agent's reporter
//! (`intellij-coverage-reporter`, with TeamCity's `coverage-report` and FreeMarker), downloaded
//! the first time a report is generated.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use collections::HashMap;
use gpui::{App, AppContext as _, Task};
use http_client::HttpClient;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use spork_coverage::Suite;
use spork_coverage::data::{Class, Data, Jump, Line, Switch};
use spork_run::configurations::{self, Configuration, Field};
use workspace::Workspace;

use crate::java_project::{JavaProject, Tool};

/// A jar from Maven Central, its SHA-256 pinned here (checked against Maven Central's SHA-1).
struct Artifact {
    path: &'static str,
    sha256: &'static str,
}

impl Artifact {
    fn file_name(&self) -> &'static str {
        self.path.rsplit('/').next().unwrap_or(self.path)
    }
}

/// IntelliJ's coverage agent, the version IntelliJ IDEA 2026.3 bundles. Its manifest's
/// `Boot-Class-Path` names the jar itself: it keeps its file name.
const AGENT: Artifact = Artifact {
    path: "org/jetbrains/intellij/deps/intellij-coverage-agent/1.0.777/intellij-coverage-agent-1.0.777.jar",
    sha256: "b8c4455cda170a2f946dcb755de3e2a76376e52cfb86c898c8de2582aa2bef11",
};

/// The HTML report's jars: the agent's reporter and its dependencies.
const REPORTER: [Artifact; 3] = [
    Artifact {
        path: "org/jetbrains/intellij/deps/intellij-coverage-reporter/1.0.777/intellij-coverage-reporter-1.0.777.jar",
        sha256: "dbac92f2dbe507d066a70d703ba15937b344900018e15644edeefe8ca90735a8",
    },
    Artifact {
        path: "org/jetbrains/intellij/deps/coverage-report/1.0.27/coverage-report-1.0.27.jar",
        sha256: "8a29a130ab4fa9a23fd390ff535afb52fb55e5f61448423c16472be88609c03a",
    },
    Artifact {
        path: "org/freemarker/freemarker/2.3.32/freemarker-2.3.32.jar",
        sha256: "04d65ec1bde6cea20e3495d5e78ef96ab774d9936434861d3254bd88e7e94f92",
    },
];

const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

/// Where the jars are kept: the data folder's, kept across runs and projects.
fn jars() -> PathBuf {
    paths::data_dir().join("java").join("coverage-agent")
}

/// The jar, downloaded the first time and checked.
async fn fetch(http: &Arc<dyn HttpClient>, artifact: &Artifact) -> Result<PathBuf> {
    let jar = jars().join(artifact.file_name());
    if jar.is_file() {
        return Ok(jar);
    }
    let url = format!("{MAVEN_CENTRAL}/{}", artifact.path);
    let bytes = crate::testing::download(http, &url).await?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    anyhow::ensure!(
        actual == artifact.sha256,
        "the checksum of {url} doesn't match"
    );
    std::fs::create_dir_all(jars()).with_context(|| format!("creating {:?}", jars()))?;
    // written aside then moved in: a half-written jar would pass for a downloaded one
    let part = jar.with_extension("jar.part");
    std::fs::write(&part, bytes).with_context(|| format!("writing {part:?}"))?;
    std::fs::rename(&part, &jar).with_context(|| format!("writing {jar:?}"))?;
    Ok(jar)
}

/// The helper's classes, built from `coverage-helper/src` by `coverage-helper/build.sh`.
const HELPER_CLASSES: [(&str, &[u8]); 2] = [
    (
        "Data.class",
        include_bytes!("../coverage-helper/classes/spork/coverage/Data.class"),
    ),
    (
        "HtmlReport.class",
        include_bytes!("../coverage-helper/classes/spork/coverage/HtmlReport.class"),
    ),
];

/// The folder of the helper's classes in the data folder, written the first time, a folder per
/// build of them.
fn helper_classes() -> Result<PathBuf> {
    let mut digest = Sha256::new();
    for (name, bytes) in HELPER_CLASSES {
        digest.update(name.as_bytes());
        digest.update(bytes);
    }
    let hash: String = digest.finalize()[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let folder = paths::data_dir()
        .join("java")
        .join("coverage-helper")
        .join(hash);
    let classes = folder.join("spork").join("coverage");
    if !classes.join("Data.class").is_file() {
        std::fs::create_dir_all(&classes).with_context(|| format!("creating {classes:?}"))?;
        for (name, bytes) in HELPER_CLASSES {
            std::fs::write(classes.join(name), bytes)
                .with_context(|| format!("writing the coverage helper's {name}"))?;
        }
    }
    Ok(folder)
}

/// A class pattern as the agent takes it (IntelliJ's `convertToPatterns`): `com.foo.*` is
/// `com\.foo\..*`, `com.foo.Bar` also takes its inner classes, `com\.foo\.Bar(\$.*)*`.
fn class_regex(pattern: &str) -> String {
    let regex = pattern
        .replace('$', "\\$")
        .replace('.', "\\.")
        .replace('*', ".*");
    match regex.ends_with(".*") {
        true => regex,
        false => format!("{regex}(\\$.*)*"),
    }
}

/// An annotation pattern as the agent takes it: `*Generated*` is `.*Generated.*`.
fn annotation_regex(pattern: &str) -> String {
    pattern.replace('.', "\\.").replace('*', ".*")
}

/// The package a configuration was made from, IntelliJ's default pattern (`com.example.*`):
/// its main class's, its test class's or its package.
fn default_pattern(configuration: &Configuration) -> Option<String> {
    let package_of = |class: &str| {
        class
            .trim()
            .rsplit_once('.')
            .map(|(package, _)| format!("{package}.*"))
    };
    match configuration.adapter.as_deref() {
        Some(configurations::JUNIT | configurations::TESTNG) => {
            match configurations::test_kind(configuration) {
                "package" => {
                    let package = configuration.text(Field::TestPackage);
                    let package = package.trim();
                    (!package.is_empty()).then(|| format!("{package}.*"))
                }
                "pattern" => None,
                _ => package_of(&configuration.text(Field::Command)),
            }
        }
        _ => package_of(&configuration.text(Field::Command)),
    }
}

/// A file or folder name made of the configuration's name.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// What a run with coverage adds: the JVM's options, and the suite it gathers for the IDE.
pub struct CoverageRun {
    pub vm_options: Vec<String>,
    pub suite: Suite,
}

impl CoverageRun {
    /// The suite, as the task's environment hands it to the IDE when the run ends.
    pub fn env(&self) -> (String, String) {
        (
            spork_coverage::SUITE_ENV.to_owned(),
            serde_json::to_string(&self.suite).unwrap_or_default(),
        )
    }
}

/// The agent's options for a run of `configuration` with coverage, once the agent is
/// downloaded; `None` for a run without coverage.
pub fn prepare(
    configuration: &Configuration,
    root: &Path,
    cx: &App,
) -> Option<Task<Result<CoverageRun>>> {
    if !configurations::with_coverage(configuration) {
        return None;
    }
    let settings = spork_settings::coverage::settings(cx);
    let patterns = configurations::coverage_patterns(configuration);
    let name = configuration.name().to_string();
    let is_test = configurations::is_test(configuration.adapter.as_deref());
    let gradle = JavaProject::containing(root, cx)
        .is_some_and(|java| java.read(cx).tool == Some(Tool::Gradle));
    let mut include: Vec<String> = patterns
        .iter()
        .filter(|pattern| pattern.enabled && pattern.include)
        .map(|pattern| class_regex(&pattern.pattern))
        .collect();
    if include.is_empty()
        && let Some(pattern) = default_pattern(configuration)
    {
        include.push(class_regex(&pattern));
    }
    let exclude: Vec<String> = patterns
        .iter()
        .filter(|pattern| pattern.enabled && !pattern.include)
        .map(|pattern| class_regex(&pattern.pattern))
        .collect();
    let annotations: Vec<String> = settings
        .exclude_annotations
        .iter()
        .filter(|(_, on)| *on)
        .map(|(pattern, _)| annotation_regex(pattern))
        .collect();
    let branch_coverage = settings.branch_coverage;
    // IntelliJ tracks each test on its own test runner only, never on Gradle's
    let per_test = is_test && !gradle && settings.track_per_test;
    let folder = crate::jdtls::project_data(root, "coverage");
    let data_file = folder.join(format!("{}.ic", file_name(&name)));
    let root = root.to_owned();
    let http = cx.http_client();
    Some(cx.background_spawn(async move {
        let agent = fetch(&http, &AGENT).await?;
        std::fs::create_dir_all(&folder).with_context(|| format!("creating {folder:?}"))?;
        // IntelliJ empties the data before each run; the traces of the last run go too
        for stale in [data_file.clone(), data_file.with_extension("sm")] {
            if stale.exists() {
                std::fs::remove_file(&stale).with_context(|| format!("removing {stale:?}"))?;
            }
        }
        let traces = data_file.with_extension("");
        if traces.is_dir() {
            std::fs::remove_dir_all(&traces).with_context(|| format!("removing {traces:?}"))?;
        }
        // IntelliJ's arguments file (`IDEACoverageRunner.writeOptionsToFile`): the data file,
        // tracking per test, unloaded classes, merging (several forked JVMs write one file),
        // line coverage only, the source map, then the patterns
        let mut lines = vec![
            data_file.to_string_lossy().into_owned(),
            per_test.to_string(),
            (!include.is_empty()).to_string(),
            "true".to_owned(),
            (!branch_coverage).to_string(),
            "true".to_owned(),
            data_file
                .with_extension("sm")
                .to_string_lossy()
                .into_owned(),
        ];
        lines.extend(include);
        if !exclude.is_empty() {
            lines.push("-exclude".into());
            lines.extend(exclude);
        }
        if !annotations.is_empty() {
            lines.push("-excludeAnnotations".into());
            lines.extend(annotations);
        }
        let args = data_file.with_extension("args");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&args, text).with_context(|| format!("writing {args:?}"))?;
        Ok(CoverageRun {
            vm_options: vec![
                format!(
                    "-javaagent:{}={}",
                    agent.to_string_lossy(),
                    args.to_string_lossy()
                ),
                // exact hits, as IntelliJ's registry has it (`idea.coverage.calculate.exact.hits`)
                "-Didea.coverage.calculate.hits=true".into(),
            ],
            suite: Suite {
                name,
                file: data_file,
                timestamp: 0,
                root,
                branch_coverage,
                track_per_test: per_test,
            },
        })
    }))
}

/// The project's source files by package folder and name, to find each class's file: a
/// Gradle subproject's too, its build tools' output and hidden folders left out.
fn source_files(root: &Path) -> HashMap<(PathBuf, String), Vec<PathBuf>> {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || ["build", "target", "out", "node_modules"].contains(&name.as_str())
            {
                continue;
            }
            if path.is_dir() {
                walk(&path, found);
            } else if name.ends_with(".java") || name.ends_with(".kt") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, &mut found);
    let mut files: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::default();
    for file in found {
        let (Some(folder), Some(name)) = (file.parent(), file.file_name()) else {
            continue;
        };
        files
            .entry((folder.to_owned(), name.to_string_lossy().into_owned()))
            .or_default()
            .push(file.clone());
    }
    files
}

/// The source file of `class`, named `source` by its class file: the file of that name in
/// the package's folder of a source root.
fn find_source(
    files: &HashMap<(PathBuf, String), Vec<PathBuf>>,
    class: &str,
    source: &str,
) -> Option<PathBuf> {
    let package: PathBuf = class
        .rsplit_once('.')
        .map(|(package, _)| package.split('.').collect())
        .unwrap_or_default();
    files
        .iter()
        .find(|((folder, name), _)| name == source && folder.ends_with(&package))
        .and_then(|(_, paths)| paths.first().cloned())
}

/// Whether `file` is in a test source root (IntelliJ leaves them out unless "Collect coverage
/// in test folders").
// shortcut: a test root by Maven's and Gradle's layout (`src/test/…`); ask the build's model
// once a project declares other test roots
fn in_test_folder(file: &Path) -> bool {
    let parts: Vec<_> = file.components().collect();
    parts
        .windows(2)
        .any(|pair| pair[0].as_os_str() == "src" && pair[1].as_os_str() == "test")
}

/// The helper's JSON as coverage data, each class's file found among `files`.
fn parse(
    value: &Value,
    files: &HashMap<(PathBuf, String), Vec<PathBuf>>,
    test_folders: bool,
) -> Data {
    let numbers = |value: Option<&Value>| -> Vec<u32> {
        value
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|n| n as u32)
                    .collect()
            })
            .unwrap_or_default()
    };
    let classes = value
        .get("classes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|class| {
            let name = class.get("name")?.as_str()?.to_owned();
            // the class file's source (the agent's source map), else its top-level class's name
            let file = match class.get("source").and_then(Value::as_str) {
                Some(source) => find_source(files, &name, source),
                None => {
                    let simple = name.rsplit('.').next().unwrap_or(&name);
                    let top = simple.split('$').next().unwrap_or(simple);
                    let kotlin = top.strip_suffix("Kt").unwrap_or(top);
                    [
                        format!("{top}.java"),
                        format!("{top}.kt"),
                        format!("{kotlin}.kt"),
                    ]
                    .iter()
                    .find_map(|source| find_source(files, &name, source))
                }
            };
            if !test_folders && file.as_deref().is_some_and(in_test_folder) {
                return None;
            }
            let lines = class
                .get("lines")?
                .as_array()?
                .iter()
                .filter_map(|line| {
                    Some(Line {
                        line: line.get("line")?.as_u64()? as u32,
                        hits: line.get("hits")?.as_u64()? as u32,
                        method: line
                            .get("method")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        jumps: line
                            .get("jumps")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|jump| {
                                let jump = numbers(Some(jump));
                                Some(Jump {
                                    true_hits: *jump.first()?,
                                    false_hits: *jump.get(1)?,
                                })
                            })
                            .collect(),
                        switches: line
                            .get("switches")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|switch| Switch {
                                keys: switch
                                    .get("keys")
                                    .and_then(Value::as_array)
                                    .map(|keys| {
                                        keys.iter()
                                            .filter_map(Value::as_i64)
                                            .map(|key| key as i32)
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                hits: numbers(switch.get("hits")),
                                default_hits: switch
                                    .get("default")
                                    .and_then(Value::as_u64)
                                    .unwrap_or_default()
                                    as u32,
                            })
                            .collect(),
                    })
                })
                .collect();
            Some(Class { name, file, lines })
        })
        .collect();
    let tests = value
        .get("tests")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(test, classes)| {
            let classes = classes
                .as_object()
                .into_iter()
                .flatten()
                .map(|(class, lines)| (class.clone(), numbers(Some(lines))))
                .collect();
            (test.clone(), classes)
        })
        .collect();
    Data { classes, tests }
}

/// The class folders and source roots of the project's modules, for the HTML report.
fn module_folders(root: &Path, test_folders: bool) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut modules = vec![root.to_owned()];
    if let Ok(entries) = std::fs::read_dir(root) {
        modules.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.join("src").is_dir()),
        );
    }
    let sets: &[&str] = match test_folders {
        true => &["main", "test"],
        false => &["main"],
    };
    let mut classes = Vec::new();
    let mut sources = Vec::new();
    for module in modules {
        for set in sets {
            let output = match *set {
                "main" => "classes",
                _ => "test-classes",
            };
            classes.push(module.join("target").join(output));
            for language in ["java", "kotlin"] {
                classes.push(
                    module
                        .join("build")
                        .join("classes")
                        .join(language)
                        .join(set),
                );
                sources.push(module.join("src").join(set).join(language));
            }
        }
    }
    classes.retain(|folder| folder.is_dir());
    sources.retain(|folder| folder.is_dir());
    (classes, sources)
}

/// The engine `spork_coverage` reads suites and writes reports with.
struct JavaEngine;

impl spork_coverage::Engine for JavaEngine {
    fn load(&self, suite: &Suite, workspace: &Workspace, cx: &mut App) -> Task<Result<Data>> {
        let Some(jdk) = JavaProject::of(workspace, cx).and_then(|java| java.read(cx).jdk.clone())
        else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let test_folders = spork_settings::coverage::settings(cx).test_folders;
        let (file, root) = (suite.file.clone(), suite.root.clone());
        let http = cx.http_client();
        cx.background_spawn(async move {
            let agent = fetch(&http, &AGENT).await?;
            let helper = helper_classes()?;
            let separator = if cfg!(windows) { ";" } else { ":" };
            let classpath = format!(
                "{}{separator}{}",
                helper.to_string_lossy(),
                agent.to_string_lossy()
            );
            let output = util::command::new_command(jdk.java())
                .arg("-classpath")
                .arg(classpath)
                .arg("spork.coverage.Data")
                .arg(&file)
                .arg(file.with_extension("sm"))
                .output()
                .await
                .context("running the coverage helper")?;
            anyhow::ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Failed to load coverage data")
            );
            let value: Value = serde_json::from_slice(&output.stdout)
                .context("reading the coverage helper's answer")?;
            let files = source_files(&root);
            Ok(parse(&value, &files, test_folders))
        })
    }

    fn report(
        &self,
        suites: &[Suite],
        folder: PathBuf,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<PathBuf>> {
        let Some(jdk) = JavaProject::of(workspace, cx).and_then(|java| java.read(cx).jdk.clone())
        else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let test_folders = spork_settings::coverage::settings(cx).test_folders;
        let reports: Vec<PathBuf> = suites.iter().map(|suite| suite.file.clone()).collect();
        let title = suites
            .first()
            .map(|suite| suite.name.clone())
            .unwrap_or_default();
        let roots: Vec<PathBuf> = suites.iter().map(|suite| suite.root.clone()).collect();
        let http = cx.http_client();
        cx.background_spawn(async move {
            let mut jars = vec![fetch(&http, &AGENT).await?];
            for artifact in &REPORTER {
                jars.push(fetch(&http, artifact).await?);
            }
            let helper = helper_classes()?;
            let separator = if cfg!(windows) { ";" } else { ":" };
            let classpath = std::iter::once(helper)
                .chain(jars)
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(separator);
            let mut classes = Vec::new();
            let mut sources = Vec::new();
            for root in roots {
                let (module_classes, module_sources) = module_folders(&root, test_folders);
                classes.extend(module_classes);
                sources.extend(module_sources);
            }
            classes.dedup();
            sources.dedup();
            let mut command = util::command::new_command(jdk.java());
            command
                .arg("-classpath")
                .arg(classpath)
                .arg("spork.coverage.HtmlReport")
                .arg(&folder)
                .arg(title)
                .arg("--reports")
                .args(&reports)
                .arg("--classes")
                .args(&classes)
                .arg("--sources")
                .args(&sources);
            let output = command
                .output()
                .await
                .context("running the coverage report")?;
            anyhow::ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Failed to generate the coverage report")
            );
            Ok(folder.join("index.html"))
        })
    }
}

pub(crate) fn init(cx: &mut App) {
    spork_coverage::register_engine(std::rc::Rc::new(JavaEngine), cx);
}

#[cfg(test)]
mod tests {
    use super::{annotation_regex, class_regex, find_source, in_test_folder, parse};
    use collections::HashMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn reads_the_helpers_data() {
        let value = serde_json::json!({
            "classes": [
                {"name": "com.example.Calc", "source": "Calc.java", "lines": [
                    {"line": 5, "hits": 3, "status": 2, "method": "sign(I)", "jumps": [[2, 1]], "switches": []},
                    {"line": 8, "hits": 1, "status": 1, "method": "sign(I)", "jumps": [],
                     "switches": [{"keys": [-1, 0], "hits": [0, 1], "default": 0}]}
                ]},
                {"name": "com.example.CalcTest", "source": null, "lines": []}
            ],
            "tests": {"com.example.CalcTest,signs": {"com.example.Calc": [5]}}
        });
        let mut files: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::default();
        for file in [
            "/p/src/main/java/com/example/Calc.java",
            "/p/src/test/java/com/example/CalcTest.java",
        ] {
            let file = PathBuf::from(file);
            let key = (
                file.parent().map(Path::to_path_buf).unwrap_or_default(),
                file.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            files.insert(key, vec![file]);
        }
        let data = parse(&value, &files, false);
        assert_eq!(data.classes.len(), 1, "the test class is left out");
        let class = &data.classes[0];
        assert_eq!(
            class.file.as_deref(),
            Some(Path::new("/p/src/main/java/com/example/Calc.java"))
        );
        assert_eq!(class.lines[0].jumps[0].true_hits, 2);
        assert_eq!(class.lines[1].switches[0].keys, [-1, 0]);
        assert_eq!(
            data.tests_of_line(Path::new("/p/src/main/java/com/example/Calc.java"), 5),
            ["com.example.CalcTest,signs"]
        );
        assert!(find_source(&files, "com.other.Calc", "Calc.java").is_none());
    }

    #[test]
    fn writes_patterns_as_intellij_does() {
        assert_eq!(class_regex("com.foo.*"), "com\\.foo\\..*");
        assert_eq!(class_regex("com.foo.Bar"), "com\\.foo\\.Bar(\\$.*)*");
        assert_eq!(annotation_regex("*Generated*"), ".*Generated.*");
        assert!(in_test_folder(Path::new("/p/src/test/java/A.java")));
        assert!(!in_test_folder(Path::new("/p/src/main/java/A.java")));
    }
}
