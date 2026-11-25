//! The navigation bar, which IntelliJ's New UI shows in the status bar (`StaticNavBarPanel`): the
//! active file's project › folders › file › symbols. A click on a folder or the file shows it in
//! the Project tool window, on a symbol the file's structure. Jump to Navigation Bar (⌘↑,
//! `ShowNavBarAction`) focuses it on the item before the last and lists what that item holds in
//! a popup (`NavBarVmImpl.selectTail`, `NavBarPopupVm`), the last selected: ← and → go from item
//! to item, ↓ or ⏎ open an item's popup; in the popup ⏎ opens a file, goes into a folder or to a
//! symbol, and ⎋ closes it, then leaves the bar.

use editor::{Anchor, Editor, SelectionEffects, ToPoint as _, scroll::Autoscroll};
use file_icons::FileIcons;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, ParentElement, Render,
    Role, SharedString, Styled, Subscription, WeakEntity, Window, anchored, deferred, px,
};
use project::ProjectPath;
use spork_workbench::popup;
use ui::prelude::*;
use util::ResultExt as _;
use util::rel_path::RelPath;
use workspace::{ItemHandle, StatusItemView, Workspace, item::ItemEvent};

/// The active item, re-rendered when it changes.
struct Watched {
    item: Box<dyn ItemHandle>,
    _subscription: Subscription,
}

fn watch(
    item: Option<&dyn ItemHandle>,
    window: &mut Window,
    cx: &mut Context<NavigationBar>,
) -> Option<Watched> {
    let item = item?;
    let this = cx.entity().downgrade();
    let subscription = match item.downcast::<Editor>() {
        Some(editor) => cx.observe_in(&editor, window, |_, _, _, cx| cx.notify()),
        None => item.subscribe_to_item_events(
            window,
            cx,
            Box::new(move |event, _, cx| {
                if let ItemEvent::UpdateBreadcrumbs | ItemEvent::UpdateTab = event {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            }),
        ),
    };
    Some(Watched {
        item: item.boxed_clone(),
        _subscription: subscription,
    })
}

/// One item of the bar: its text, its icon and what it stands for.
#[derive(Clone)]
struct Crumb {
    text: SharedString,
    icon: Option<CrumbIcon>,
    kind: Kind,
}

#[derive(Clone)]
enum CrumbIcon {
    Folder,
    File(SharedString),
}

#[derive(Clone)]
enum Kind {
    Folder(ProjectPath),
    File(ProjectPath),
    /// A symbol around the caret, by its place in the outline, when the bar knows it.
    Symbol(Option<usize>),
}

/// An item of a popup: a folder's file or folder, or a file's or symbol's member.
#[derive(Clone)]
struct Child {
    text: SharedString,
    icon: &'static str,
    target: Target,
    /// A member's place in the outline.
    outline: Option<usize>,
}

impl Child {
    /// Whether it's what `crumb` stands for: the item after a popup's own in the bar.
    fn is(&self, crumb: &Crumb) -> bool {
        match (&self.target, &crumb.kind) {
            (Target::Folder(a) | Target::File(a), Kind::Folder(b) | Kind::File(b)) => a == b,
            (Target::Symbol(_), Kind::Symbol(Some(ix))) => self.outline == Some(*ix),
            _ => false,
        }
    }
}

#[derive(Clone)]
enum Target {
    Folder(ProjectPath),
    File(ProjectPath),
    Symbol(Anchor),
}

struct Popup {
    /// The item of the bar it lists the children of.
    crumb: usize,
    children: Vec<Child>,
    selected: usize,
}

pub struct NavigationBar {
    workspace: WeakEntity<Workspace>,
    active: Option<Watched>,
    focus: FocusHandle,
    /// The item chosen from the keyboard while the bar has the focus.
    selected: Option<usize>,
    /// The items once a popup went into a folder, in place of the active file's.
    walked: Option<Vec<Crumb>>,
    popup: Option<Popup>,
    _focus_out: Option<Subscription>,
    _info: Subscription,
}

impl NavigationBar {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.weak_handle(),
            active: None,
            focus: cx.focus_handle(),
            selected: None,
            walked: None,
            popup: None,
            _focus_out: None,
            _info: cx.observe_global::<crate::StatusInfo>(|_, cx| cx.notify()),
        }
    }
}

impl Focusable for NavigationBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The active editor's outline and the items of it around the caret, outermost first.
fn outline_at_caret(
    editor: &Entity<Editor>,
    cx: &App,
) -> Option<(language::Outline<Anchor>, Vec<usize>)> {
    let editor = editor.read(cx);
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let outline = snapshot.outline(None)?;
    let caret = editor.selections.newest_anchor().head().to_point(&snapshot);
    let around = outline
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.range.start.to_point(&snapshot) <= caret
                && caret <= item.range.end.to_point(&snapshot)
        })
        .map(|(ix, _)| ix)
        .collect();
    Some((outline, around))
}

impl NavigationBar {
    fn active_crumbs(&self, cx: &App) -> Vec<Crumb> {
        let Some(active) = self.active.as_ref() else {
            return Vec::new();
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return Vec::new();
        };
        let project = workspace.read(cx).project().read(cx);
        let Some(ProjectPath { worktree_id, path }) = active.item.project_path(cx) else {
            return Vec::new();
        };
        let Some(worktree) = project.worktree_for_id(worktree_id, cx) else {
            return Vec::new();
        };
        let worktree = worktree.read(cx);
        let project_path = |path: &RelPath| ProjectPath {
            worktree_id,
            path: path.into(),
        };
        let mut crumbs = vec![Crumb {
            text: worktree.root_name_str().to_string().into(),
            icon: Some(CrumbIcon::Folder),
            kind: Kind::Folder(project_path(RelPath::empty())),
        }];
        let ancestors: Vec<_> = path.ancestors().collect();
        for ancestor in ancestors.into_iter().rev().skip(1) {
            let Some(name) = ancestor.file_name() else {
                continue;
            };
            let is_file = ancestor == path.as_ref();
            crumbs.push(Crumb {
                text: name.to_string().into(),
                icon: is_file
                    .then(|| FileIcons::get_icon(ancestor.as_std_path(), cx))
                    .flatten()
                    .map(CrumbIcon::File),
                kind: match is_file {
                    true => Kind::File(project_path(ancestor)),
                    false => Kind::Folder(project_path(ancestor)),
                },
            });
        }
        // the symbols at the caret: an editor's breadcrumbs after the file's own, each its outline
        // item when the two agree
        if let Some((segments, _)) = active.item.breadcrumbs(cx) {
            let symbols: Vec<_> = segments.into_iter().skip(1).collect();
            let around = active
                .item
                .downcast::<Editor>()
                .and_then(|editor| outline_at_caret(&editor, cx))
                .map(|(_, around)| around)
                .filter(|around| around.len() == symbols.len());
            crumbs.extend(symbols.into_iter().enumerate().map(|(ix, segment)| Crumb {
                text: segment.text,
                icon: None,
                kind: Kind::Symbol(around.as_ref().and_then(|around| around.get(ix).copied())),
            }));
        }
        crumbs
    }

    fn crumbs(&self, cx: &App) -> Vec<Crumb> {
        match &self.walked {
            Some(walked) => walked.clone(),
            None => self.active_crumbs(cx),
        }
    }

    /// What `crumb` holds: a folder's folders and files (folders first, as the Project tool
    /// window sorts them), the file's or a symbol's members.
    fn children(&self, crumb: &Crumb, cx: &App) -> Vec<Child> {
        match &crumb.kind {
            Kind::Folder(path) => {
                let Some(workspace) = self.workspace.upgrade() else {
                    return Vec::new();
                };
                let project = workspace.read(cx).project().read(cx);
                let Some(worktree) = project.worktree_for_id(path.worktree_id, cx) else {
                    return Vec::new();
                };
                let mut entries: Vec<_> = worktree
                    .read(cx)
                    .child_entries(&path.path)
                    .map(|entry| (entry.is_dir(), entry.path.clone()))
                    .collect();
                entries.sort_by_key(|(is_dir, path)| {
                    (!is_dir, path.file_name().unwrap_or_default().to_lowercase())
                });
                entries
                    .into_iter()
                    .map(|(is_dir, child)| {
                        let name = child.file_name().unwrap_or_default().to_owned();
                        let child_path = ProjectPath {
                            worktree_id: path.worktree_id,
                            path: child,
                        };
                        Child {
                            outline: None,
                            icon: match is_dir {
                                true => "nodes/folder",
                                false => spork_workbench::file_icon(&name),
                            },
                            text: name.into(),
                            target: match is_dir {
                                true => Target::Folder(child_path),
                                false => Target::File(child_path),
                            },
                        }
                    })
                    .collect()
            }
            Kind::File(_) => self.members(None, cx),
            Kind::Symbol(Some(ix)) => self.members(Some(*ix), cx),
            Kind::Symbol(None) => Vec::new(),
        }
    }

    /// The members of the outline item `parent`, or the file's top ones.
    fn members(&self, parent: Option<usize>, cx: &App) -> Vec<Child> {
        let Some(editor) = self
            .active
            .as_ref()
            .and_then(|active| active.item.downcast::<Editor>())
        else {
            return Vec::new();
        };
        let Some((outline, _)) = outline_at_caret(&editor, cx) else {
            return Vec::new();
        };
        let (depth, after, end) = match parent.and_then(|ix| Some((ix, outline.items.get(ix)?))) {
            Some((ix, item)) => (item.depth + 1, ix + 1, item.range.end),
            None => (0, 0, Anchor::Max),
        };
        let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
        let end = end.to_point(&snapshot);
        outline
            .items
            .iter()
            .enumerate()
            .skip(after)
            .take_while(|(_, item)| item.range.start.to_point(&snapshot) <= end)
            .filter(|(_, item)| item.depth == depth)
            .map(|(ix, item)| {
                let (name, kind) = crate::file_structure::split_name(&item.text, &item.name_ranges);
                Child {
                    text: name.into(),
                    icon: crate::file_structure::icon(&kind, item.depth),
                    target: Target::Symbol(item.selection_range.start),
                    outline: Some(ix),
                }
            })
            .collect()
    }

    /// Jump to Navigation Bar: the item before the last selected and its popup open
    /// (`show_popup`), or the last one (Select In ▸ Navigation Bar).
    fn select_tail(&mut self, show_popup: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.walked = None;
        self.popup = None;
        let crumbs = self.crumbs(cx);
        if crumbs.is_empty() {
            return;
        }
        let ix = (crumbs.len() - 1).saturating_sub(usize::from(show_popup));
        self.selected = Some(ix);
        window.focus(&self.focus, cx);
        if show_popup {
            self.open_popup(ix, cx);
        }
        cx.notify();
    }

    /// Lists `crumb`'s children, the next item of the bar selected among them; nothing for an
    /// empty one.
    fn open_popup(&mut self, crumb: usize, cx: &mut Context<Self>) {
        let crumbs = self.crumbs(cx);
        let children = crumbs
            .get(crumb)
            .map(|crumb| self.children(crumb, cx))
            .unwrap_or_default();
        if children.is_empty() {
            self.popup = None;
            return;
        }
        let selected = crumbs
            .get(crumb + 1)
            .and_then(|next| children.iter().position(|child| child.is(next)))
            .unwrap_or(0);
        self.popup = Some(Popup {
            crumb,
            children,
            selected,
        });
        cx.notify();
    }

    fn step_bar(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.crumbs(cx).len() as isize;
        let Some(selected) = self.selected.filter(|_| len > 0) else {
            return;
        };
        let ix = (selected as isize + step).rem_euclid(len) as usize;
        self.selected = Some(ix);
        if self.popup.is_some() {
            self.open_popup(ix, cx);
        }
        cx.notify();
    }

    fn step_popup(&mut self, step: isize, cx: &mut Context<Self>) {
        if let Some(popup) = self.popup.as_mut() {
            let len = popup.children.len() as isize;
            popup.selected = (popup.selected as isize + step).rem_euclid(len) as usize;
            cx.notify();
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.popup.as_ref() else {
            if let Some(selected) = self.selected {
                self.open_popup(selected, cx);
            }
            return;
        };
        let Some(child) = popup.children.get(popup.selected).cloned() else {
            return;
        };
        let crumb = popup.crumb;
        match child.target {
            Target::Folder(path) => {
                let mut walked = self.crumbs(cx);
                walked.truncate(crumb + 1);
                walked.push(Crumb {
                    text: child.text,
                    icon: Some(CrumbIcon::Folder),
                    kind: Kind::Folder(path),
                });
                let ix = walked.len() - 1;
                self.walked = Some(walked);
                self.selected = Some(ix);
                self.open_popup(ix, cx);
            }
            Target::File(path) => {
                self.leave(window, cx);
                self.workspace
                    .update(cx, |workspace, cx| {
                        workspace
                            .open_path(path, None, true, window, cx)
                            .detach_and_log_err(cx)
                    })
                    .log_err();
            }
            Target::Symbol(anchor) => {
                self.leave(window, cx);
                if let Some(editor) = self
                    .active
                    .as_ref()
                    .and_then(|active| active.item.downcast::<Editor>())
                {
                    editor.update(cx, |editor, cx| {
                        editor.change_selections(
                            SelectionEffects::scroll(Autoscroll::center()),
                            window,
                            cx,
                            |selections| selections.select_anchor_ranges([anchor..anchor]),
                        );
                        window.focus(&editor.focus_handle(cx), cx);
                    });
                }
            }
        }
        cx.notify();
    }

    /// ⎋: closes the popup, then leaves the bar for the editor.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popup.take().is_some() {
            cx.notify();
            return;
        }
        self.leave(window, cx);
    }

    /// Back to the active item, the bar as it was.
    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset(cx);
        if let Some(active) = self.active.as_ref() {
            window.focus(&active.item.item_focus_handle(cx), cx);
        }
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.walked = None;
        self.popup = None;
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.shift {
            return;
        }
        match event.keystroke.key.as_str() {
            "left" => self.step_bar(-1, cx),
            "right" => self.step_bar(1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_popup(&self, popup: &Popup, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = popup.children.iter().enumerate().map(|(ix, child)| {
            let selected = ix == popup.selected;
            popup::row(("navigation-bar-popup-row", ix), selected, cx)
                .role(Role::MenuItem)
                .aria_label(child.text.clone())
                .aria_selected(selected)
                .child(
                    spork_workbench::image(child.icon, cx)
                        .size(px(16.))
                        .flex_none(),
                )
                .child(div().min_w_0().truncate().child(child.text.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(popup) = this.popup.as_mut() {
                        popup.selected = ix;
                    }
                    this.confirm(window, cx);
                }))
        });
        let surface = popup::surface(cx)
            .id("navigation-bar-popup")
            .role(Role::Menu)
            .aria_label("Navigation Bar Popup")
            .min_w(px(160.))
            .max_w(px(480.))
            .max_h(px(400.))
            .py(px(6.))
            .child(
                v_flex()
                    .id("navigation-bar-popup-rows")
                    .overflow_y_scroll()
                    .children(rows),
            );
        div().absolute().top_0().left_0().child(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(spork_vcs::spork_look(cx).child(div().mb(px(4.)).child(surface))),
            )
            .with_priority(1),
        )
    }
}

impl Render for NavigationBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let crumbs = self.crumbs(cx);
        // Spork's status bar text: 13 px in the muted color, its parts 0.5 rem apart
        let colors = cx.theme().colors();
        let (hover, selection) = (colors.ghost_element_hover, colors.ghost_element_selected);
        let muted =
            spork_workbench::Tokens::for_appearance(!cx.theme().appearance().is_light()).text_muted;
        h_flex()
            .id("navigation-path")
            .role(Role::Toolbar)
            .aria_label("Navigation Bar")
            .min_w_0()
            .gap_2()
            .overflow_x_hidden()
            .track_focus(&self.focus)
            .key_context("NavigationBar menu")
            .on_action(cx.listener(|this, _: &menu::SelectNext, window, cx| {
                match this.popup.is_some() {
                    true => this.step_popup(1, cx),
                    false => this.confirm(window, cx),
                }
            }))
            .on_action(cx.listener(|this, _: &menu::SelectPrevious, _, cx| this.step_popup(-1, cx)))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, window, cx| this.cancel(window, cx)))
            .on_key_down(cx.listener(Self::key_down))
            .children(crumbs.into_iter().enumerate().map(|(ix, crumb)| {
                let text = crumb.text.clone();
                let workspace = self.workspace.clone();
                let selected = self.selected == Some(ix);
                let popup = self
                    .popup
                    .as_ref()
                    .filter(|popup| popup.crumb == ix)
                    .map(|popup| self.render_popup(popup, cx));
                h_flex()
                    .flex_none()
                    .relative()
                    .gap_2()
                    .when(ix > 0, |this| {
                        this.child(
                            Icon::new(IconName::ChevronRight)
                                .size(IconSize::XSmall)
                                .color(Color::Custom(muted)),
                        )
                    })
                    .children(popup)
                    .child(
                        h_flex()
                            .id(("navigation-path-crumb", ix))
                            .role(Role::Button)
                            .aria_label(text.clone())
                            .gap_1()
                            .rounded_sm()
                            .cursor_pointer()
                            // a press leaves the focus where it is, as a button's does: a
                            // symbol's crumb acts on the editor
                            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                                window.prevent_default()
                            })
                            .map(|this| match selected {
                                true => this.bg(selection),
                                false => this.hover(move |this| this.bg(hover)),
                            })
                            .children(crumb.icon.clone().map(|icon| {
                                match icon {
                                    CrumbIcon::Folder => Icon::new(IconName::Folder),
                                    CrumbIcon::File(path) => Icon::from_path(path),
                                }
                                .size(IconSize::Small)
                                .color(Color::Custom(muted))
                            }))
                            .child(Label::new(text).color(match selected {
                                true => Color::Default,
                                false => Color::Custom(muted),
                            }))
                            .on_click(move |_, window, cx| match &crumb.kind {
                                Kind::Folder(path) | Kind::File(path) => {
                                    if let Some(workspace) = workspace.upgrade() {
                                        workspace.update(cx, |workspace, cx| {
                                            let entry = workspace
                                                .project()
                                                .read(cx)
                                                .entry_for_path(path, cx)
                                                .map(|entry| entry.id);
                                            if let Some(entry) = entry {
                                                workspace.project().update(cx, |_, cx| {
                                                    cx.emit(project::Event::RevealInProjectPanel(
                                                        entry,
                                                    ))
                                                })
                                            }
                                        });
                                    }
                                }
                                Kind::Symbol(_) => window.dispatch_action(
                                    Box::new(zed_actions::outline::ToggleOutline),
                                    cx,
                                ),
                            }),
                    )
            }))
            // none before `init` (zed's tests build the status bar without it)
            .children(
                cx.try_global::<crate::StatusInfo>()
                    .and_then(|status| status.0.clone())
                    .map(|info| {
                        div()
                            .id("status-info")
                            .role(Role::Label)
                            .aria_label(info.clone())
                            .flex_none()
                            .ml_4()
                            .child(Label::new(info).size(LabelSize::Small).color(Color::Muted))
                    }),
            )
    }
}

impl StatusItemView for NavigationBar {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // the status bar tells it again as the item changes and the focus moves between panes:
        // the bar shows the change and keeps its keyboard state for the same item
        let same = match (&self.active, item) {
            (Some(active), Some(item)) => active.item.item_id() == item.item_id(),
            (None, None) => true,
            _ => false,
        };
        if same {
            cx.notify();
            return;
        }
        self.active = watch(item, window, cx);
        if self._focus_out.is_none() {
            self._focus_out = Some(cx.on_focus_out(&self.focus, window, |this, _, window, cx| {
                // the window coming to the front tells of a focus change too
                if !this.focus.contains_focused(window, cx) {
                    this.reset(cx)
                }
            }));
        }
        self.reset(cx);
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}

/// Jump to Navigation Bar (`show_popup`), and Select In ▸ Navigation Bar.
pub(crate) fn jump(
    workspace: &Entity<Workspace>,
    show_popup: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let bar = workspace
        .read(cx)
        .status_bar()
        .read(cx)
        .item_of_type::<NavigationBar>();
    if let Some(bar) = bar {
        bar.update(cx, |bar, cx| bar.select_tail(show_popup, window, cx));
    }
}
