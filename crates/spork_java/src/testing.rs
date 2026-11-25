//! IntelliJ's JUnit and TestNG run configurations and the gutter's test icons: tests run as
//! IntelliJ runs them, with the project's tool deciding how. A Gradle project's go to Gradle
//! (IntelliJ's default "Run tests using: Gradle"): `:cleanTest :test --tests …` with an init
//! script whose test listener reports each test to the IDE. A Maven project's run on Spork IDE's
//! own test runner (`test-runner/`, as IntelliJ runs its own): after JDT LS's build, on the
//! module's test classpath JDT LS resolves, or, for Kotlin, which JDT LS doesn't build, after
//! Maven's `test-compile` on the exec plugin's test classpath. Either way the Run tool window
//! draws the test tree from the events (`spork_run::test_runner`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use collections::HashMap;
use gpui::{App, AppContext as _, Task};
use http_client::{AsyncBody, HttpClient};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use spork_run::configurations::{self, Configuration, Field, Runner};
use spork_run::test_runner::TEST_RUN;
use task::TaskTemplate;
use workspace::Workspace;

use crate::java_project::{JavaProject, Tool};

/// The test runner's classes, built from `test-runner/src` by `test-runner/build.sh`.
const RUNNER_CLASSES: [(&str, &[u8]); 11] = [
    (
        "TestRunner.class",
        include_bytes!("../test-runner/classes/spork/tests/TestRunner.class"),
    ),
    (
        "TestRunner$Events.class",
        include_bytes!("../test-runner/classes/spork/tests/TestRunner$Events.class"),
    ),
    (
        "TestRunner$Tee.class",
        include_bytes!("../test-runner/classes/spork/tests/TestRunner$Tee.class"),
    ),
    (
        "JUnit4Tests.class",
        include_bytes!("../test-runner/classes/spork/tests/JUnit4Tests.class"),
    ),
    (
        "JUnit4Tests$1.class",
        include_bytes!("../test-runner/classes/spork/tests/JUnit4Tests$1.class"),
    ),
    (
        "JUnit4Tests$Listener.class",
        include_bytes!("../test-runner/classes/spork/tests/JUnit4Tests$Listener.class"),
    ),
    (
        "PlatformTests.class",
        include_bytes!("../test-runner/classes/spork/tests/PlatformTests.class"),
    ),
    (
        "PlatformTests$1.class",
        include_bytes!("../test-runner/classes/spork/tests/PlatformTests$1.class"),
    ),
    (
        "PlatformTests$Listener.class",
        include_bytes!("../test-runner/classes/spork/tests/PlatformTests$Listener.class"),
    ),
    (
        "TestNgTests.class",
        include_bytes!("../test-runner/classes/spork/tests/TestNgTests.class"),
    ),
    (
        "TestNgTests$Listener.class",
        include_bytes!("../test-runner/classes/spork/tests/TestNgTests$Listener.class"),
    ),
];

/// The folder of the test runner's classes in the data folder, written the first time, a
/// folder per build of them.
pub fn runner_classes() -> Result<PathBuf> {
    let mut digest = Sha256::new();
    for (name, bytes) in RUNNER_CLASSES {
        digest.update(name.as_bytes());
        digest.update(bytes);
    }
    let hash: String = digest.finalize()[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let folder = paths::data_dir()
        .join("java")
        .join("test-runner")
        .join(hash);
    let classes = folder.join("spork").join("tests");
    if !classes.join("TestRunner.class").is_file() {
        std::fs::create_dir_all(&classes).with_context(|| format!("creating {classes:?}"))?;
        for (name, bytes) in RUNNER_CLASSES {
            std::fs::write(classes.join(name), bytes)
                .with_context(|| format!("writing the test runner's {name}"))?;
        }
    }
    Ok(folder)
}

/// Maven Central, where JUnit's launcher comes from when the classpath lacks it.
const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

/// The classpath with the JUnit Platform's launcher of the engine's version on it: a JUnit 5 or 6
/// project needs it to run its tests, and seldom declares it (surefire and Gradle bring their
/// own, IntelliJ adds it as this does), taken from the local Maven repository, downloaded from
/// Maven Central into it first when missing, its SHA-1 checked.
pub async fn with_launcher(
    mut classpath: Vec<String>,
    http: Arc<dyn HttpClient>,
) -> Result<Vec<String>> {
    let jar_name = |path: &str| {
        Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    if classpath
        .iter()
        .any(|path| jar_name(path).starts_with("junit-platform-launcher-"))
    {
        return Ok(classpath);
    }
    let Some(version) = classpath.iter().find_map(|path| {
        Some(
            jar_name(path)
                .strip_prefix("junit-platform-engine-")?
                .strip_suffix(".jar")?
                .to_owned(),
        )
    }) else {
        return Ok(classpath);
    };
    let relative = format!(
        "org/junit/platform/junit-platform-launcher/{version}/junit-platform-launcher-{version}.jar"
    );
    let jar = paths::home_dir()
        .join(".m2")
        .join("repository")
        .join(&relative);
    if !jar.is_file() {
        let url = format!("{MAVEN_CENTRAL}/{relative}");
        let bytes = download(&http, &url).await?;
        let expected = String::from_utf8(download(&http, &format!("{url}.sha1")).await?)?;
        let actual: String = sha1_hex(&bytes);
        anyhow::ensure!(
            expected.split_whitespace().next() == Some(actual.as_str()),
            "the checksum of {url} doesn't match"
        );
        if let Some(folder) = jar.parent() {
            std::fs::create_dir_all(folder).with_context(|| format!("creating {folder:?}"))?;
        }
        std::fs::write(&jar, bytes).with_context(|| format!("writing {jar:?}"))?;
    }
    classpath.push(jar.to_string_lossy().into_owned());
    Ok(classpath)
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn download(http: &Arc<dyn HttpClient>, url: &str) -> Result<Vec<u8>> {
    use futures::AsyncReadExt as _;
    let mut response = http
        .get(url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("fetching {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "fetching {url} failed with status {}",
        response.status()
    );
    let mut bytes = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("reading {url}"))?;
    Ok(bytes)
}

/// The init script that reports a Gradle build's tests to the IDE (IntelliJ's own does the same
/// for its test runner): each test task's suites, tests, output and results, as service
/// messages on the port the build gets as `spork.test.port`.
const GRADLE_INIT_SCRIPT: &str = r#"// Spork IDE's test events: each test task's tests reported to the IDE
import org.gradle.api.tasks.testing.Test
import org.gradle.api.tasks.testing.TestDescriptor
import org.gradle.api.tasks.testing.TestListener
import org.gradle.api.tasks.testing.TestOutputEvent
import org.gradle.api.tasks.testing.TestOutputListener
import org.gradle.api.tasks.testing.TestResult

def sporkPort = System.getProperty('spork.test.port') ?: System.getenv('SPORK_TEST_PORT')
if (sporkPort) {
    def sporkEvents = new SporkTestEvents(sporkPort as int)
    allprojects {
        tasks.withType(Test).configureEach { task ->
            task.addTestListener(sporkEvents)
            task.addTestOutputListener(sporkEvents)
        }
    }
}

class SporkTestEvents implements TestListener, TestOutputListener {
    private final int port
    private Writer out
    private final Map<String, String> ids = [:]
    private int next = 0

    SporkTestEvents(int port) { this.port = port }

    private synchronized void send(String name, Map attributes) {
        if (out == null) {
            out = new OutputStreamWriter(new Socket('127.0.0.1', port).getOutputStream(), 'UTF-8')
        }
        def line = new StringBuilder('##teamcity[').append(name)
        attributes.each { key, value ->
            if (value != null) {
                line.append(' ').append(key).append("='").append(escape(value.toString())).append("'")
            }
        }
        out.write(line.append(']\n').toString())
        out.flush()
    }

    private static String escape(String text) {
        text.replace('|', '||').replace("'", "|'").replace('\n', '|n').replace('\r', '|r')
            .replace('[', '|[').replace(']', '|]')
    }

    private static String trace(Throwable failure) {
        def text = new StringWriter()
        failure.printStackTrace(new PrintWriter(text))
        def kept = new StringBuilder()
        for (String line : text.toString().split('\n')) {
            def frame = line.trim()
            if (frame.startsWith('at ') && ['java.base/jdk.internal.reflect.', 'jdk.internal.reflect.',
                    'sun.reflect.', 'java.base/java.lang.reflect.', 'java.lang.reflect.', 'org.junit.runners.',
                    'org.junit.platform.', 'org.gradle.'].any { frame.substring(3).startsWith(it) }) {
                break
            }
            kept.append(line.replace('\r', '')).append('\n')
        }
        kept.toString()
    }

    private static String key(TestDescriptor descriptor) {
        descriptor.hasProperty('id') ? descriptor.id.toString() : System.identityHashCode(descriptor).toString()
    }

    private String parentOf(TestDescriptor descriptor) {
        descriptor.parent == null ? '0' : (ids[key(descriptor.parent)] ?: '0')
    }

    // Gradle's own suites (the run, its executors) aren't shown: their children are the root's
    private static boolean shown(TestDescriptor suite) {
        suite.parent != null && suite.className != null
    }

    void beforeSuite(TestDescriptor suite) {
        if (!shown(suite)) {
            ids[key(suite)] = parentOf(suite)
            return
        }
        def id = (++next).toString()
        ids[key(suite)] = id
        def name = suite.className.substring(suite.className.lastIndexOf('.') + 1)
        send('testSuiteStarted', [nodeId: id, parentNodeId: parentOf(suite), name: name,
                                  locationHint: "java:suite://${suite.className}"])
    }

    void afterSuite(TestDescriptor suite, TestResult result) {
        if (shown(suite)) {
            send('testSuiteFinished', [nodeId: ids[key(suite)]])
        } else if (suite.parent == null) {
            synchronized (this) {
                out?.close()
                out = null
            }
        }
    }

    void beforeTest(TestDescriptor test) {
        def id = (++next).toString()
        ids[key(test)] = id
        def method = test.name.replaceAll(/\(.*\)$/, '')
        send('testStarted', [nodeId: id, parentNodeId: parentOf(test), name: test.displayName ?: test.name,
                             locationHint: "java:test://${test.className}/${method}"])
    }

    void afterTest(TestDescriptor test, TestResult result) {
        def id = ids[key(test)]
        if (result.resultType == TestResult.ResultType.FAILURE && result.exception != null) {
            def attributes = [nodeId: id, message: result.exception.message ?: '', details: trace(result.exception)]
            def details = result.hasProperty('failures') && result.failures ? result.failures[0].details : null
            if (details != null && details.assertionFailure && details.expected != null && details.actual != null) {
                attributes += [type: 'comparisonFailure', expected: details.expected, actual: details.actual]
            }
            send('testFailed', attributes)
        } else if (result.resultType == TestResult.ResultType.SKIPPED) {
            send('testIgnored', [nodeId: id, message: ''])
        }
        send('testFinished', [nodeId: id, duration: result.endTime - result.startTime])
    }

    void onOutput(TestDescriptor test, TestOutputEvent event) {
        def id = ids[key(test)]
        if (id != null && id != '0') {
            def name = event.destination == TestOutputEvent.Destination.StdErr ? 'testStdErr' : 'testStdOut'
            send(name, [nodeId: id, out: event.message])
        }
    }
}
"#;

/// The init script, written into the data folder.
fn gradle_init_script() -> Result<PathBuf> {
    let path = paths::data_dir()
        .join("java")
        .join("test-runner")
        .join("spork-test-events.gradle");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(GRADLE_INIT_SCRIPT) {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).with_context(|| format!("creating {folder:?}"))?;
        }
        std::fs::write(&path, GRADLE_INIT_SCRIPT).with_context(|| format!("writing {path:?}"))?;
    }
    Ok(path)
}

/// Gradle's `--tests` filters for the test runner's selectors.
// shortcut: a Pattern kind's regular expression goes to Gradle as written, which reads
// wildcards; translate it once a pattern needs more than `*`
fn gradle_filters(selectors: &[String]) -> Vec<String> {
    selectors
        .iter()
        .filter_map(|selector| {
            let (kind, value) = selector.split_once(':')?;
            Some(match kind {
                "method" => value.replace('#', "."),
                "package" => format!("{value}.*"),
                _ => value.to_owned(),
            })
        })
        .collect()
}

/// A command line as a POSIX shell reads it, each argument quoted where needed, `raw` written as
/// given (the shell expands its variables).
fn command_line(command: &str, args: &[String], raw: &[&str]) -> String {
    std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .map(|arg| match util::shell::ShellKind::Posix.try_quote(arg) {
            Some(quoted) => quoted.into_owned(),
            None => arg.to_owned(),
        })
        .chain(raw.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the project's tests include Kotlin, which JDT LS doesn't compile.
fn has_kotlin(root: &Path) -> bool {
    ["src/test/kotlin", "src/main/kotlin"]
        .iter()
        .any(|folder| root.join(folder).is_dir())
}

/// The exec plugin, which runs the test runner on Maven's test classpath.
const EXEC_PLUGIN: &str = "org.codehaus.mojo:exec-maven-plugin:3.5.0:exec";

/// IntelliJ's JUnit and TestNG configurations, run.
pub(crate) struct TestRunner;

impl Runner for TestRunner {
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<TaskTemplate>> {
        let Some(java) = JavaProject::of(workspace, cx) else {
            return Task::ready(Err(anyhow::anyhow!("The project isn't a Java project")));
        };
        let selectors = configurations::test_selectors(configuration);
        let missing = match configurations::test_kind(configuration) {
            _ if configuration
                .entry
                .contains_key(configurations::RERUN_TESTS) =>
            {
                None
            }
            "method" if configuration.text(Field::TestMethod).trim().is_empty() => {
                Some("Method name not specified")
            }
            "package" => None,
            "pattern" if configuration.text(Field::TestPattern).trim().is_empty() => {
                Some("No pattern selected")
            }
            "class" | "method" if configuration.text(Field::Command).trim().is_empty() => {
                Some("Class not specified")
            }
            _ => None,
        };
        if let Some(missing) = missing {
            return Task::ready(Err(anyhow::anyhow!(missing)));
        }
        let label = configuration.name().to_string();
        let java_ref = java.read(cx);
        let (Some(root), Some(jdk)) = (java_ref.root.clone(), java_ref.jdk.clone()) else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let cwd = configuration
            .entry
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let mut env: HashMap<String, String> = configuration
            .entry
            .get("env")
            .and_then(Value::as_object)
            .map(|env| {
                env.iter()
                    .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        env.insert(TEST_RUN.into(), "1".into());
        let mut vm_options = configurations::split_arguments(&configuration.text(Field::VmOptions));
        // a debugger attaches once the JVM listens (Gradle's `--debug-jvm` does the same)
        let debug_jvm = configuration
            .entry
            .get(configurations::DEBUG_JVM)
            .and_then(Value::as_bool)
            == Some(true);
        if debug_jvm {
            vm_options
                .push("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=5005".into());
        }
        let runner = match runner_classes() {
            Ok(runner) => runner,
            Err(error) => return Task::ready(Err(error)),
        };
        let runner_path = runner.to_string_lossy().into_owned();
        // IntelliJ's "Run tests using: Gradle"
        if java_ref.tool == Some(Tool::Gradle) {
            let Some(wrapper) = java_ref.wrapper() else {
                return Task::ready(Err(anyhow::anyhow!("The project has no Gradle wrapper")));
            };
            let script = match gradle_init_script() {
                Ok(script) => script,
                Err(error) => return Task::ready(Err(error)),
            };
            // shortcut: the root project's test task; a multi-project build's tests need the
            // project of the test's source (Kotlin's `gradle_project_path`)
            let mut args = vec![
                "--init-script".to_owned(),
                script.to_string_lossy().into_owned(),
                ":cleanTest".into(),
                ":test".into(),
            ];
            for filter in gradle_filters(&selectors) {
                args.push("--tests".into());
                args.push(filter);
            }
            if debug_jvm {
                args.push("--debug-jvm".into());
            }
            args.extend(java_ref.tool_args(&[]));
            env.extend(java_ref.tool_env());
            let command = command_line(
                &wrapper.to_string_lossy(),
                &args,
                &["-Dspork.test.port=$SPORK_TEST_PORT"],
            );
            return Task::ready(Ok(TaskTemplate {
                label,
                command,
                cwd: Some(root.to_string_lossy().into_owned()),
                env: env.into_iter().collect(),
                ..TaskTemplate::default()
            }));
        }
        // Kotlin: Maven compiles the tests, its exec plugin gives the test classpath
        if java_ref.tool == Some(Tool::Maven) && has_kotlin(&root) {
            let Some(wrapper) = java_ref.wrapper() else {
                return Task::ready(Err(anyhow::anyhow!("The project has no Maven wrapper")));
            };
            // the exec plugin splits its arguments at spaces, but not inside double quotes
            let quote = |item: String| match item.contains(char::is_whitespace) {
                true => format!("\"{item}\""),
                false => item,
            };
            let mut line: Vec<String> = vm_options.into_iter().map(quote).collect();
            line.extend([
                "-classpath".into(),
                quote(runner_path),
                configurations::TEST_RUNNER.into(),
                "--classpath".into(),
                "%classpath".into(),
            ]);
            line.extend(selectors);
            let mut args = vec![
                "-q".to_owned(),
                "test-compile".into(),
                EXEC_PLUGIN.into(),
                "-Dexec.classpathScope=test".into(),
                format!("-Dexec.executable={}", jdk.java().to_string_lossy()),
                format!("-Dexec.workingdir={cwd}"),
                format!("-Dexec.args={}", line.join(" ")),
            ];
            args.extend(java_ref.tool_args(&[]));
            env.extend(java_ref.tool_env());
            return Task::ready(Ok(TaskTemplate {
                label,
                command: command_line(&wrapper.to_string_lossy(), &args, &[]),
                cwd: Some(root.to_string_lossy().into_owned()),
                env: env.into_iter().collect(),
                ..TaskTemplate::default()
            }));
        }
        // Maven's Java, or a plain folder: JDT LS builds and gives the test classpath
        let build = java_ref.build(cx);
        let classpath = java_ref.test_classpath(&root, cx);
        let http = cx.http_client();
        cx.background_spawn(async move {
            build.await?;
            let classpath = with_launcher(classpath.await?, http).await?;
            let separator = if cfg!(windows) { ";" } else { ":" };
            let mut args = vm_options;
            args.push("-classpath".into());
            args.push(
                std::iter::once(runner_path)
                    .chain(classpath)
                    .collect::<Vec<_>>()
                    .join(separator),
            );
            args.push(configurations::TEST_RUNNER.into());
            args.extend(selectors);
            // one command line, quoted: the runner's folder (the data folder's) holds spaces
            Ok(TaskTemplate {
                label,
                command: command_line(&jdk.java().to_string_lossy(), &args, &[]),
                cwd: Some(cwd),
                env: env.into_iter().collect(),
                ..TaskTemplate::default()
            })
        })
    }

    fn can_debug(&self, cx: &App) -> bool {
        crate::application::ApplicationRunner.can_debug(cx)
    }
}

/// The JUnit or TestNG configuration the gutter's icon makes for a test class or method.
pub fn gutter_configuration(adapter: &str, class: &str, method: Option<&str>) -> Configuration {
    configurations::test_configuration(adapter, class, method)
}

/// The test framework of a test file, by its imports: TestNG's type, JUnit's otherwise.
pub fn configuration_type(text: &str) -> &'static str {
    match text.contains("org.testng") {
        true => configurations::TESTNG,
        false => configurations::JUNIT,
    }
}

/// The Project tool window's Run and Debug items for a file or folder in a test root (IntelliJ's
/// `RunContextGroup`): a test class's own configuration, a package's "Tests in 'com.example'",
/// the root's "All Tests".
pub(crate) fn run_items(test_root: &Path, abs_path: &Path) -> Vec<project_panel::NewItem> {
    let Ok(relative) = abs_path.strip_prefix(test_root) else {
        return Vec::new();
    };
    let is_source = |extension: &str| matches!(extension, "java" | "kt");
    let file = relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(is_source);
    let folder = match file {
        true => relative.parent().unwrap_or(Path::new("")),
        false => relative,
    };
    let package: Vec<String> = folder
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let package = package.join(".");
    let configuration = match file {
        true => {
            let Some(stem) = relative.file_stem().and_then(|stem| stem.to_str()) else {
                return Vec::new();
            };
            let class = match package.is_empty() {
                true => stem.to_owned(),
                false => format!("{package}.{stem}"),
            };
            let text = std::fs::read_to_string(abs_path).unwrap_or_default();
            gutter_configuration(configuration_type(&text), &class, None)
        }
        false => {
            let name = match package.is_empty() {
                true => "All Tests".to_owned(),
                false => format!("Tests in '{package}'"),
            };
            let mut configuration = Configuration::new_debug(configurations::JUNIT, &name);
            configuration
                .entry
                .insert(configurations::TEST_KIND.into(), "package".into());
            configuration.set_text(Field::TestPackage, &package);
            configuration
        }
    };
    let name = configuration.name();
    [false, true]
        .into_iter()
        .map(|debug| {
            let configuration = configuration.clone();
            project_panel::NewItem {
                label: match debug {
                    false => format!("Run '{name}'").into(),
                    true => format!("Debug '{name}'").into(),
                },
                run: std::rc::Rc::new(move |window, cx| {
                    if let Some(workspace) = Workspace::for_window(window, cx) {
                        workspace.update(cx, |workspace, cx| {
                            spork_run::run_temporary(
                                configuration.clone(),
                                debug,
                                workspace,
                                window,
                                cx,
                            )
                        });
                    }
                }),
                after: false,
            }
        })
        .collect()
}

pub(crate) fn init(cx: &mut App) {
    for adapter in [configurations::JUNIT, configurations::TESTNG] {
        configurations::register_runner(adapter, std::rc::Rc::new(TestRunner), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{command_line, gradle_filters, gutter_configuration};
    use spork_run::configurations;

    #[test]
    fn names_and_selects_the_gutters_tests() {
        let class = gutter_configuration("JUnit", "com.example.GreetingTest", None);
        assert_eq!(class.name(), "GreetingTest");
        assert_eq!(
            configurations::test_selectors(&class),
            ["class:com.example.GreetingTest"]
        );
        let method = gutter_configuration("JUnit", "com.example.GreetingTest", Some("greets"));
        assert_eq!(method.name(), "GreetingTest.greets");
        let selectors = configurations::test_selectors(&method);
        assert_eq!(selectors, ["method:com.example.GreetingTest#greets"]);
        assert_eq!(
            gradle_filters(&selectors),
            ["com.example.GreetingTest.greets"]
        );
        assert_eq!(
            gradle_filters(&["package:com.example".into()]),
            ["com.example.*"]
        );
    }

    #[test]
    fn writes_the_command_line_for_the_shell() {
        assert_eq!(
            command_line(
                "/p/gradlew",
                &["--tests".into(), "a b".into()],
                &["-Dx=$PORT"]
            ),
            "/p/gradlew --tests 'a b' -Dx=$PORT"
        );
    }
}
