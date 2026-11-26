//! Build, Execution, Deployment › Coverage (IntelliJ's `CoverageOptionsConfigurable` and Java's
//! `JavaCoverageOptionsProvider`), kept in the key-value store as one JSON object and read by
//! `spork_coverage` (what a new run's coverage does, the Coverage tool window, the Project tool
//! window) and `spork_java` (the agent's options).

use gpui::{App, AppContext as _, Global, TaskExt as _};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// The options, IntelliJ's defaults when unset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoverageSettings {
    /// When new coverage is gathered.
    pub when_gathered: WhenGathered,
    /// Activate Coverage View.
    pub activate_view: bool,
    /// Show coverage in the project view.
    pub show_in_project_view: bool,
    /// Java Coverage: Branch coverage.
    pub branch_coverage: bool,
    /// Java Coverage: Track per test coverage.
    pub track_per_test: bool,
    /// Java Coverage: Collect coverage in test folders.
    pub test_folders: bool,
    /// Java Coverage: Exclude annotations, each a pattern with whether it is on.
    pub exclude_annotations: Vec<(String, bool)>,
}

/// What a run's new coverage does to the coverage shown (IntelliJ's `CoverageOptionsProvider`
/// option): ask, or each of the dialog's answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhenGathered {
    #[default]
    Ask,
    DoNotApply,
    Replace,
    Add,
}

impl Default for CoverageSettings {
    fn default() -> Self {
        Self {
            when_gathered: WhenGathered::Ask,
            activate_view: true,
            show_in_project_view: true,
            branch_coverage: true,
            track_per_test: false,
            test_folders: false,
            exclude_annotations: vec![("*Generated*".into(), true)],
        }
    }
}

struct CoverageSettingsGlobal(CoverageSettings);

impl Global for CoverageSettingsGlobal {}

/// The key the options are kept under.
const KEY: &str = "spork_coverage_settings";

/// Reads the options from the store, once.
pub(crate) fn init(cx: &mut App) {
    let settings = db::kvp::KeyValueStore::global(cx)
        .read_kvp(KEY)
        .log_err()
        .flatten()
        .and_then(|value| serde_json::from_str(&value).log_err())
        .unwrap_or_default();
    cx.set_global(CoverageSettingsGlobal(settings));
}

/// The options.
pub fn settings(cx: &App) -> CoverageSettings {
    cx.try_global::<CoverageSettingsGlobal>()
        .map(|CoverageSettingsGlobal(settings)| settings.clone())
        .unwrap_or_default()
}

/// Keeps the options, for the next runs.
pub fn set_settings(settings: CoverageSettings, cx: &mut App) -> anyhow::Result<()> {
    let value = serde_json::to_string(&settings)?;
    cx.set_global(CoverageSettingsGlobal(settings));
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(KEY.into(), value).await })
        .detach_and_log_err(cx);
    Ok(())
}
