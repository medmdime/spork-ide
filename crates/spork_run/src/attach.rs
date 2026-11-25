//! Attach to Process… (⌥⇧F5, `XDebugger.AttachToProcess`): IntelliJ's popup of the local
//! processes ("Attach To", `AttachToProcessAction`'s list popup: the four attached last under
//! Recent, then a group per debugger, each row "pid name", the speed search) and the debug session
//! attached to the one picked. A process goes to the adapter that attaches to it by its process
//! id: Python's to Debugpy, Node's to JavaScript, the others to CodeLLDB (or GDB without it).
// shortcut: IntelliJ 2025.2 shows a dialog by default (`debugger.attach.dialog.enabled`), with
// remote hosts, columns and a debugger filter; this is its popup, which the registry key brings
// back

use dap::DapRegistry;
use gpui::{AppContext as _, Context, Global, SharedString, TaskExt as _, Window};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
use task::{AttachRequest, DebugRequest, SharedTaskContext, ZedDebugConfig};
use util::ResultExt as _;
use workspace::Workspace;

use crate::list_popup::{ListPopup, Row};
use crate::run;

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &run::AttachToProcess, window, cx| {
        show(workspace, window, cx)
    });
}

#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    name: SharedString,
}

/// The processes attached to last, newest first, with their debuggers: IntelliJ's Recent group
/// keeps four.
#[derive(Default)]
struct Recent(Vec<(SharedString, Process)>);

impl Global for Recent {}

/// The adapter that attaches to a process by its name, of those registered.
fn debugger_for(name: &str, registered: &[SharedString]) -> Option<SharedString> {
    let name = name.to_lowercase();
    let wanted: &[&str] = if name.starts_with("python") {
        &["Debugpy"]
    } else if name == "node" || name.starts_with("node.") {
        &["JavaScript"]
    } else {
        &["CodeLLDB", "GDB"]
    };
    wanted
        .iter()
        .find_map(|wanted| registered.iter().find(|name| name.as_ref() == *wanted))
        .cloned()
}

fn processes() -> Vec<Process> {
    let refresh = RefreshKind::nothing().with_processes(
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::Never),
    );
    let own = std::process::id();
    let mut processes: Vec<Process> = System::new_with_specifics(refresh)
        .processes()
        .values()
        .filter(|process| process.pid().as_u32() != own)
        .map(|process| Process {
            pid: process.pid().as_u32(),
            name: process.name().to_string_lossy().into_owned().into(),
        })
        .collect();
    processes.sort_by_key(|process| process.pid);
    processes
}

fn show(_: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let listing = cx.background_spawn(async { processes() });
    cx.spawn_in(window, async move |workspace, cx| {
        let processes = listing.await;
        workspace.update_in(cx, |workspace, window, cx| {
            let registered: Vec<SharedString> = DapRegistry::global(cx)
                .enumerate_adapters::<Vec<_>>()
                .into_iter()
                .map(|name| name.0)
                .collect();
            let mut attachable: Vec<(SharedString, Process)> = processes
                .iter()
                .filter_map(|process| {
                    Some((debugger_for(&process.name, &registered)?, process.clone()))
                })
                .collect();
            // grouped by debugger, each group by process id
            attachable.sort_by(|(a, x), (b, y)| a.cmp(b).then(x.pid.cmp(&y.pid)));
            let recent: Vec<(SharedString, Process)> = cx
                .try_global::<Recent>()
                .map(|recent| recent.0.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, process)| processes.contains(process))
                .collect();
            let mut rows = Vec::new();
            let mut group = None;
            for (ix, (debugger, process)) in recent.iter().chain(&attachable).enumerate() {
                let mut row =
                    Row::new(format!("{} {}", process.pid, process.name)).image("run/debug");
                let title = match ix < recent.len() {
                    true => SharedString::from("Recent"),
                    false => debugger.clone(),
                };
                if group.as_ref() != Some(&title) {
                    row = row.group(title.clone());
                    group = Some(title);
                }
                rows.push(row);
            }
            let attachable: Vec<_> = recent.into_iter().chain(attachable).collect();
            let workspace_handle = cx.weak_entity();
            ListPopup::show(
                workspace,
                "Attach To",
                rows,
                "No processes to attach to",
                420.,
                move |ix, window, cx| {
                    if let Some((debugger, process)) = attachable.get(ix).cloned() {
                        workspace_handle
                            .update(cx, |workspace, cx| {
                                attach(workspace, debugger, process, window, cx)
                            })
                            .log_err();
                    }
                },
                window,
                cx,
            );
        })
    })
    .detach_and_log_err(cx);
}

/// Starts a debug session attached to `process`, named after it as IntelliJ's.
fn attach(
    workspace: &mut Workspace,
    debugger: SharedString,
    process: Process,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(adapter) = DapRegistry::global(cx).adapter(&debugger) else {
        return;
    };
    let recent = &mut cx.default_global::<Recent>().0;
    recent.retain(|(_, old)| old != &process);
    recent.insert(0, (debugger.clone(), process.clone()));
    recent.truncate(4);
    let config = ZedDebugConfig {
        label: format!("{} {}", process.pid, process.name).into(),
        adapter: debugger,
        request: DebugRequest::Attach(AttachRequest {
            process_id: Some(process.pid),
        }),
        stop_on_entry: None,
    };
    let worktree = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).id());
    cx.spawn_in(window, async move |workspace, cx| {
        let scenario = adapter.config_from_zed_format(config).await?;
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.start_debug_session(
                scenario,
                SharedTaskContext::default(),
                None,
                worktree,
                window,
                cx,
            )
        })
    })
    .detach_and_log_err(cx);
}

/// Whether a process can be attached to, for the tests.
#[cfg(test)]
fn attaches(name: &str, registered: &[&str]) -> Option<String> {
    let registered: Vec<SharedString> = registered.iter().map(|n| (*n).into()).collect();
    debugger_for(name, &registered).map(|name| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::attaches;

    #[test]
    fn each_process_goes_to_the_debugger_of_its_kind() {
        let all = ["CodeLLDB", "Debugpy", "JavaScript", "GDB"];
        assert_eq!(attaches("python3.12", &all).as_deref(), Some("Debugpy"));
        assert_eq!(attaches("node", &all).as_deref(), Some("JavaScript"));
        assert_eq!(attaches("fixture", &all).as_deref(), Some("CodeLLDB"));
        assert_eq!(attaches("fixture", &["GDB"]).as_deref(), Some("GDB"));
        assert_eq!(attaches("python3", &["CodeLLDB"]), None);
    }
}
