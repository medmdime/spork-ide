//! Spork IDE: the roles a language's toolchain gives a project's folders, IntelliJ's source,
//! resource and test roots and its packages (`spork_java` sets them): their icons in the tree,
//! a package folded with its only child package, written with dots, as IntelliJ's Compact
//! Middle Packages shows it, and what the folder adds to New in its menu (Java Class); and the
//! folders shown after the project's, as IntelliJ's External Libraries, with their own names.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Global, SharedString, Window};

/// An item a folder's role puts in New in its menu: IntelliJ's Java Class (before File) and
/// package-info.java (after Directory) in a source folder.
pub struct NewItem {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
    /// After Directory rather than before File.
    pub after: bool,
}

pub trait FolderRoles: 'static + Send + Sync {
    /// The icon of the folder at `abs_path`, when it has a role.
    fn icon(&self, abs_path: &Path) -> Option<SharedString>;

    /// Whether the folder is a package: a folder inside a source root.
    fn is_package(&self, abs_path: &Path) -> bool;

    /// What New offers in the folder at `abs_path`.
    fn new_items(&self, _abs_path: &Path) -> Vec<NewItem> {
        Vec::new()
    }

    /// The roots of the invisible worktrees the tree shows after the project's folders (the
    /// app adds and holds them): IntelliJ's External Libraries.
    fn extra_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The name a row shows in place of its file name ("External Libraries", "< 17 >").
    fn label(&self, _abs_path: &Path) -> Option<SharedString> {
        None
    }
}

struct GlobalFolderRoles(Arc<dyn FolderRoles>);

impl Global for GlobalFolderRoles {}

/// Gives the project's folders their roles.
pub fn set_folder_roles(roles: Arc<dyn FolderRoles>, cx: &mut App) {
    cx.set_global(GlobalFolderRoles(roles));
}

pub(crate) fn folder_roles(cx: &App) -> Option<Arc<dyn FolderRoles>> {
    cx.try_global::<GlobalFolderRoles>()
        .map(|roles| roles.0.clone())
}
