//! A workspace's Kotlin: detected in the Java project's folder (a build that declares Kotlin, or
//! Kotlin files), and the Kotlin language server started once the project is loaded (IntelliJ's
//! Load brings Kotlin in with the build's import), on one of its Kotlin files. Also the server's
//! expiry: a build that says it has expired is set aside, a balloon says so, and the server is
//! started again on the newest build.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use collections::{HashMap, HashSet};
use futures::StreamExt as _;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, Task, WeakEntity};
use language::{BinaryStatus, Buffer, LanguageRegistry};
use lsp::{LanguageServer, LanguageServerSelector};
use project::Project;
use project::lsp_store::OpenLspBufferHandle;
use spork_java::{JavaProject, Tool};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::notifications::{NotificationId, simple_message_notification::MessageNotification};

use crate::kotlin_lsp::{self, SERVER_NAME};

/// The Kotlin of each workspace, by the workspace's entity.
#[derive(Default)]
struct Projects(HashMap<EntityId, Entity<KotlinProject>>);

impl Global for Projects {}

pub struct KotlinProject {
    workspace: WeakEntity<Workspace>,
    java: Entity<JavaProject>,
    project: Entity<Project>,
    /// The build declares Kotlin, or the project has Kotlin files.
    pub detected: bool,
    /// A Kotlin file opened with the language servers, which keeps the server running.
    language_server: Option<(Entity<Buffer>, OpenLspBufferHandle)>,
    /// The build files were read for Kotlin.
    read: bool,
    started: bool,
    /// The server has imported the project (its `intellij/workspaceImportState` said so).
    pub imported: bool,
    _subscriptions: Vec<gpui::Subscription>,
    /// The server's import notifications, followed while it runs.
    _import_state: Option<lsp::Subscription>,
    start: Task<()>,
}

/// kotlin-lsp's `intellij/workspaceImportState`: `FINISHED` once its import has ended.
enum WorkspaceImportState {}

impl lsp::notification::Notification for WorkspaceImportState {
    type Params = serde_json::Value;
    const METHOD: &'static str = "intellij/workspaceImportState";
}

pub(crate) fn register(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some(java) = JavaProject::of(workspace, cx) else {
        return;
    };
    let id = cx.entity().entity_id();
    let workspace_handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let lsp_store = project.read(cx).lsp_store();
    let kotlin = cx.new(|cx| {
        let _subscriptions = vec![
            cx.observe(&java, |this: &mut KotlinProject, _, cx| {
                this.java_changed(cx)
            }),
            cx.subscribe(&lsp_store, |this: &mut KotlinProject, _, event, cx| {
                if let project::LspStoreEvent::LanguageServerAdded(id, name, _) = event
                    && *name == SERVER_NAME
                {
                    this.follow_import(*id, cx);
                }
            }),
        ];
        let mut this = KotlinProject {
            workspace: workspace_handle,
            java,
            project,
            detected: false,
            language_server: None,
            read: false,
            started: false,
            imported: false,
            _subscriptions,
            _import_state: None,
            start: Task::ready(()),
        };
        this.java_changed(cx);
        this
    });
    cx.default_global::<Projects>().0.insert(id, kotlin);
    cx.on_release(move |_, cx| {
        cx.default_global::<Projects>().0.remove(&id);
    })
    .detach();
}

/// Whether a project's build files declare Kotlin: Gradle's Kotlin plugin (`kotlin("jvm")`,
/// `org.jetbrains.kotlin.*`) or Maven's `kotlin-maven-plugin`.
pub(crate) fn declares_kotlin(root: &Path) -> bool {
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    let gradle = ["build.gradle.kts", "build.gradle"].map(read).join("\n");
    gradle.contains("org.jetbrains.kotlin")
        || gradle.contains("kotlin(\"")
        || read("pom.xml").contains("kotlin-maven-plugin")
}

impl KotlinProject {
    /// The Kotlin of the workspace.
    pub fn of(workspace: &Workspace, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Projects>()?
            .0
            .get(&workspace.weak_handle().entity_id())
            .cloned()
    }

    /// The Kotlin project whose folder holds `path`.
    pub fn containing(path: &Path, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Projects>()?
            .0
            .values()
            .find(|kotlin| {
                kotlin
                    .read(cx)
                    .root(cx)
                    .is_some_and(|root| path.starts_with(root))
            })
            .cloned()
    }

    /// The first Kotlin project open.
    pub(crate) fn first(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Projects>()?.0.values().next().cloned()
    }

    pub(crate) fn root(&self, cx: &App) -> Option<PathBuf> {
        self.java.read(cx).root.clone()
    }

    pub fn tool(&self, cx: &App) -> Option<Tool> {
        self.java.read(cx).tool
    }

    /// Follows the Java project: its folder read, Kotlin is detected; it loaded, the server
    /// starts.
    fn java_changed(&mut self, cx: &mut Context<Self>) {
        let (root, linked) = {
            let java = self.java.read(cx);
            (java.root.clone(), java.linked)
        };
        let Some(root) = root else { return };
        if !self.read {
            self.read = true;
            if declares_kotlin(&root) {
                self.detected = true;
                cx.notify();
            }
        }
        if linked && !self.started {
            self.started = true;
            self.start(cx);
        }
    }

    /// Starts the server on one of the project's Kotlin files, a main one first, or, in a project
    /// whose build declares Kotlin but has no Kotlin file yet, on its Kotlin build script.
    // shortcut: a Maven or Groovy Gradle project without a Kotlin file isn't analysed until one is
    // opened, as Java's; start the server on its build file once the language servers can start
    // without a Kotlin buffer
    fn start(&mut self, cx: &mut Context<Self>) {
        let project = self.project.clone();
        let declared = self.detected;
        let Some(worktree) = project.read(cx).visible_worktrees(cx).next() else {
            return;
        };
        let scanned = worktree
            .read(cx)
            .as_local()
            .map(|local| local.scan_complete());
        self.start = cx.spawn(async move |this, cx| {
            if let Some(scanned) = scanned {
                scanned.await;
            }
            let Some(kotlin_file) = worktree.read_with(cx, |worktree, _| {
                let kotlin = |entry: &&project::Entry| {
                    entry.is_file() && entry.path.extension() == Some("kt")
                };
                let script = || {
                    ["build.gradle.kts", "settings.gradle.kts"]
                        .into_iter()
                        .map(|name| worktree.abs_path().join(name))
                        .find(|path| declared && path.is_file())
                };
                worktree
                    .entries(false, 0)
                    .filter(kotlin)
                    .find(|entry| !entry.path.as_unix_str().contains("/test/"))
                    .or_else(|| worktree.entries(false, 0).find(kotlin))
                    .map(|entry| worktree.abs_path().join(entry.path.as_std_path()))
                    .or_else(script)
            }) else {
                return;
            };
            this.update(cx, |this, cx| {
                this.detected = true;
                cx.notify();
            })
            .ok();
            let opened = project
                .update(cx, |project, cx| project.open_local_buffer(kotlin_file, cx))
                .await;
            match opened {
                Ok(buffer) => {
                    let handle = project.update(cx, |project, cx| {
                        project.register_buffer_with_language_servers(&buffer, cx)
                    });
                    this.update(cx, |this, cx| {
                        this.language_server = Some((buffer, handle));
                        cx.notify();
                    })
                    .ok();
                }
                Err(error) => log::error!("starting the Kotlin language server: {error:#}"),
            }
        });
    }

    /// Follows a server that started: its import's end.
    fn follow_import(&mut self, id: lsp::LanguageServerId, cx: &mut Context<Self>) {
        let Some(server) = self
            .project
            .read(cx)
            .lsp_store()
            .read(cx)
            .language_server_for_id(id)
        else {
            return;
        };
        self.imported = false;
        let this = cx.entity().downgrade();
        self._import_state = Some(server.on_notification::<WorkspaceImportState, _>(
            move |params, cx| {
                if params["phase"] == "FINISHED" {
                    this.update(cx, |this, cx| {
                        this.imported = true;
                        cx.notify();
                    })
                    .ok();
                }
            },
        ));
    }

    /// The project's Kotlin language server, once it runs.
    pub fn language_server(&self, cx: &App) -> Option<Arc<LanguageServer>> {
        let lsp_store = self.project.read(cx).lsp_store();
        let lsp_store = lsp_store.read(cx);
        let id = lsp_store
            .language_server_statuses()
            .find(|(_, status)| status.name == SERVER_NAME)
            .map(|(id, _)| id)?;
        lsp_store.language_server_for_id(id)
    }

    /// Whether the server is still importing or indexing: its import not over, work in
    /// progress, or its diagnostics.
    pub fn language_server_busy(&self, cx: &App) -> Option<bool> {
        self.project
            .read(cx)
            .language_server_statuses(cx)
            .find(|(_, status)| status.name == SERVER_NAME)
            .map(|(_, status)| {
                !self.imported
                    || !status.pending_work.is_empty()
                    || status.has_pending_diagnostic_updates
            })
    }

    /// The server's build expired: IntelliJ's balloon, then the server again, which fetches the
    /// newest build.
    fn restart_after_expiry(&mut self, cx: &mut Context<Self>) {
        let Some((buffer, _)) = &self.language_server else {
            return;
        };
        let buffer = buffer.clone();
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.show_notification(NotificationId::unique::<Expired>(), cx, |cx| {
                    cx.new(|cx| {
                        MessageNotification::new(
                            "This build has expired. Spork IDE is downloading the newest one.",
                            cx,
                        )
                        .with_title(kotlin_lsp::DISPLAY_NAME)
                    })
                })
            });
        }
        self.project.update(cx, |project, cx| {
            project.restart_language_servers_for_buffers(
                vec![buffer],
                HashSet::from_iter([LanguageServerSelector::Name(SERVER_NAME)]),
                cx,
            )
        });
    }
}

struct Expired;

/// Watches the server's starts: one that failed saying its build has expired sets that build
/// aside, and each project's server starts again.
pub(crate) fn watch_expiry(languages: &Arc<LanguageRegistry>, cx: &mut App) {
    let (mut statuses, subscription) = languages.language_server_binary_statuses();
    cx.spawn(async move |cx| {
        let _subscription = subscription;
        while let Some((_, name, status)) = statuses.next().await {
            let BinaryStatus::Failed { error } = status else {
                continue;
            };
            if name != SERVER_NAME || !says_expired(&error) {
                continue;
            }
            let container = kotlin_lsp::container();
            if let Some((version, _)) = kotlin_lsp::newest_installed(&container) {
                log::warn!("the Kotlin language server {version} has expired");
                kotlin_lsp::mark_expired(&container, &version).log_err();
            }
            cx.update(|cx| {
                let projects: Vec<Entity<KotlinProject>> = cx
                    .try_global::<Projects>()
                    .map(|projects| projects.0.values().cloned().collect())
                    .unwrap_or_default();
                for kotlin in projects {
                    kotlin.update(cx, |kotlin, cx| kotlin.restart_after_expiry(cx));
                }
            });
        }
    })
    .detach();
}

/// Whether a failed start's error has the server saying its build expired: its own words, on the
/// stderr the error carries.
fn says_expired(error: &str) -> bool {
    error
        .split_once("-- stderr --")
        .is_some_and(|(_, stderr)| stderr.contains(kotlin_lsp::EXPIRED))
}

/// Runs one of the server's commands.
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
        .with_context(|| {
            format!(
                "The {} answered nothing to {command}",
                kotlin_lsp::DISPLAY_NAME
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{declares_kotlin, says_expired};

    #[test]
    fn a_start_that_failed_on_an_expired_build() {
        assert!(says_expired(
            "server shut down\n-- stderr --\nThis build of kotlin-server has expired.\nThe IDE will now close."
        ));
        assert!(!says_expired(
            "server shut down\n-- stderr --\nLog file: /tmp/x"
        ));
        // the IDE's own message about the newest build isn't the server's
        assert!(!says_expired("No newer build; the newest has expired"));
    }

    #[test]
    fn a_build_declares_kotlin() {
        let root = tempfile::tempdir().expect("a temporary folder");
        assert!(!declares_kotlin(root.path()));
        std::fs::write(
            root.path().join("build.gradle.kts"),
            "plugins {\n    kotlin(\"jvm\") version \"2.4.10\"\n}\n",
        )
        .expect("a build script");
        assert!(declares_kotlin(root.path()));
        std::fs::remove_file(root.path().join("build.gradle.kts")).expect("removed");
        std::fs::write(
            root.path().join("pom.xml"),
            "<artifactId>kotlin-maven-plugin</artifactId>",
        )
        .expect("a POM");
        assert!(declares_kotlin(root.path()));
    }
}
