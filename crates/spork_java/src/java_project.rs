//! A workspace's Java project: the build tool its folder uses (Maven's `pom.xml`, Gradle's build
//! scripts), the JDK found, the build tool's model for its tool window, and the import: IntelliJ's
//! auto-link of a single build tool's project, or its "build scripts found" notifications when
//! both are there, starting JDT LS, which imports the project through m2e or Buildship (its
//! progress in the status bar).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use collections::HashMap;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, Task, TaskExt as _, WeakEntity, Window,
};
use language::Buffer;
use lsp::LanguageServer;
use project::Project;
use project::lsp_store::OpenLspBufferHandle;
use util::ResultExt as _;
use workspace::Workspace;
use workspace::notifications::{NotificationId, simple_message_notification::MessageNotification};

use crate::gradle_model::GradleModel;
use crate::jdk::{self, Jdk};
use crate::jdtls;
use crate::pom::Pom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Maven,
    Gradle,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Maven => "Maven",
            Tool::Gradle => "Gradle",
        }
    }

    /// The wrapper script a project of this tool may have.
    pub fn wrapper(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Tool::Maven, false) => "mvnw",
            (Tool::Maven, true) => "mvnw.cmd",
            (Tool::Gradle, false) => "gradlew",
            (Tool::Gradle, true) => "gradlew.bat",
        }
    }

    /// The build tool of the project at `root`: Maven's when it has a `pom.xml`.
    pub fn detect(root: &Path) -> Option<Tool> {
        Self::detect_all(root).first().copied()
    }

    /// Each build tool whose build scripts are at `root`, Maven's first.
    pub fn detect_all(root: &Path) -> Vec<Tool> {
        let gradle = [
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ]
        .iter()
        .any(|file| root.join(file).is_file());
        [
            (root.join("pom.xml").is_file(), Tool::Maven),
            (gradle, Tool::Gradle),
        ]
        .into_iter()
        .filter_map(|(found, tool)| found.then_some(tool))
        .collect()
    }
}

/// When Maven runs a goal or Gradle a task around the IDE's builds and Gradle's syncs: IntelliJ's
/// Execute Before/After Sync, Build and Rebuild (`MavenTasksManager.Phase`,
/// `ExternalSystemTaskActivator.Phase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Trigger {
    BeforeSync,
    AfterSync,
    BeforeBuild,
    AfterBuild,
    BeforeRebuild,
    AfterRebuild,
}

impl Trigger {
    /// Maven's, in its Build menu's order.
    pub const ALL: [Trigger; 4] = [
        Trigger::BeforeBuild,
        Trigger::AfterBuild,
        Trigger::BeforeRebuild,
        Trigger::AfterRebuild,
    ];

    /// Gradle's (`ExternalSystemView.TaskActivationGroup`).
    pub const GRADLE: [Trigger; 6] = [
        Trigger::BeforeSync,
        Trigger::AfterSync,
        Trigger::BeforeBuild,
        Trigger::AfterBuild,
        Trigger::BeforeRebuild,
        Trigger::AfterRebuild,
    ];

    /// How a goal's node names it (`maven.tasks.goal.before.compile`…).
    pub fn label(self) -> &'static str {
        match self {
            Trigger::BeforeSync => "Before Sync",
            Trigger::AfterSync => "After Sync",
            Trigger::BeforeBuild => "Before Build",
            Trigger::AfterBuild => "After Build",
            Trigger::BeforeRebuild => "Before Rebuild",
            Trigger::AfterRebuild => "After Rebuild",
        }
    }

    /// The triggers a build runs around it: before or after, a Rebuild's or a Build's.
    pub fn of(full: bool, before: bool) -> Self {
        match (full, before) {
            (false, true) => Trigger::BeforeBuild,
            (false, false) => Trigger::AfterBuild,
            (true, true) => Trigger::BeforeRebuild,
            (true, false) => Trigger::AfterRebuild,
        }
    }
}

/// What Download Sources and Documentation fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    Sources,
    Documentation,
    Both,
}

/// The folder roles the Project tool window reads, shared with it.
pub(crate) struct FolderRoles(pub(crate) Arc<JavaFolderRoles>);

impl Global for FolderRoles {}

/// The Java project of each workspace, by the workspace's entity.
#[derive(Default)]
pub(crate) struct Projects(pub(crate) HashMap<EntityId, Entity<JavaProject>>);

impl Global for Projects {}

pub struct JavaProject {
    pub workspace: WeakEntity<Workspace>,
    pub(crate) project: Entity<Project>,
    /// The project's first folder.
    pub root: Option<PathBuf>,
    pub tool: Option<Tool>,
    pub jdk: Option<Jdk>,
    pub pom: Option<Pom>,
    /// Why the POM couldn't be read.
    pub pom_error: Option<String>,
    pub gradle: Option<GradleModel>,
    pub gradle_loading: bool,
    pub gradle_error: Option<String>,
    /// Loaded: JDT LS imports it (IntelliJ's linked project).
    pub linked: bool,
    /// Maven's and Gradle's Toggle Offline Mode.
    pub offline: bool,
    /// Maven's Toggle 'Skip Tests' Mode.
    pub skip_tests: bool,
    /// The goals Maven runs around the IDE's builds, kept in the data folder.
    pub triggers: Vec<(Trigger, String)>,
    /// The Maven profiles toggled away from their default, on or off.
    pub profiles: Vec<(String, bool)>,
    /// What a Download Sources/Documentation run fetches, while it runs.
    pub downloading: Option<Download>,
    /// The build tools whose scripts are in the project's folder, Maven's first.
    found: Vec<Tool>,
    announced: bool,
    /// A Java file opened with the language servers, which keeps JDT LS running for the project.
    language_server: Option<(Entity<Buffer>, OpenLspBufferHandle)>,
    _subscriptions: Vec<gpui::Subscription>,
    refresh: Task<()>,
    link: Task<()>,
    gradle_task: Task<()>,
    download_task: Task<()>,
    sync_task: Task<()>,
    libraries_task: Task<()>,
    /// External Libraries' worktree, held: a project keeps an invisible worktree only while
    /// something holds it.
    libraries_worktree: Option<Entity<project::Worktree>>,
    /// Spring Boot's state, for a Spring Boot project.
    pub(crate) spring: crate::spring_boot::SpringBoot,
}

pub(crate) fn register(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let workspace_handle = cx.entity();
    let project = workspace.project().clone();
    let java = cx.new(|cx| JavaProject::new(workspace_handle.downgrade(), project, cx));
    let id = workspace_handle.entity_id();
    cx.default_global::<Projects>().0.insert(id, java.clone());
    cx.on_release(move |_, cx| {
        cx.default_global::<Projects>().0.remove(&id);
    })
    .detach();
    // the build tool's tool window, once the project says which
    let mut added = false;
    cx.observe_in(&java, window, move |_, java, window, cx| {
        if added {
            return;
        }
        if let Some(tool) = java.read(cx).tool {
            added = true;
            let spring = java
                .read(cx)
                .root
                .as_deref()
                .is_some_and(crate::spring_boot::is_project);
            cx.defer_in(window, move |workspace, window, cx| {
                crate::build_tool_window::add(workspace, tool, window, cx);
                crate::build_view::add(workspace, window, cx);
                if spring {
                    crate::endpoints::add(workspace, window, cx);
                    crate::beans::add(workspace, window, cx);
                }
            });
        }
    })
    .detach();
    crate::build_view::register(workspace);
    crate::project_structure::register(workspace);
    crate::jdk_banner::register(workspace, window, cx);
    // after the app's own items: the right side draws its last item first, as IntelliJ shows a
    // background task before the widgets, and an idle one then moves none of them
    cx.defer_in(window, |workspace, window, cx| {
        let downloads = cx.new(crate::jdk_download::DownloadStatus::new);
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_right_item(downloads, window, cx)
        });
    });
}

impl JavaProject {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&project, |this, _, event, cx| {
            if let project::Event::WorktreeAdded(_) | project::Event::WorktreeRemoved(_) = event {
                this.refresh(cx);
            }
        });
        let mut this = Self {
            workspace,
            project,
            root: None,
            tool: None,
            jdk: None,
            pom: None,
            pom_error: None,
            gradle: None,
            gradle_loading: false,
            gradle_error: None,
            linked: false,
            offline: false,
            skip_tests: false,
            triggers: Vec::new(),
            profiles: Vec::new(),
            downloading: None,
            found: Vec::new(),
            announced: false,
            language_server: None,
            _subscriptions: vec![subscription],
            refresh: Task::ready(()),
            link: Task::ready(()),
            gradle_task: Task::ready(()),
            download_task: Task::ready(()),
            sync_task: Task::ready(()),
            libraries_task: Task::ready(()),
            libraries_worktree: None,
            spring: Default::default(),
        };
        this.refresh(cx);
        this
    }

    /// The Java project of the workspace.
    pub fn of(workspace: &Workspace, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Projects>()?
            .0
            .get(&workspace.weak_handle().entity_id())
            .cloned()
    }

    /// The Java project whose folder holds `path`.
    pub fn containing(path: &Path, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Projects>()?
            .0
            .values()
            .find(|java| {
                java.read(cx)
                    .root
                    .as_ref()
                    .is_some_and(|root| path.starts_with(root))
            })
            .cloned()
    }

    /// The project's name, as IntelliJ names a project: its folder's, unless Project Structure
    /// renamed it.
    pub fn name(&self) -> String {
        let Some(root) = &self.root else {
            return String::new();
        };
        crate::project_settings::ProjectSettings::load(root)
            .name
            .or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    }

    /// Reads the project's folder again: its build tool, the JDK, the POM.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
        else {
            return;
        };
        self.refresh = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    let tools = Tool::detect_all(&root);
                    // shortcut: the IDE's own environment, where JDT LS gets the project's shell
                    // one; they differ only when the shell alone sets JAVA_HOME
                    let jdk = jdk::for_module(&root, &jdk::process_env());
                    let pom = tools.contains(&Tool::Maven).then(|| {
                        std::fs::read_to_string(root.join("pom.xml"))
                            .context("reading pom.xml")
                            .and_then(|text| Pom::parse(&text))
                    });
                    let triggers = std::fs::read_to_string(triggers_file(&root))
                        .ok()
                        .and_then(|text| serde_json::from_str(&text).log_err())
                        .unwrap_or_default();
                    (root, tools, jdk, pom, triggers)
                })
                .await;
            let (root, tools, jdk, pom, triggers) = read;
            this.update(cx, |this, cx| {
                this.root = Some(root);
                this.triggers = triggers;
                // the tool a project with both was loaded with stays its tool
                this.tool = this
                    .tool
                    .filter(|tool| this.linked && tools.contains(tool))
                    .or(tools.first().copied());
                this.found = tools;
                this.jdk = jdk;
                match pom {
                    Some(Ok(pom)) => {
                        this.pom = Some(pom);
                        this.pom_error = None;
                    }
                    Some(Err(error)) => {
                        this.pom = None;
                        this.pom_error = Some(format!("{error:#}"));
                    }
                    None => {}
                }
                this.announce(cx);
                cx.notify();
            })
            .ok();
        });
    }

    /// Setup SDK's choice: `jdk` becomes the project's JDK, and the project's import is offered
    /// as when it opened with one.
    pub fn set_jdk(&mut self, jdk: Jdk, cx: &mut Context<Self>) {
        if let Some(root) = &self.root {
            jdk::choose(root, &jdk).log_err();
            // the module's own JDK stays the one it builds on
            self.jdk = jdk::for_module(root, &jdk::process_env());
        }
        if self.jdk.is_none() {
            self.jdk = Some(jdk);
        }
        self.restart_language_server(cx);
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.dismiss_notification(&NotificationId::unique::<NoJdk>(), cx)
            });
        }
        self.announced = false;
        self.announce(cx);
        cx.notify();
    }

    /// IntelliJ's auto-link (`UnlinkedProjectStartupActivity`): a project opened with a single
    /// build tool's scripts is linked and loaded without asking; with both Maven's and Gradle's,
    /// each has its "build scripts found" notification, whose Load links the project with it. A
    /// project imported before is loaded again, as IntelliJ syncs a linked project when it opens.
    fn announce(&mut self, cx: &mut Context<Self>) {
        let (Some(_), Some(root)) = (self.tool, self.root.clone()) else {
            return;
        };
        if self.announced {
            return;
        }
        self.announced = true;
        let usable = self.jdk.as_ref().is_some_and(|jdk| jdk.major >= 17);
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if !usable {
            workspace.update(cx, |workspace, cx| {
                workspace.show_notification(NotificationId::unique::<NoJdk>(), cx, |cx| {
                    cx.new(|cx| {
                        MessageNotification::new(
                            "Spork IDE runs Java on a JDK 17 or newer, and none was found \
                             (JAVA_HOME, the java on the PATH, /Library/Java/JavaVirtualMachines).",
                            cx,
                        )
                        .with_title(jdtls::NO_JDK)
                    })
                });
            });
            return;
        }
        if self.found.len() == 1 || jdtls::project_data(&root, "workspaces").exists() {
            self.load(cx);
            return;
        }
        let this = cx.entity().downgrade();
        let name = self.name();
        workspace.update(cx, |workspace, cx| {
            for tool in self.found.clone() {
                let title = format!("{} '{name}' build scripts found", tool.name());
                let id = NotificationId::composite::<BuildScriptsFound>(tool.name());
                let this = this.clone();
                workspace.show_notification(id, cx, |cx| {
                    cx.new(|cx| {
                        // IntelliJ's balloon has the title alone, its help behind a "?"
                        MessageNotification::new(title, cx)
                            .primary_message(format!("Load {} Project", tool.name()))
                            .primary_on_click(move |_, cx| {
                                this.update(cx, |this, cx| {
                                    this.tool = Some(tool);
                                    this.load(cx);
                                })
                                .ok();
                                cx.emit(gpui::DismissEvent);
                            })
                            .secondary_message("Skip")
                            .secondary_on_click(|_, cx| cx.emit(gpui::DismissEvent))
                    })
                });
            }
        });
    }

    /// Links the project: JDT LS starts on one of its Java files and imports it.
    // shortcut: a project without a Java file isn't imported until one is opened; start JDT LS
    // on the build file once the language servers can start without a buffer
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.linked = true;
        if let (Some(root), Some(roles)) = (&self.root, cx.try_global::<FolderRoles>()) {
            roles.0.link(root);
        }
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                for tool in [Tool::Maven, Tool::Gradle] {
                    let id = NotificationId::composite::<BuildScriptsFound>(tool.name());
                    workspace.dismiss_notification(&id, cx)
                }
            });
        }
        if self.tool == Some(Tool::Gradle) {
            self.load_gradle(cx);
        }
        let project = self.project.clone();
        let Some(worktree) = project.read(cx).visible_worktrees(cx).next() else {
            return;
        };
        let scanned = worktree
            .read(cx)
            .as_local()
            .map(|local| local.scan_complete());
        self.link = cx.spawn(async move |this, cx| {
            if let Some(scanned) = scanned {
                scanned.await;
            }
            let root = worktree.read_with(cx, |worktree, _| worktree.abs_path().to_path_buf());
            let found = worktree.read_with(cx, |worktree, _| {
                worktree
                    .entries(false, 0)
                    .find(|entry| {
                        entry.is_file()
                            && entry.path.extension() == Some("java")
                            && !entry.path.as_unix_str().contains("/test/")
                    })
                    .or_else(|| {
                        worktree
                            .entries(false, 0)
                            .find(|entry| entry.is_file() && entry.path.extension() == Some("java"))
                    })
                    .map(|entry| worktree.abs_path().join(entry.path.as_std_path()))
            });
            // a folder outside git is scanned 5 levels deep at first, `src/main/java/com/x`'s
            // sources deeper still
            let found = match found {
                Some(file) => Some(file),
                None => {
                    cx.background_spawn(async move { first_java_file(&root) })
                        .await
                }
            };
            let Some(java_file) = found else {
                // a Kotlin project's applications, which JDT LS wouldn't see
                this.update(cx, crate::spring_boot::imported_without_java)
                    .ok();
                return;
            };
            let opened = project
                .update(cx, |project, cx| project.open_local_buffer(java_file, cx))
                .await;
            match opened {
                Ok(buffer) => {
                    let handle = project.update(cx, |project, cx| {
                        project.register_buffer_with_language_servers(&buffer, cx)
                    });
                    this.update(cx, |this, cx| {
                        this.language_server = Some((buffer, handle));
                        this.show_libraries(cx);
                        crate::spring_boot::imported(this, cx);
                        cx.notify();
                    })
                    .ok();
                }
                Err(error) => log::error!("starting the Java language server: {error:#}"),
            }
        });
        cx.notify();
    }

    /// Restarts the project's JDT LS on the JDK and settings the project has now, when it runs.
    pub fn restart_language_server(&mut self, cx: &mut Context<Self>) {
        let buffers: Vec<Entity<Buffer>> = self
            .project
            .read(cx)
            .opened_buffers(cx)
            .into_iter()
            .filter(|buffer| {
                buffer
                    .read(cx)
                    .language()
                    .is_some_and(|language| language.name().as_ref() == "Java")
            })
            .collect();
        if buffers.is_empty() || self.language_server(cx).is_none() {
            return;
        }
        let only = collections::HashSet::from_iter([lsp::LanguageServerSelector::Name(
            jdtls::SERVER_NAME,
        )]);
        self.project.update(cx, |project, cx| {
            project.restart_language_servers_for_buffers(buffers, only, cx)
        });
    }

    /// The project's JDT LS, once it runs.
    pub fn language_server(&self, cx: &App) -> Option<Arc<LanguageServer>> {
        language_server(&self.project, cx)
    }

    /// Whether JDT LS is still importing or building: work in progress, or its diagnostics.
    pub fn language_server_busy(&self, cx: &App) -> Option<bool> {
        let project = self.project.read(cx);
        project
            .language_server_statuses(cx)
            .find(|(_, status)| status.name == jdtls::SERVER_NAME)
            .map(|(_, status)| {
                !status.pending_work.is_empty() || status.has_pending_diagnostic_updates
            })
    }

    /// Reload All Maven Projects / Sync All Gradle Projects: reads the build files again and has
    /// JDT LS update the project's configuration from them.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.announced = true;
        let before = self.goals_on(Trigger::BeforeSync);
        if !before.is_empty() {
            // Execute Before Sync: the sync waits for the tasks, and goes on when they fail
            let ran = self.run_goals(before, cx);
            self.sync_task = cx.spawn(async move |this, cx| {
                ran.await.log_err();
                this.update(cx, |this, cx| this.sync(false, cx)).ok();
            });
            return;
        }
        self.sync(false, cx);
    }

    /// Refresh Gradle Dependencies: a sync whose Gradle run checks the dependencies again
    /// (`--refresh-dependencies`).
    pub fn refresh_dependencies(&mut self, cx: &mut Context<Self>) {
        self.announced = true;
        self.sync(true, cx);
    }

    fn sync(&mut self, refresh_dependencies: bool, cx: &mut Context<Self>) {
        self.refresh(cx);
        if self.tool == Some(Tool::Gradle) {
            self.load_gradle_with(refresh_dependencies, cx);
        }
        if !self.linked {
            self.load(cx);
            return;
        }
        let (Some(server), Some(root), Some(tool)) =
            (self.language_server(cx), self.root.clone(), self.tool)
        else {
            return;
        };
        let file = match tool {
            Tool::Maven => root.join("pom.xml"),
            Tool::Gradle => ["build.gradle", "build.gradle.kts", "settings.gradle"]
                .iter()
                .map(|name| root.join(name))
                .find(|path| path.is_file())
                .unwrap_or_else(|| root.join("build.gradle")),
        };
        let Ok(uri) = lsp::Uri::from_file_path(&file) else {
            return;
        };
        server
            .notify::<ProjectConfigurationUpdate>(lsp::TextDocumentIdentifier { uri })
            .log_err();
        self.show_libraries(cx);
        crate::spring_boot::imported(self, cx);
    }

    /// External Libraries: once JDT LS has imported the project, its JDK and the jars of its
    /// test classpath (the widest) in the Project tool window.
    fn show_libraries(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let roles = cx.try_global::<FolderRoles>().map(|roles| roles.0.clone());
        self.libraries_task = cx.spawn(async move |this, cx| {
            // shortcut: polls until JDT LS is idle; follow its progress events once the
            // project panel needs the libraries sooner
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let Ok(busy) = this.read_with(cx, |this, cx| this.language_server_busy(cx)) else {
                    return;
                };
                if busy == Some(false) {
                    break;
                }
            }
            let Ok(classpath) = this.read_with(cx, |this, cx| this.test_classpath(&root, cx))
            else {
                return;
            };
            let Some(classpath) = classpath.await.log_err() else {
                return;
            };
            let Ok((jdk, tool)) = this.read_with(cx, |this, _| (this.jdk.clone(), this.tool))
            else {
                return;
            };
            let folder = jdtls::project_data(&root, "libraries");
            let written = cx
                .background_spawn({
                    let folder = folder.clone();
                    async move {
                        let libraries: Vec<crate::libraries::Library> = classpath
                            .iter()
                            .filter(|path| path.ends_with(".jar"))
                            .map(|jar| crate::libraries::library(Path::new(jar), tool))
                            .collect();
                        crate::libraries::write(&folder, jdk.as_ref(), &libraries)
                    }
                })
                .await;
            let Some(rows) = written.log_err() else {
                return;
            };
            if let Some(roles) = &roles {
                roles.set_libraries(&folder, rows);
            }
            let Ok(project) = this.read_with(cx, |this, _| this.project.clone()) else {
                return;
            };
            let created = project.update(cx, |project, cx| {
                project.find_or_create_worktree(&folder, false, cx)
            });
            if let Some((worktree, _)) = created.await.log_err() {
                this.update(cx, |this, cx| {
                    this.libraries_worktree = Some(worktree);
                    cx.notify();
                })
                .ok();
            }
        });
    }

    /// The project's wrapper script, when it has one.
    pub fn wrapper(&self) -> Option<PathBuf> {
        let (root, tool) = (self.root.as_ref()?, self.tool?);
        let wrapper = root.join(tool.wrapper());
        wrapper.is_file().then_some(wrapper)
    }

    /// The environment Maven and Gradle run in: the JDK found, and their caches under HOME, where
    /// the JVM's `user.home` is the account's own (a test's HOME is its own).
    pub fn tool_env(&self) -> HashMap<String, String> {
        let home = paths::home_dir().to_string_lossy().into_owned();
        let mut env = HashMap::default();
        if let Some(jdk) = &self.jdk {
            env.insert("JAVA_HOME".into(), jdk.home.to_string_lossy().into_owned());
        }
        env.insert("MAVEN_OPTS".into(), format!("-Duser.home={home}"));
        env.insert("GRADLE_USER_HOME".into(), format!("{home}/.gradle"));
        env
    }

    /// The arguments for Maven or Gradle to run `goals` with the toolbar's modes.
    pub fn tool_args(&self, goals: &[String]) -> Vec<String> {
        let mut args: Vec<String> = goals.to_vec();
        if self.offline {
            args.push("--offline".into());
        }
        if self.skip_tests && self.tool == Some(Tool::Maven) {
            args.push("-DskipTests".into());
        }
        if !self.profiles.is_empty() && self.tool == Some(Tool::Maven) {
            let profiles: Vec<String> = self
                .profiles
                .iter()
                .map(|(name, on)| match on {
                    true => name.clone(),
                    false => format!("!{name}"),
                })
                .collect();
            args.push(format!("-P{}", profiles.join(",")));
        }
        if self.tool == Some(Tool::Gradle) {
            args.push("--console=plain".into());
        }
        args
    }

    /// Runs Maven goals or Gradle tasks with the project's wrapper in the Run tool window, its
    /// tab named as IntelliJ names the configuration it makes: `fixture [compile]`.
    pub fn run_tool(this: &Entity<Self>, goals: Vec<String>, window: &mut Window, cx: &mut App) {
        let java = this.read(cx);
        let (Some(wrapper), Some(root)) = (java.wrapper(), java.root.clone()) else {
            return;
        };
        let template = task::TaskTemplate {
            label: format!("{} [{}]", java.name(), goals.join(" ")),
            command: wrapper.to_string_lossy().into_owned(),
            args: java.tool_args(&goals),
            cwd: Some(root.to_string_lossy().into_owned()),
            env: java.tool_env().into_iter().collect(),
            ..task::TaskTemplate::default()
        };
        let Some(workspace) = java.workspace.upgrade() else {
            return;
        };
        workspace.update(cx, |workspace, cx| {
            workspace.schedule_task(
                project::TaskSourceKind::UserInput,
                &template,
                &task::TaskContext::default(),
                false,
                window,
                cx,
            )
        });
    }

    /// Runs Maven goals or Gradle tasks with the project's wrapper outside a console: their output
    /// once they end, an error with it when they fail.
    pub fn run_goals(&self, goals: Vec<String>, cx: &App) -> Task<Result<String>> {
        let (Some(wrapper), Some(root)) = (self.wrapper(), self.root.clone()) else {
            return Task::ready(Err(anyhow::anyhow!(
                "The project has no {} wrapper",
                self.tool.map_or("build tool", Tool::name)
            )));
        };
        let (args, env) = (self.tool_args(&goals), self.tool_env());
        cx.background_spawn(async move {
            let output = util::command::new_command(&wrapper)
                .args(&args)
                .current_dir(&root)
                .envs(env)
                .output()
                .await
                .with_context(|| format!("running {}", wrapper.display()))?;
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            anyhow::ensure!(output.status.success(), "{text}");
            Ok(text)
        })
    }

    /// The goals run on `trigger`.
    pub fn goals_on(&self, trigger: Trigger) -> Vec<String> {
        self.triggers
            .iter()
            .filter(|(on, _)| *on == trigger)
            .map(|(_, goal)| goal.clone())
            .collect()
    }

    /// Execute Before Build and the like: runs `goal` on `trigger`, or no longer.
    pub fn toggle_trigger(&mut self, trigger: Trigger, goal: &str, cx: &mut Context<Self>) {
        match self
            .triggers
            .iter()
            .position(|(on, set)| *on == trigger && set == goal)
        {
            Some(ix) => {
                self.triggers.remove(ix);
            }
            None => self.triggers.push((trigger, goal.to_owned())),
        }
        if let Some(root) = &self.root {
            let file = triggers_file(root);
            let text = serde_json::to_string(&self.triggers).unwrap_or_default();
            cx.background_spawn(async move {
                if let Some(folder) = file.parent() {
                    std::fs::create_dir_all(folder)?;
                }
                std::fs::write(&file, text)?;
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        }
        cx.notify();
    }

    /// Whether a profile is on: toggled, or active by default.
    // shortcut: a profile's own activation (`activeByDefault`, a property, the JDK) isn't read:
    // every profile is off until toggled; read it once the POM's activation is parsed
    pub fn profile_on(&self, name: &str) -> bool {
        self.profiles
            .iter()
            .find(|(toggled, _)| toggled == name)
            .is_some_and(|(_, on)| *on)
    }

    /// Toggle Profile: on when off, off when on; back at its default, it's no longer passed.
    pub fn toggle_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        match self
            .profiles
            .iter()
            .position(|(toggled, _)| toggled == name)
        {
            Some(ix) => {
                self.profiles.remove(ix);
            }
            None => self.profiles.push((name.to_owned(), true)),
        }
        cx.notify();
    }

    /// Download Sources, Documentation, or both, of every dependency or of one
    /// (`group:artifact`), with Maven's dependency plugin; JDT LS then attaches them.
    pub fn download(
        &mut self,
        what: Download,
        only: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let mut runs: Vec<Vec<String>> = Vec::new();
        let filter: Vec<String> = only
            .iter()
            .flat_map(|(group, artifact)| {
                [
                    format!("-DincludeGroupIds={group}"),
                    format!("-DincludeArtifactIds={artifact}"),
                ]
            })
            .collect();
        if what != Download::Documentation {
            runs.push([vec!["dependency:sources".to_owned()], filter.clone()].concat());
        }
        if what != Download::Sources {
            runs.push(
                [
                    vec![
                        "dependency:resolve".to_owned(),
                        "-Dclassifier=javadoc".to_owned(),
                    ],
                    filter,
                ]
                .concat(),
            );
        }
        let runs: Vec<Task<Result<String>>> = runs
            .into_iter()
            .map(|goals| self.run_goals(goals, cx))
            .collect();
        self.downloading = Some(what);
        cx.notify();
        self.download_task = cx.spawn(async move |this, cx| {
            let mut failed = None;
            for run in runs {
                if let Err(error) = run.await {
                    failed = Some(error);
                }
            }
            this.update(cx, |this, cx| {
                this.downloading = None;
                match failed {
                    // IntelliJ's `maven.sources.cannot.download`
                    Some(error) => {
                        log::error!("downloading Maven sources: {error:#}");
                        if let Some(workspace) = this.workspace.upgrade() {
                            workspace.update(cx, |workspace, cx| {
                                workspace.show_notification(
                                    NotificationId::unique::<CannotDownload>(),
                                    cx,
                                    |cx| {
                                        cx.new(|cx| {
                                            MessageNotification::new("Cannot download sources", cx)
                                        })
                                    },
                                )
                            });
                        }
                    }
                    None => this.reload(cx),
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Reads the Gradle project's tasks and dependencies from one run of its wrapper.
    pub fn load_gradle(&mut self, cx: &mut Context<Self>) {
        self.load_gradle_with(false, cx);
    }

    /// Loads the Gradle model; the tasks set to run after a sync run once it has loaded.
    fn load_gradle_with(&mut self, refresh_dependencies: bool, cx: &mut Context<Self>) {
        let (Some(wrapper), Some(root)) = (self.wrapper(), self.root.clone()) else {
            self.gradle_error = Some("The project has no Gradle wrapper (gradlew)".into());
            cx.notify();
            return;
        };
        let env = self.tool_env();
        let mut args = vec!["tasks".to_owned(), "--all".into(), "dependencies".into()];
        args.push("--console=plain".into());
        if self.offline {
            args.push("--offline".into());
        }
        if refresh_dependencies {
            args.push("--refresh-dependencies".into());
        }
        self.gradle_loading = true;
        cx.notify();
        self.gradle_task = cx.spawn(async move |this, cx| {
            let output = cx
                .background_spawn(async move {
                    let output = util::command::new_command(&wrapper)
                        .args(&args)
                        .current_dir(&root)
                        .envs(env)
                        .output()
                        .await
                        .context("running gradlew")?;
                    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                    anyhow::ensure!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .unwrap_or("Gradle failed")
                    );
                    anyhow::Ok(GradleModel::parse(&stdout))
                })
                .await;
            this.update(cx, |this, cx| {
                this.gradle_loading = false;
                match output {
                    Ok(model) => {
                        this.gradle = Some(model);
                        this.gradle_error = None;
                    }
                    Err(error) => this.gradle_error = Some(format!("{error:#}")),
                }
                let after = this.goals_on(Trigger::AfterSync);
                if !after.is_empty() {
                    this.run_goals(after, cx).detach_and_log_err(cx);
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Asks JDT LS to build the workspace (incrementally) and says whether it built without
    /// errors.
    pub fn build(&self, cx: &App) -> Task<Result<()>> {
        let (Some(server), Some(root)) = (self.language_server(cx), self.root.clone()) else {
            return Task::ready(Err(anyhow::anyhow!(
                "The Java language server isn't running"
            )));
        };
        let scope = crate::build_view::Scope {
            full: false,
            module: false,
            recompile: false,
        };
        cx.background_spawn(async move {
            match build(&server, &root, scope).await? {
                true => Ok(()),
                false => Err(anyhow::anyhow!("Build completed with errors")),
            }
        })
    }

    /// The runtime classpath of the module that holds `file`, as JDT LS resolves it.
    pub fn classpath(&self, file: &Path, cx: &App) -> Task<Result<Vec<String>>> {
        self.classpath_of_scope(file, "runtime", cx)
    }

    /// The test classpath (the main and test dependencies) of the module that holds `file`.
    pub(crate) fn test_classpath(&self, file: &Path, cx: &App) -> Task<Result<Vec<String>>> {
        self.classpath_of_scope(file, "test", cx)
    }

    fn classpath_of_scope(
        &self,
        file: &Path,
        scope: &'static str,
        cx: &App,
    ) -> Task<Result<Vec<String>>> {
        let Some(server) = self.language_server(cx) else {
            return Task::ready(Err(anyhow::anyhow!(
                "The Java language server isn't running"
            )));
        };
        let uri = match lsp::Uri::from_file_path(file) {
            Ok(uri) => uri.to_string(),
            Err(()) => return Task::ready(Err(anyhow::anyhow!("{file:?} isn't a file path"))),
        };
        cx.background_spawn(async move {
            let result = execute_command(
                &server,
                "java.project.getClasspaths",
                vec![
                    serde_json::Value::from(uri),
                    serde_json::Value::from(format!(r#"{{"scope":"{scope}"}}"#)),
                ],
            )
            .await?;
            let classpaths = result
                .get("classpaths")
                .and_then(|paths| paths.as_array())
                .context("JDT LS gave no classpath")?;
            Ok(classpaths
                .iter()
                .filter_map(|path| path.as_str().map(str::to_owned))
                .collect())
        })
    }
}

/// The first Java source under `root` on disk, a main source before a test's, past the build's
/// output and hidden folders.
fn first_java_file(root: &Path) -> Option<PathBuf> {
    let mut folders = std::collections::VecDeque::from([root.join("src/main"), root.to_path_buf()]);
    let mut seen = 0;
    while let Some(folder) = folders.pop_front() {
        seen += 1;
        // shortcut: a bounded walk; the worktree's own scan finds what this misses
        if seen > 2000 {
            return None;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if !name.starts_with('.') && !matches!(name, "target" | "build" | "node_modules") {
                    folders.push_back(path);
                }
            } else if path
                .extension()
                .is_some_and(|extension| extension == "java")
            {
                return Some(path);
            }
        }
    }
    None
}

/// The JDT LS of `project`, once it runs.
pub(crate) fn language_server(project: &Entity<Project>, cx: &App) -> Option<Arc<LanguageServer>> {
    let lsp_store = project.read(cx).lsp_store();
    let lsp_store = lsp_store.read(cx);
    let id = lsp_store
        .language_server_statuses()
        .find(|(_, status)| status.name == jdtls::SERVER_NAME)
        .map(|(id, _)| id)?;
    lsp_store.language_server_for_id(id)
}

/// The roles of a linked project's folders in the Project tool window: the build tools'
/// source, resource and test roots, and the packages under the source roots.
// shortcut: Maven's and Gradle's conventional folders (`src/main/java`…); read the source folders
// JDT LS reports (`java.project.listSourcePaths`) for a project that moves them
#[derive(Default)]
pub(crate) struct JavaFolderRoles(
    std::sync::RwLock<Vec<PathBuf>>,
    /// External Libraries: each project's folder of links, and the rows it names.
    std::sync::RwLock<Vec<(PathBuf, Vec<crate::libraries::Row>)>>,
);

/// A root's folder in a project, its file icon, and whether packages are under it.
const ROOTS: [(&str, &str, bool); 6] = [
    ("src/main/java", "icons/file_icons/source_root.svg", true),
    ("src/main/kotlin", "icons/file_icons/source_root.svg", true),
    (
        "src/main/resources",
        "icons/file_icons/resources_root.svg",
        false,
    ),
    ("src/test/java", "icons/file_icons/test_root.svg", true),
    ("src/test/kotlin", "icons/file_icons/test_root.svg", true),
    (
        "src/test/resources",
        "icons/file_icons/test_resources_root.svg",
        false,
    ),
];

impl JavaFolderRoles {
    pub(crate) fn link(&self, root: &Path) {
        if let Ok(mut roots) = self.0.write()
            && !roots.iter().any(|linked| linked == root)
        {
            roots.push(root.to_owned());
        }
    }

    /// Shows `rows` as the External Libraries of the folder `root`.
    pub(crate) fn set_libraries(&self, root: &Path, rows: Vec<crate::libraries::Row>) {
        if let Ok(mut libraries) = self.1.write() {
            libraries.retain(|(shown, _)| shown != root);
            libraries.push((root.to_owned(), rows));
        }
    }

    /// The name and icon of an External Libraries row.
    fn library_row(&self, abs_path: &Path) -> Option<(String, &'static str)> {
        let libraries = self.1.read().ok()?;
        libraries.iter().find_map(|(_, rows)| {
            rows.iter()
                .find(|(path, _, _)| path == abs_path)
                .map(|(_, name, icon)| (name.clone(), *icon))
        })
    }

    /// The Spring Boot configuration file icon of `application*.properties|yml|yaml` and
    /// `bootstrap*…` in a linked Spring Boot project's resource roots.
    fn spring_boot_file_icon(&self, abs_path: &Path) -> Option<&'static str> {
        let ((_, packages), _) = self.root_of(abs_path.parent()?)?;
        let name = abs_path.file_name()?.to_str()?;
        let (stem, extension) = name.rsplit_once('.')?;
        if packages
            || !["application", "bootstrap"]
                .iter()
                .any(|n| stem.starts_with(n))
        {
            return None;
        }
        let project = self
            .0
            .read()
            .ok()?
            .iter()
            .find(|project| abs_path.starts_with(project))?
            .clone();
        if !crate::spring_boot::is_project(&project) {
            return None;
        }
        match extension {
            "properties" => Some("icons/file_icons/spring_boot_properties.svg"),
            "yml" | "yaml" => Some("icons/file_icons/spring_boot_yaml.svg"),
            _ => None,
        }
    }

    /// The source root with packages (`src/main/java`, `src/test/java`) `abs_path` is or is in.
    fn source_root_of(&self, abs_path: &Path) -> Option<PathBuf> {
        let roots = self.0.read().ok()?;
        roots.iter().find_map(|project| {
            let relative = abs_path.strip_prefix(project).ok()?;
            ROOTS.iter().find_map(|(folder, _, packages)| {
                (*packages && relative.starts_with(folder)).then(|| project.join(folder))
            })
        })
    }

    /// The root `abs_path` is or is in, and whether it is the root itself.
    fn root_of(&self, abs_path: &Path) -> Option<((&'static str, bool), bool)> {
        let roots = self.0.read().ok()?;
        roots.iter().find_map(|project| {
            let relative = abs_path.strip_prefix(project).ok()?;
            ROOTS.iter().find_map(|(folder, icon, packages)| {
                let inside = relative.strip_prefix(folder).ok()?;
                Some(((*icon, *packages), inside.as_os_str().is_empty()))
            })
        })
    }
}

impl project_panel::FolderRoles for JavaFolderRoles {
    fn icon(&self, abs_path: &Path) -> Option<gpui::SharedString> {
        if let Some((_, icon)) = self.library_row(abs_path) {
            return Some(icon.into());
        }
        // Spork IDE: Spring Boot's configuration files in a linked project's resources
        if let Some(icon) = self.spring_boot_file_icon(abs_path) {
            return Some(icon.into());
        }
        match self.root_of(abs_path)? {
            ((icon, _), true) => Some(icon.into()),
            ((_, true), false) => Some("icons/file_icons/java_package.svg".into()),
            _ => None,
        }
    }

    fn is_package(&self, abs_path: &Path) -> bool {
        matches!(self.root_of(abs_path), Some(((_, true), false)))
    }

    fn new_items(&self, abs_path: &Path) -> Vec<project_panel::NewItem> {
        match self.source_root_of(abs_path) {
            Some(source_root) => crate::new_class::new_items(&source_root, abs_path),
            None => Vec::new(),
        }
    }

    fn run_items(&self, abs_path: &Path) -> Vec<project_panel::NewItem> {
        let Some(source_root) = self.source_root_of(abs_path) else {
            return Vec::new();
        };
        if source_root.ends_with("src/test/java") || source_root.ends_with("src/test/kotlin") {
            return crate::testing::run_items(&source_root, abs_path);
        }
        // `src/main/java`'s project
        let project_root = source_root.ancestors().nth(3).unwrap_or(&source_root);
        crate::testing::main_items(project_root, &source_root, abs_path)
    }

    fn extra_roots(&self) -> Vec<PathBuf> {
        self.1
            .read()
            .map(|libraries| libraries.iter().map(|(root, _)| root.clone()).collect())
            .unwrap_or_default()
    }

    fn label(&self, abs_path: &Path) -> Option<gpui::SharedString> {
        self.library_row(abs_path).map(|(name, _)| name.into())
    }
}

/// Runs one of JDT LS's (or a bundle's) commands.
pub(crate) async fn execute_command(
    server: &LanguageServer,
    command: &str,
    arguments: Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    server
        .request::<lsp::request::ExecuteCommand>(
            lsp::ExecuteCommandParams {
                command: command.into(),
                arguments,
                ..Default::default()
            },
            Duration::from_secs(120),
        )
        .await
        .into_response()?
        .with_context(|| format!("JDT LS answered nothing to {command}"))
}

/// Has JDT LS build the project at `root`: the workspace, or its project alone for a module's
/// scope, every file when `full`. Whether it built without errors.
pub(crate) async fn build(
    server: &LanguageServer,
    root: &Path,
    scope: crate::build_view::Scope,
) -> Result<bool> {
    let response = if scope.module {
        let uri = lsp::Uri::from_file_path(root)
            .map_err(|()| anyhow::anyhow!("{root:?} isn't a folder path"))?;
        server
            .request::<BuildProjects>(
                BuildProjectsParams {
                    identifiers: vec![lsp::TextDocumentIdentifier { uri }],
                    is_full_build: scope.full,
                },
                Duration::from_secs(600),
            )
            .await
    } else {
        server
            .request::<BuildWorkspace>(scope.full, Duration::from_secs(600))
            .await
    };
    // JDT LS's BuildWorkspaceStatus: 0 failed, 1 succeeded, 2 with errors, 3 cancelled
    match response.into_response()? {
        1 => Ok(true),
        2 => Ok(false),
        3 => Err(anyhow::anyhow!(crate::build_view::CANCELLED)),
        _ => Err(anyhow::anyhow!("Build failed")),
    }
}

struct NoJdk;
struct BuildScriptsFound;
struct CannotDownload;

/// Where the project's Maven triggers are kept: the data folder, as IntelliJ keeps them in the
/// project's workspace file.
fn triggers_file(root: &Path) -> PathBuf {
    jdtls::project_data(root, "triggers").with_extension("json")
}

/// Maven's user settings file, `~/.m2/settings.xml`.
pub fn maven_settings() -> PathBuf {
    paths::home_dir().join(".m2").join("settings.xml")
}

/// IntelliJ's `Maven Settings.xml` file template.
pub const MAVEN_SETTINGS_TEMPLATE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<settings xmlns=\"http://maven.apache.org/SETTINGS/1.0.0\"\n          xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n          xsi:schemaLocation=\"http://maven.apache.org/SETTINGS/1.0.0 http://maven.apache.org/xsd/settings-1.0.0.xsd\">\n    \n</settings>\n";

/// JDT LS's `java/buildWorkspace`: builds the workspace, a full build when `true`.
enum BuildWorkspace {}

impl lsp::request::Request for BuildWorkspace {
    type Params = bool;
    type Result = i32;
    const METHOD: &'static str = "java/buildWorkspace";
}

/// JDT LS's `java/buildProjects`: builds the projects of the documents named.
enum BuildProjects {}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildProjectsParams {
    identifiers: Vec<lsp::TextDocumentIdentifier>,
    is_full_build: bool,
}

impl lsp::request::Request for BuildProjects {
    type Params = BuildProjectsParams;
    type Result = i32;
    const METHOD: &'static str = "java/buildProjects";
}

/// JDT LS's `java/projectConfigurationUpdate`: updates a project from its build file.
enum ProjectConfigurationUpdate {}

impl lsp::notification::Notification for ProjectConfigurationUpdate {
    type Params = lsp::TextDocumentIdentifier;
    const METHOD: &'static str = "java/projectConfigurationUpdate";
}

#[cfg(test)]
mod tests {
    use super::{JavaFolderRoles, Tool, first_java_file};
    use project_panel::FolderRoles as _;
    use std::path::Path;

    #[test]
    fn finds_a_main_source_deeper_than_the_first_scan() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let main = root.path().join("src/main/java/com/example/demo/app");
        let test = root.path().join("src/test/java/com/example");
        std::fs::create_dir_all(&main)?;
        std::fs::create_dir_all(&test)?;
        std::fs::write(test.join("ATest.java"), "")?;
        std::fs::write(main.join("App.java"), "")?;
        assert_eq!(first_java_file(root.path()), Some(main.join("App.java")));
        assert_eq!(first_java_file(&root.path().join("missing")), None);
        Ok(())
    }

    #[test]
    fn finds_each_build_tool_at_the_root() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        assert!(Tool::detect_all(root.path()).is_empty());
        std::fs::write(root.path().join("build.gradle.kts"), "")?;
        assert_eq!(Tool::detect_all(root.path()), [Tool::Gradle]);
        std::fs::write(root.path().join("pom.xml"), "")?;
        assert_eq!(Tool::detect_all(root.path()), [Tool::Maven, Tool::Gradle]);
        assert_eq!(Tool::detect(root.path()), Some(Tool::Maven));
        Ok(())
    }

    #[test]
    fn a_linked_projects_roots_and_packages_have_their_roles() {
        let roles = JavaFolderRoles::default();
        let root = Path::new("/work/fixture");
        assert_eq!(roles.icon(&root.join("src/main/java")), None);
        roles.link(root);
        assert_eq!(
            roles.icon(&root.join("src/main/java")).as_deref(),
            Some("icons/file_icons/source_root.svg")
        );
        assert_eq!(
            roles.icon(&root.join("src/test/java")).as_deref(),
            Some("icons/file_icons/test_root.svg")
        );
        assert_eq!(
            roles.icon(&root.join("src/main/java/com")).as_deref(),
            Some("icons/file_icons/java_package.svg")
        );
        assert!(roles.is_package(&root.join("src/main/java/com/example")));
        assert!(!roles.is_package(&root.join("src/main/java")));
        assert!(!roles.is_package(&root.join("src/main/resources/static")));
        assert_eq!(roles.icon(&root.join("src/main/resources/static")), None);
        assert_eq!(roles.icon(&root.join("src")), None);
    }
}
