mod event_coalescer;

use clock::SystemClock;
use gpui::App;
use parking_lot::Mutex;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;
use std::{env, sync::Arc, time::Duration};
use worktree::{UpdatedEntriesSet, WorktreeId};

use self::event_coalescer::EventCoalescer;

/// What Zed's telemetry knew of this installation and its user, kept for the code that still asks:
/// Spork IDE sends no telemetry, `telemetry::event!` drops every event.
pub struct Telemetry {
    state: Arc<Mutex<TelemetryState>>,
}

struct TelemetryState {
    system_id: Option<Arc<str>>,  // Per system
    metrics_id: Option<Arc<str>>, // Per logged-in user
    is_staff: Option<bool>,
    event_coalescer: EventCoalescer,
    worktrees_with_project_type_events_sent: HashSet<WorktreeId>,
}

/// Minidumps are written to the logs folder only when asked for; nothing uploads them.
pub fn should_install_crash_handler() -> bool {
    matches!(
        env::var("ZED_GENERATE_MINIDUMPS").as_deref(),
        Ok("true" | "1")
    )
}

static DOTNET_PROJECT_FILES_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(global\.json|Directory\.Build\.props|.*\.(csproj|fsproj|vbproj|sln))$").unwrap()
});

/// Returns the OS name, including the display server the platform connected to.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "freebsd")),
    expect(unused_variables)
)]
pub fn os_name(cx: &App) -> String {
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        format!("Linux {}", cx.compositor_name())
    }
    #[cfg(target_os = "freebsd")]
    {
        format!("FreeBSD {}", cx.compositor_name())
    }

    #[cfg(target_os = "windows")]
    {
        "Windows".to_string()
    }
}

/// Returns the OS name, guessing the display server from the environment.
///
/// Prefer [`os_name`] once an [`App`] exists; the platform may connect to a
/// different display server than the environment suggests.
pub fn guessed_os_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        format!("Linux {}", gpui::guess_compositor())
    }
    #[cfg(target_os = "freebsd")]
    {
        format!("FreeBSD {}", gpui::guess_compositor())
    }

    #[cfg(target_os = "windows")]
    {
        "Windows".to_string()
    }
}

/// Note: This might do blocking IO! Only call from background threads
pub fn os_version() -> String {
    cfg_select! {
       feature = "test-support" => {
           // MacOS branch in particular is quite slow, hence we ought to "avoid" it in tests.
           "test binary".to_owned()
       }
       target_os = "macos" => {
           static MACOS_VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
               Regex::new(r"(\s*\(Build [^)]*[0-9]\))").unwrap()
           });
           use objc2_foundation::NSProcessInfo;
           let process_info = NSProcessInfo::processInfo();
           let version_nsstring = process_info.operatingSystemVersionString();
           // "Version 15.6.1 (Build 24G90)" -> "15.6.1 (Build 24G90)"
           let version_string = version_nsstring.to_string().replace("Version ", "");
           // "15.6.1 (Build 24G90)" -> "15.6.1"
           // "26.0.0 (Build 25A5349a)" -> unchanged (Beta or Rapid Security Response; ends with letter)
           MACOS_VERSION_REGEX
               .replace_all(&version_string, "")
               .to_string()
       }
       any(target_os = "linux", target_os = "freebsd") => {
           use std::path::Path;

           let content = if let Ok(file) = std::fs::read_to_string(&Path::new("/etc/os-release")) {
               file
           } else if let Ok(file) = std::fs::read_to_string(&Path::new("/usr/lib/os-release")) {
               file
           } else if let Ok(file) = std::fs::read_to_string(&Path::new("/var/run/os-release")) {
               file
           } else {
               log::error!(
                   "Failed to load /etc/os-release, /usr/lib/os-release, or /var/run/os-release"
               );
               "".to_string()
           };
           util::parse_os_release(&content).unwrap_or_else(|| "unknown".to_string())
       }
       target_os = "windows" => {
           let mut info = unsafe { std::mem::zeroed() };
           let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut info) };
           if status.is_ok() {
               semver::Version::new(
                   info.dwMajorVersion as _,
                   info.dwMinorVersion as _,
                   info.dwBuildNumber as _,
               )
               .to_string()
           } else {
               "unknown".to_string()
           }
       }
    }
}

impl Telemetry {
    pub fn new(clock: Arc<dyn SystemClock>) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(TelemetryState {
                system_id: None,
                metrics_id: None,
                is_staff: None,
                event_coalescer: EventCoalescer::new(clock),
                worktrees_with_project_type_events_sent: HashSet::new(),
            })),
        })
    }

    pub fn start(self: &Arc<Self>, system_id: Option<String>) {
        self.state.lock().system_id = system_id.map(|id| id.into());
    }

    pub fn set_authenticated_user_info(
        self: &Arc<Self>,
        metrics_id: Option<String>,
        is_staff: bool,
    ) {
        let mut state = self.state.lock();
        state.metrics_id = metrics_id.map(|id| id.into());
        state.is_staff = Some(is_staff);
    }

    pub fn log_edit_event(self: &Arc<Self>, environment: &'static str, is_via_ssh: bool) {
        static LAST_EVENT_TIME: Mutex<Option<Instant>> = Mutex::new(None);

        let mut state = self.state.lock();
        let period_data = state.event_coalescer.log_event(environment);
        drop(state);

        if let Some(mut last_event) = LAST_EVENT_TIME.try_lock() {
            let current_time = std::time::Instant::now();
            let last_time = last_event.get_or_insert(current_time);

            if current_time.duration_since(*last_time) > Duration::from_secs(60 * 10) {
                *last_time = current_time;
            } else {
                return;
            }

            if let Some((start, end, environment)) = period_data {
                let duration = end
                    .saturating_duration_since(start)
                    .min(Duration::from_secs(60 * 60 * 24))
                    .as_millis() as i64;

                telemetry::event!(
                    "Editor Edited",
                    duration = duration,
                    environment = environment,
                    is_via_ssh = is_via_ssh
                );
            }
        }
    }

    pub fn report_discovered_project_type_events(
        self: &Arc<Self>,
        worktree_id: WorktreeId,
        updated_entries_set: &UpdatedEntriesSet,
    ) {
        let Some(project_types) = self.detect_project_types(worktree_id, updated_entries_set)
        else {
            return;
        };

        for project_type in project_types {
            telemetry::event!("Project Opened", project_type = project_type);
        }
    }

    fn detect_project_types(
        self: &Arc<Self>,
        worktree_id: WorktreeId,
        updated_entries_set: &UpdatedEntriesSet,
    ) -> Option<Vec<String>> {
        let mut state = self.state.lock();

        if state
            .worktrees_with_project_type_events_sent
            .contains(&worktree_id)
        {
            return None;
        }

        let mut project_types: HashSet<&str> = HashSet::new();

        for (path, _, _) in updated_entries_set.iter() {
            let Some(file_name) = path.file_name() else {
                continue;
            };

            let project_type = match file_name {
                "pnpm-lock.yaml" => Some("pnpm"),
                "yarn.lock" => Some("yarn"),
                "package.json" => Some("node"),
                _ if DOTNET_PROJECT_FILES_REGEX.is_match(file_name) => Some("dotnet"),
                _ => None,
            };

            if let Some(project_type) = project_type {
                project_types.insert(project_type);
            };
        }

        if !project_types.is_empty() {
            state
                .worktrees_with_project_type_events_sent
                .insert(worktree_id);
        }

        let mut project_types: Vec<_> = project_types.into_iter().map(String::from).collect();
        project_types.sort();
        Some(project_types)
    }

    pub fn metrics_id(self: &Arc<Self>) -> Option<Arc<str>> {
        self.state.lock().metrics_id.clone()
    }

    pub fn system_id(self: &Arc<Self>) -> Option<Arc<str>> {
        self.state.lock().system_id.clone()
    }

    pub fn is_staff(self: &Arc<Self>) -> Option<bool> {
        self.state.lock().is_staff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clock::FakeSystemClock;

    use util::rel_path::RelPath;
    use worktree::{PathChange, ProjectEntryId, WorktreeId};

    #[test]
    fn test_project_discovery_does_not_double_report() {
        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = Telemetry::new(clock);
        let worktree_id = 1;

        // Scan of empty worktree finds nothing
        test_project_discovery_helper(telemetry.clone(), vec![], Some(vec![]), worktree_id);

        // Files added, second scan of worktree 1 finds project type
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["package.json"],
            Some(vec!["node"]),
            worktree_id,
        );

        // Third scan of worktree does not double report, as we already reported
        test_project_discovery_helper(telemetry, vec!["package.json"], None, worktree_id);
    }

    #[test]
    fn test_pnpm_project_discovery() {
        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = Telemetry::new(clock);

        test_project_discovery_helper(
            telemetry,
            vec!["package.json", "pnpm-lock.yaml"],
            Some(vec!["node", "pnpm"]),
            1,
        );
    }

    #[test]
    fn test_yarn_project_discovery() {
        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = Telemetry::new(clock);

        test_project_discovery_helper(
            telemetry,
            vec!["package.json", "yarn.lock"],
            Some(vec!["node", "yarn"]),
            1,
        );
    }

    #[test]
    fn test_dotnet_project_discovery() {
        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = Telemetry::new(clock);

        // Using different worktrees, as production code blocks from reporting a
        // project type for the same worktree multiple times

        test_project_discovery_helper(
            telemetry.clone(),
            vec!["global.json"],
            Some(vec!["dotnet"]),
            1,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["Directory.Build.props"],
            Some(vec!["dotnet"]),
            2,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.csproj"],
            Some(vec!["dotnet"]),
            3,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.fsproj"],
            Some(vec!["dotnet"]),
            4,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.vbproj"],
            Some(vec!["dotnet"]),
            5,
        );
        test_project_discovery_helper(telemetry.clone(), vec!["file.sln"], Some(vec!["dotnet"]), 6);

        // Each worktree should only send a single project type event, even when
        // encountering multiple files associated with that project type
        test_project_discovery_helper(
            telemetry,
            vec!["global.json", "Directory.Build.props"],
            Some(vec!["dotnet"]),
            7,
        );
    }

    fn test_project_discovery_helper(
        telemetry: Arc<Telemetry>,
        file_paths: Vec<&str>,
        expected_project_types: Option<Vec<&str>>,
        worktree_id_num: usize,
    ) {
        let worktree_id = WorktreeId::from_usize(worktree_id_num);
        let entries: Vec<_> = file_paths
            .into_iter()
            .enumerate()
            .filter_map(|(i, path)| {
                Some((
                    Arc::from(RelPath::from_unix_str(path).ok()?),
                    ProjectEntryId::from_proto(i as u64 + 1),
                    PathChange::Added,
                ))
            })
            .collect();
        let updated_entries: UpdatedEntriesSet = Arc::from(entries.as_slice());

        let detected_project_types = telemetry.detect_project_types(worktree_id, &updated_entries);

        let expected_project_types =
            expected_project_types.map(|types| types.iter().map(|&t| t.to_string()).collect());

        assert_eq!(detected_project_types, expected_project_types);
    }
}
