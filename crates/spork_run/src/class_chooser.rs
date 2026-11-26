//! IntelliJ's class chooser (`TreeClassChooserDialog`) as the Breakpoints dialog's + ▸ Java
//! Exception Breakpoints opens it ("Enter Exception Class"): Search by Name, the classes whose name
//! holds the text typed (the language's search, JDT LS's for Java), each with its package in grey;
//! OK or ⏎ picks the selected one, as a double click does; Cancel or ⎋ closes it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use project::Project;
use spork_workbench::Tokens;
use util::ResultExt as _;

/// A language's classes for a chooser: the JVM names of the classes whose name holds the text.
pub type ClassSearch = Rc<dyn Fn(&Entity<Project>, String, &mut App) -> Task<Vec<String>>>;

/// Opens the chooser titled `title` over `project`'s classes; `chosen` takes the class picked.
pub(crate) fn open(
    title: &'static str,
    project: Entity<Project>,
    search: ClassSearch,
    chosen: Rc<dyn Fn(String, &mut App)>,
    cx: &mut App,
) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, size(px(540.), px(420.)), cx)),
        window_min_size: Some(size(px(400.), px(300.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    cx.defer(move |cx| {
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            crate::configurations_dialog::follow_ide_theme(window, cx);
            cx.new(|cx| ClassChooser::new(title, project, search, chosen, window, cx))
        });
        // opened from the Breakpoints dialog's menu, it takes the keys that follow
        if let Some((handle, _)) = opened.log_err() {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .log_err();
        }
    });
}

struct ClassChooser {
    title: &'static str,
    project: Entity<Project>,
    search: ClassSearch,
    chosen: Rc<dyn Fn(String, &mut App)>,
    query: Entity<InputState>,
    classes: Vec<String>,
    selected: usize,
    focus: FocusHandle,
    _searching: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ClassChooser {
    fn new(
        title: &'static str,
        project: Entity<Project>,
        search: ClassSearch,
        chosen: Rc<dyn Fn(String, &mut App)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.find(cx),
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                _ => {}
            });
        query.update(cx, |field, cx| field.focus(window, cx));
        Self {
            title,
            project,
            search,
            chosen,
            query,
            classes: Vec::new(),
            selected: 0,
            focus: cx.focus_handle(),
            _searching: Task::ready(()),
            _subscriptions: vec![subscription],
        }
    }

    fn find(&mut self, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value().trim().to_owned();
        if text.is_empty() {
            self.classes.clear();
            cx.notify();
            return;
        }
        let found = (self.search)(&self.project, text, cx);
        self._searching = cx.spawn(async move |this, cx| {
            let mut classes = found.await;
            // by name, then package, as IntelliJ's list sorts them
            classes.sort_by(|a, b| simple(a).cmp(simple(b)).then(a.cmp(b)));
            classes.dedup();
            this.update(cx, |this, cx| {
                this.classes = classes;
                this.selected = 0;
                cx.notify();
            })
            .log_err();
        });
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(class) = self.classes.get(self.selected).cloned() else {
            return;
        };
        (self.chosen)(class, cx);
        window.remove_window();
    }

    fn rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let tokens = Tokens::get(cx);
        self.classes
            .iter()
            .enumerate()
            .map(|(ix, class)| {
                let name = simple(class).to_owned();
                let package = class
                    .rsplit_once('.')
                    .map(|(package, _)| package.to_owned());
                let selected = ix == self.selected;
                let label = match &package {
                    Some(package) => format!("{name} ({package})"),
                    None => name.clone(),
                };
                h_flex()
                    .id(("class-chooser-row", ix))
                    .role(Role::ListItem)
                    .aria_label(SharedString::from(label))
                    .aria_selected(selected)
                    .h(px(24.))
                    .mx(px(4.))
                    .px(px(6.))
                    .gap(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(tokens.selection))
                    .when(!selected, |row| {
                        row.hover(move |row| row.bg(tokens.selection_hover))
                    })
                    .child(spork_workbench::image("nodes/exception", cx).size(px(16.)))
                    .child(name)
                    .children(package.map(|package| {
                        div()
                            .text_color(tokens.text_secondary)
                            .child(format!("({package})"))
                    }))
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            this.selected = ix;
                            cx.notify();
                            if event.click_count() > 1 {
                                this.confirm(window, cx);
                            }
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

/// A class's name without its package; a nested class's after its `$`.
fn simple(class: &str) -> &str {
    class.rsplit(['.', '$']).next().unwrap_or(class)
}

impl Focusable for ClassChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ClassChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let rows = self.rows(cx);
        let count = self.classes.len();
        let tab = |label: &'static str, on: bool| {
            div()
                .id(label)
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(on)
                .px(px(10.))
                .h(px(28.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .when(on, |tab| tab.bg(tokens.tab_selected))
                .when(!on, |tab| tab.text_color(tokens.text_disabled))
                .child(label)
        };
        v_flex()
            .id("class-chooser")
            .key_context("ClassChooser")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens.panel)
            .text_color(tokens.text)
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => window.remove_window(),
                        "down" if this.selected + 1 < count => this.selected += 1,
                        "up" => this.selected = this.selected.saturating_sub(1),
                        _ => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(spork_workbench::shell::window_title(self.title, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(12.))
                    .pt(px(8.))
                    .gap(px(8.))
                    // shortcut: the Project tab's tree of the project's classes isn't built
                    .child(
                        h_flex()
                            .id("class-chooser-tabs")
                            .role(Role::TabList)
                            .aria_label("Class Chooser")
                            .gap(px(2.))
                            .child(tab("Search by Name", true))
                            .child(tab("Project", false)),
                    )
                    .child(
                        spork_workbench::TextField::new(&self.query).aria_label("Search by Name"),
                    )
                    .child(
                        v_flex()
                            .id("class-chooser-list")
                            .role(Role::List)
                            .aria_label("Classes")
                            .flex_1()
                            .min_h_0()
                            .py(px(4.))
                            .border_1()
                            .border_color(tokens.separator)
                            .rounded(px(4.))
                            .bg(tokens.island)
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .child(
                        spork_workbench::Button::new("class-chooser-cancel", "Cancel")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        spork_workbench::Button::new("class-chooser-ok", "OK")
                            .primary(true)
                            .disabled(count == 0)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::simple;

    #[test]
    fn a_class_is_listed_by_its_simple_name() {
        assert_eq!(
            simple("java.lang.NumberFormatException"),
            "NumberFormatException"
        );
        assert_eq!(simple("com.example.Outer$Failure"), "Failure");
        assert_eq!(simple("Plain"), "Plain");
    }
}
