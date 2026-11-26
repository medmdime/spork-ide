use gpui::{App, Menu, MenuItem, OsAction};
use project::DisableAiSettings;
use settings::Settings;
use spork_code::code;
use spork_run::{run, run_widget};
use terminal_view::terminal_panel;
use zed_actions::{Quit, assistant, debug_panel, project_panel};

/// The menu bar in IntelliJ's order (reference/idea/menus.md), each menu with IntelliJ's items,
/// texts and separators where Spork IDE has the action; macOS's app menu holds About, Settings…
/// and Quit as IntelliJ's does.
// shortcut: IntelliJ's items without an action yet are left out, open tasks in
// reference/idea/tasks; its dynamic texts (Run 'Main') read as the action's own
pub fn app_menus(cx: &mut App) -> Vec<Menu> {
    // IntelliJ names Run and Debug after what they run: `Run 'GreetingTest'`
    let run_menu = spork_run::run_menu(cx);
    // IntelliJ lists the tool windows by name; AI Chat is the agent panel, there while AI is on
    let mut tool_windows = Vec::new();
    if !DisableAiSettings::get_global(cx).disable_ai {
        tool_windows.push(MenuItem::action("AI Chat", assistant::ToggleFocus));
    }
    tool_windows.extend([
        MenuItem::action(
            "Bookmarks",
            spork_navigate::bookmarks::bookmarks_panel::ToggleFocus,
        ),
        MenuItem::action("Commit", spork_vcs::commit_panel::ToggleFocus),
        MenuItem::action("Debug", debug_panel::ToggleFocus),
        MenuItem::action("Git", spork_vcs::git_panel::ToggleFocus),
        MenuItem::action("Hierarchy", spork_code::hierarchy_panel::ToggleFocus),
        MenuItem::action(
            "Notifications",
            spork_tool_windows::notifications_panel::ToggleFocus,
        ),
        MenuItem::action("Problems", spork_tool_windows::problems_panel::ToggleFocus),
        MenuItem::action("Project", project_panel::ToggleFocus),
        MenuItem::action("Run", spork_run::run_panel::run_panel::ToggleFocus),
        MenuItem::action("Services", spork_run::services::services_panel::ToggleFocus),
        MenuItem::action("Structure", outline_panel::ToggleFocus),
        MenuItem::action("Terminal", terminal_panel::ToggleFocus),
        MenuItem::action("TODO", spork_tool_windows::todo_panel::ToggleFocus),
    ]);

    // IntelliJ's `UpdateRunningApplicationAction` names the one application it updates
    let update_text = run_menu
        .update
        .clone()
        .unwrap_or_else(|| "Update Running Application…".into());

    vec![
        Menu {
            name: "Spork IDE".into(),
            disabled: false,
            items: vec![
                MenuItem::action("About Spork IDE", zed_actions::About),
                MenuItem::separator(),
                MenuItem::action("Settings…", zed_actions::OpenSettings),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::action("Hide Spork IDE", super::Hide),
                #[cfg(target_os = "macos")]
                MenuItem::action("Hide Others", super::HideOthers),
                #[cfg(target_os = "macos")]
                MenuItem::action("Show All", super::ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Spork IDE", Quit),
            ],
        },
        Menu::new("File").items([
            MenuItem::submenu(Menu::new("New").items([
                MenuItem::action("Project…", spork_projects::NewProject),
                MenuItem::action(
                    "Project from Version Control…",
                    spork_projects::GetFromVersionControl,
                ),
                MenuItem::separator(),
                MenuItem::action("File", ::project_panel::NewFile),
                MenuItem::action("Scratch File", ::project_panel::NewScratchFile),
                MenuItem::action("Directory", ::project_panel::NewDirectory),
            ])),
            MenuItem::action("Open…", workspace::Open::default()),
            MenuItem::action("Attach Project…", workspace::AddFolderToProject),
            MenuItem::submenu(
                Menu::new("Recent Projects").items(spork_projects::recent_projects_menu(cx)),
            ),
            MenuItem::action("Close Project", workspace::CloseProject),
            MenuItem::action("Close All Projects", spork_projects::CloseAllProjects),
            MenuItem::action("Close Other Projects", spork_projects::CloseOtherProjects),
            MenuItem::separator(),
            MenuItem::action(
                "Project Structure…",
                spork_java::project_structure::ShowProjectStructureSettings,
            ),
            MenuItem::action("Remote Development…", zed_actions::OpenRemote::default()),
            MenuItem::separator(),
            MenuItem::action("Plugins…", zed_actions::Extensions::default()),
            MenuItem::submenu(Menu::new("File Properties").items([
                MenuItem::action("File Encoding", encoding_selector::Toggle),
                MenuItem::action("Line Separators", line_ending_selector::Toggle),
            ])),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Local History").items([
                MenuItem::action("Show History…", spork_local_history::ShowHistory),
                MenuItem::action("Put Label…", spork_local_history::PutLabel),
            ])),
            MenuItem::separator(),
            MenuItem::action("Save All", workspace::SaveAll { save_intent: None }),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Manage IDE Settings").items([
                MenuItem::action("Import Settings…", spork_settings::ImportSettings),
                MenuItem::action("Export Settings…", spork_settings::ExportSettings),
            ])),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", editor::actions::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", editor::actions::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", editor::actions::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", editor::actions::Copy, OsAction::Copy),
            MenuItem::action("Copy Paths", zed_actions::workspace::CopyPath),
            MenuItem::submenu(Menu::new("Copy Path/Reference…").items([
                MenuItem::action("Absolute Path", zed_actions::workspace::CopyPath),
                MenuItem::action(
                    "Path From Content Root",
                    zed_actions::workspace::CopyRelativePath,
                ),
                MenuItem::action("Path With Line Number", editor::actions::CopyFileLocation),
            ])),
            MenuItem::submenu(Menu::new("Paste").items([
                MenuItem::os_action("Paste", editor::actions::Paste, OsAction::Paste),
                MenuItem::action("Paste from History…", spork_editor::PasteFromHistory),
                // Zed's paste is plain text
                MenuItem::action("Paste as Plain Text", editor::actions::Paste),
            ])),
            MenuItem::action("Delete", editor::actions::Delete),
            MenuItem::separator(),
            MenuItem::action("Search in Selection Only", search::ToggleSelection),
            MenuItem::submenu(Menu::new("Find").items([
                MenuItem::action("Find…", search::buffer_search::Deploy::find()),
                MenuItem::action("Replace…", search::buffer_search::Deploy::replace()),
                MenuItem::action(
                    "Find Next / Move to Next Occurrence",
                    search::SelectNextMatch,
                ),
                MenuItem::action(
                    "Find Previous / Move to Previous Occurrence",
                    search::SelectPreviousMatch,
                ),
                MenuItem::action("Select All Occurrences", editor::actions::SelectAllMatches),
                MenuItem::action(
                    "Add Selection for Next Occurrence",
                    editor::actions::SelectNext {
                        replace_newest: false,
                    },
                ),
                MenuItem::action("Unselect Occurrence", editor::actions::UndoSelection),
                MenuItem::action("Search in Selection Only", search::ToggleSelection),
                MenuItem::separator(),
                MenuItem::action(
                    "Next Occurrence of the Word at Caret",
                    editor::actions::SelectNext {
                        replace_newest: true,
                    },
                ),
                MenuItem::action(
                    "Previous Occurrence of the Word at Caret",
                    editor::actions::SelectPrevious {
                        replace_newest: true,
                    },
                ),
                MenuItem::separator(),
                MenuItem::action("Find in Files…", spork_navigate::navigate::FindInFiles),
                MenuItem::action(
                    "Replace in Files…",
                    spork_navigate::navigate::ReplaceInFiles,
                ),
            ])),
            MenuItem::submenu(Menu::new("Find Usages").items([
                MenuItem::action("Find Usages", code::FindUsages),
                MenuItem::action("Show Usages", code::ShowUsages),
                MenuItem::separator(),
                MenuItem::action(
                    "Next Highlighted Usage",
                    editor::actions::GoToNextDocumentHighlight,
                ),
                MenuItem::action(
                    "Previous Highlighted Usage",
                    editor::actions::GoToPreviousDocumentHighlight,
                ),
            ])),
            MenuItem::separator(),
            MenuItem::action("Column Selection Mode", spork_editor::ToggleColumnMode),
            MenuItem::os_action(
                "Select All",
                editor::actions::SelectAll,
                OsAction::SelectAll,
            ),
            MenuItem::action(
                "Add Carets to Ends of Selected Lines",
                spork_editor::AddCaretsToLineEnds,
            ),
            MenuItem::action("Extend Selection", editor::actions::SelectLargerSyntaxNode),
            MenuItem::action("Shrink Selection", editor::actions::SelectSmallerSyntaxNode),
            MenuItem::separator(),
            MenuItem::action("Toggle Case", editor::actions::ToggleCase),
            MenuItem::action("Join Lines", editor::actions::JoinLines),
            MenuItem::action(
                "Duplicate Line or Selection",
                editor::actions::DuplicateSelection,
            ),
            MenuItem::action("Fill Paragraph", editor::actions::Rewrap),
            MenuItem::action("Sort Lines", editor::actions::SortLinesCaseSensitive),
            MenuItem::action("Reverse Lines", editor::actions::ReverseLines),
            MenuItem::action("Transpose", editor::actions::Transpose),
            MenuItem::separator(),
            MenuItem::action("Indent Selection", editor::actions::Indent),
            MenuItem::action("Unindent Line or Selection", editor::actions::Outdent),
            MenuItem::submenu(Menu::new("Convert Indents").items([
                MenuItem::action("To Spaces", editor::actions::ConvertIndentationToSpaces),
                MenuItem::action("To Tabs", editor::actions::ConvertIndentationToTabs),
            ])),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Bookmarks").items([
                MenuItem::action("Rename Bookmark…", spork_navigate::navigate::RenameBookmark),
                MenuItem::action("Toggle Bookmark", editor::actions::ToggleBookmark),
                MenuItem::action("Remove Mnemonic", spork_navigate::navigate::RemoveMnemonic),
                MenuItem::action(
                    "Toggle Bookmark Mnemonic…",
                    spork_navigate::navigate::ToggleBookmarkMnemonic,
                ),
                MenuItem::separator(),
                MenuItem::action(
                    "Previous Line Bookmark",
                    editor::actions::GoToPreviousBookmark,
                ),
                MenuItem::action("Next Line Bookmark", editor::actions::GoToNextBookmark),
                MenuItem::separator(),
                MenuItem::action("Show Bookmarks…", spork_navigate::navigate::ShowBookmarks),
                MenuItem::action("Go to Mnemonic…", spork_navigate::navigate::GoToMnemonic),
            ])),
        ]),
        Menu::new("View").items([
            MenuItem::submenu(Menu::new("Tool Windows").items(tool_windows)),
            MenuItem::submenu(Menu::new("Appearance").items([MenuItem::action(
                "Toggle Full Screen Mode",
                super::ToggleFullScreen,
            )])),
            MenuItem::separator(),
            MenuItem::action("Quick Definition", code::QuickDefinition),
            MenuItem::action("Quick Documentation", spork_editor::QuickDocumentation),
            MenuItem::action("External Documentation", editor::actions::OpenDocs),
            MenuItem::action("Parameter Info", editor::actions::ShowSignatureHelp),
            MenuItem::separator(),
            MenuItem::action("Jump to Source", editor::actions::OpenExcerpts),
            MenuItem::separator(),
            MenuItem::action("Recent Files", spork_navigate::navigate::RecentFiles),
            MenuItem::action(
                "Recently Changed Files",
                spork_navigate::navigate::RecentlyChangedFiles,
            ),
            MenuItem::action(
                "Recent Locations",
                spork_navigate::navigate::RecentLocations,
            ),
            MenuItem::separator(),
            MenuItem::action("Compare Files", ::project_panel::CompareMarkedFiles),
            MenuItem::action(
                "Compare with Clipboard",
                editor::actions::DiffClipboardWithSelection,
            ),
            MenuItem::separator(),
            MenuItem::action(
                "Quick Switch Scheme…",
                zed_actions::theme_selector::Toggle::default(),
            ),
            MenuItem::submenu(Menu::new("Active Editor").items([
                MenuItem::action("Soft-Wrap", editor::actions::ToggleSoftWrap),
                MenuItem::separator(),
                MenuItem::action("Show Line Numbers", editor::actions::ToggleLineNumbers),
                MenuItem::action("Show Indent Guides", editor::actions::ToggleIndentGuides),
                MenuItem::action("Show Breadcrumbs", editor::actions::ToggleBreadcrumb),
                MenuItem::separator(),
                MenuItem::action(
                    "Increase Font Size",
                    zed_actions::IncreaseBufferFontSize { persist: false },
                ),
                MenuItem::action(
                    "Decrease Font Size",
                    zed_actions::DecreaseBufferFontSize { persist: false },
                ),
            ])),
            MenuItem::separator(),
            MenuItem::action(
                "Increase Font Size in All Editors",
                zed_actions::IncreaseBufferFontSize { persist: true },
            ),
            MenuItem::action(
                "Decrease Font Size in All Editors",
                zed_actions::DecreaseBufferFontSize { persist: true },
            ),
            MenuItem::action(
                "Reset Font Size in All Editors",
                zed_actions::ResetBufferFontSize { persist: true },
            ),
        ]),
        Menu::new("Navigate").items([
            MenuItem::action("Back", workspace::GoBack),
            MenuItem::action("Forward", workspace::GoForward),
            MenuItem::separator(),
            MenuItem::action(
                "Search Everywhere",
                spork_navigate::navigate::SearchEverywhere,
            ),
            MenuItem::separator(),
            MenuItem::action("Class…", spork_navigate::navigate::GoToClass),
            MenuItem::action("File…", spork_navigate::navigate::GoToFile),
            MenuItem::action("Symbol…", spork_navigate::navigate::GoToSymbol),
            MenuItem::action("Text…", spork_navigate::navigate::GoToText),
            MenuItem::action("Line:Column…", spork_navigate::navigate::GoToLine),
            MenuItem::action("Endpoint…", spork_navigate::navigate::GoToEndpoint),
            MenuItem::separator(),
            MenuItem::action(
                "Next Highlighted Error",
                editor::actions::GoToDiagnostic::default(),
            ),
            MenuItem::action(
                "Previous Highlighted Error",
                editor::actions::GoToPreviousDiagnostic::default(),
            ),
            MenuItem::separator(),
            MenuItem::action(
                "Last Edit Location",
                spork_navigate::navigate::LastEditLocation,
            ),
            MenuItem::action(
                "Next Edit Location",
                spork_navigate::navigate::NextEditLocation,
            ),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Navigate in File").items([
                MenuItem::action("Next Method", editor::actions::GoToNextSymbol),
                MenuItem::action("Previous Method", editor::actions::GoToPreviousSymbol),
                MenuItem::action(
                    "Move Caret to Matching Brace",
                    spork_editor::MoveToMatchingBrace,
                ),
                MenuItem::separator(),
                MenuItem::action("Next Change", spork_vcs::ShowNextChangeMarker),
                MenuItem::action("Previous Change", spork_vcs::ShowPrevChangeMarker),
            ])),
            MenuItem::separator(),
            MenuItem::action("Select In…", spork_navigate::navigate::SelectIn),
            MenuItem::action(
                "Jump to Navigation Bar",
                spork_navigate::navigate::JumpToNavigationBar,
            ),
            MenuItem::separator(),
            MenuItem::action("Declaration or Usages", code::GotoDeclaration),
            MenuItem::action("Implementation(s)", code::GotoImplementation),
            MenuItem::action("Type Declaration", code::GotoTypeDeclaration),
            MenuItem::action("Super Method", code::GotoSuperMethod),
            MenuItem::action("Test", code::GotoTest),
            MenuItem::action("Related Symbol…", code::GotoRelated),
            MenuItem::separator(),
            MenuItem::action("File Structure", spork_navigate::navigate::FileStructure),
            MenuItem::action("File Path", spork_navigate::navigate::ShowFilePath),
            MenuItem::action("Type Hierarchy", code::TypeHierarchy),
            MenuItem::action("Call Hierarchy", code::CallHierarchy),
            MenuItem::separator(),
            MenuItem::action(
                "Previous Occurrence",
                spork_navigate::navigate::PreviousOccurrence,
            ),
            MenuItem::action("Next Occurrence", spork_navigate::navigate::NextOccurrence),
        ]),
        Menu::new("Code").items([
            MenuItem::action("Override Methods…", spork_java::java::OverrideMethods),
            MenuItem::action("Implement Methods…", spork_java::java::ImplementMethods),
            MenuItem::action("Delegate Methods…", spork_java::java::DelegateMethods),
            MenuItem::action("Generate…", code::Generate),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Code Completion").items([
                MenuItem::action("Basic", editor::actions::ShowCompletions),
                MenuItem::separator(),
                MenuItem::action(
                    "Call Inline Completion",
                    editor::actions::ShowEditPrediction,
                ),
            ])),
            MenuItem::separator(),
            MenuItem::action("Inspect Code…", code::InspectCode),
            MenuItem::separator(),
            MenuItem::action("Surround With…", code::SurroundWith),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Folding").items([
                MenuItem::action("Expand", editor::actions::UnfoldLines),
                MenuItem::action("Expand Recursively", editor::actions::UnfoldRecursive),
                MenuItem::action("Expand All", editor::actions::UnfoldAll),
                MenuItem::separator(),
                MenuItem::action("Collapse", editor::actions::Fold),
                MenuItem::action("Collapse Recursively", editor::actions::FoldRecursive),
                MenuItem::action("Collapse All", editor::actions::FoldAll),
                MenuItem::separator(),
                MenuItem::action("Toggle Folding", editor::actions::ToggleFold),
                MenuItem::separator(),
                MenuItem::action(
                    "Fold Selection / Remove Region",
                    editor::actions::FoldSelectedRanges,
                ),
            ])),
            MenuItem::separator(),
            MenuItem::action(
                "Comment with Line Comment",
                editor::actions::ToggleComments::default(),
            ),
            MenuItem::action(
                "Comment with Block Comment",
                editor::actions::ToggleBlockComments,
            ),
            MenuItem::action("Reformat Code", code::ReformatCode),
            MenuItem::action("Reformat File…", code::ReformatFile),
            MenuItem::action("Auto-Indent Lines", editor::actions::AutoIndent),
            MenuItem::action("Optimize Imports", code::OptimizeImports),
            MenuItem::separator(),
            MenuItem::action("Move Statement Down", code::MoveStatementDown),
            MenuItem::action("Move Statement Up", code::MoveStatementUp),
            MenuItem::action("Move Line Down", editor::actions::MoveLineDown),
            MenuItem::action("Move Line Up", editor::actions::MoveLineUp),
        ]),
        Menu::new("Refactor").items([
            MenuItem::action("Refactor This…", code::RefactorThis),
            MenuItem::action("Rename…", code::Rename),
            MenuItem::action("Rename File…", ::project_panel::Rename),
            MenuItem::action("Change Signature…", code::ChangeSignature),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Extract/Introduce").items([
                MenuItem::action("Introduce Variable…", code::IntroduceVariable),
                MenuItem::action("Introduce Constant…", code::IntroduceConstant),
                MenuItem::separator(),
                MenuItem::action("Extract Method…", code::ExtractMethod),
            ])),
            MenuItem::action("Inline…", code::Inline),
            MenuItem::separator(),
            MenuItem::action("Copy…", ::project_panel::Duplicate),
        ]),
        // IntelliJ's Java.BuildMenu, on JDT LS's build; Rebuild (⇧⌘F9) shows in IntelliJ's only
        // for a module selected in the Project tool window, which a menu bar can't follow
        Menu::new("Build").items([
            MenuItem::action("Build Project", spork_java::build::BuildProject),
            MenuItem::action("Build Module", spork_java::build::BuildModule),
            MenuItem::separator(),
            MenuItem::action("Rebuild Project", spork_java::build::RebuildProject),
        ]),
        Menu::new("Run").items([
            MenuItem::action(run_menu.run, run_widget::Run).disabled(!run_menu.enabled),
            MenuItem::action(run_menu.debug, run_widget::Debug).disabled(!run_menu.enabled),
            MenuItem::action(run_menu.coverage, run_widget::RunWithCoverage)
                .disabled(!run_menu.enabled),
            MenuItem::separator(),
            MenuItem::action("Run…", run::ChooseRunConfiguration),
            MenuItem::action("Debug…", run::ChooseDebugConfiguration),
            MenuItem::action("Attach to Process…", run::AttachToProcess),
            MenuItem::action("Edit Configurations…", run::EditConfigurations),
            MenuItem::separator(),
            MenuItem::action("Stop", run::Stop),
            MenuItem::action("Stop Background Processes…", run::StopBackgroundProcesses),
            MenuItem::action("Show Running List", run::ShowRunningList),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Debugging Actions").items([
                MenuItem::action(update_text, run::UpdateRunningApplication),
                MenuItem::separator(),
                MenuItem::action("Step Over", debugger_ui::StepOver),
                MenuItem::action("Force Step Over", run::ForceStepOver),
                MenuItem::action("Step Into", debugger_ui::StepInto),
                MenuItem::action("Force Step Into", run::ForceStepInto),
                MenuItem::action("Smart Step Into", run::SmartStepInto),
                MenuItem::action("Step Out", debugger_ui::StepOut),
                MenuItem::action("Run to Cursor", editor::actions::RunToCursor),
                MenuItem::action("Force Run to Cursor", run::ForceRunToCursor),
                MenuItem::action("Reset Frame", run::ResetFrame),
                MenuItem::action("Pause Program", debugger_ui::Pause),
                MenuItem::action("Resume Program", debugger_ui::Continue),
                MenuItem::separator(),
                MenuItem::action("Evaluate Expression…", run::EvaluateExpression),
                MenuItem::action("Show Execution Point", run::ShowExecutionPoint),
                MenuItem::separator(),
                MenuItem::action("Get Thread Dump", run::GetThreadDump),
            ])),
            MenuItem::submenu(Menu::new("Toggle Breakpoint").items([
                MenuItem::action("Line Breakpoint", editor::actions::ToggleBreakpoint),
                MenuItem::action("Method Breakpoint", run::ToggleMethodBreakpoint),
                MenuItem::action("Field Watchpoint", run::ToggleFieldWatchpoint),
                MenuItem::action(
                    "Temporary Line Breakpoint",
                    run::ToggleTemporaryLineBreakpoint,
                ),
                MenuItem::action("Toggle Breakpoint Enabled", run::ToggleBreakpointEnabled),
            ])),
            MenuItem::action("View Breakpoints…", run::ViewBreakpoints),
            MenuItem::separator(),
            // IntelliJ's CoveragePlatformMenu, in RunTestGroup
            MenuItem::action(
                "Manage Coverage Reports…",
                spork_coverage::ManageCoverageReports,
            ),
            MenuItem::action(
                "Generate Coverage Report…",
                spork_coverage::GenerateCoverageReport,
            ),
            MenuItem::action("Hide Coverage", spork_coverage::HideCoverage),
        ]),
        // Spork IDE: its one item installs the CLI, which Windows doesn't have (as upstream's Install CLI)
        #[cfg(not(target_os = "windows"))]
        Menu::new("Tools").items([MenuItem::action(
            "Create Command Line Launcher…",
            install_cli::InstallCliBinary,
        )]),
        // IntelliJ's place, between Tools and Window
        spork_vcs::git_menu(cx),
        Menu::new("Window").items([
            MenuItem::action("Minimize", super::Minimize),
            MenuItem::action("Zoom", super::Zoom),
            MenuItem::submenu(Menu::new("Tool Window Layouts").items([
                MenuItem::action(
                    "Default",
                    spork_tool_windows::tool_windows::RestoreFactoryDefaultLayout,
                ),
                MenuItem::separator(),
                MenuItem::action(
                    "Restore Current Layout",
                    spork_tool_windows::tool_windows::RestoreDefaultLayout,
                ),
            ])),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Active Tool Window").items([
                MenuItem::action("Hide Active Tool Window", workspace::CloseActiveDock),
                MenuItem::action("Hide All Tool Windows", workspace::ToggleAllDocks),
                MenuItem::action(
                    "Jump to Last Tool Window",
                    spork_tool_windows::tool_windows::JumpToLastToolWindow,
                ),
                MenuItem::action("Maximize Tool Window", workspace::ToggleZoom),
                MenuItem::separator(),
                MenuItem::submenu(Menu::new("Resize").items([
                    MenuItem::action("Stretch to Left", workspace::frame::StretchToolWindowLeft),
                    MenuItem::action("Stretch to Right", workspace::frame::StretchToolWindowRight),
                    MenuItem::action("Stretch to Top", workspace::frame::StretchToolWindowUp),
                    MenuItem::action("Stretch to Bottom", workspace::frame::StretchToolWindowDown),
                ])),
            ])),
            MenuItem::submenu(Menu::new("Editor Tabs").items([
                MenuItem::action(
                    "Select Next Tab",
                    workspace::ActivateNextItem { wrap_around: true },
                ),
                MenuItem::action(
                    "Select Previous Tab",
                    workspace::ActivatePreviousItem { wrap_around: true },
                ),
                MenuItem::action("Pin Active Tab", workspace::TogglePinTab),
                MenuItem::separator(),
                MenuItem::action("Close Tab", workspace::CloseActiveItem::default()),
                MenuItem::action("Close Other Tabs", workspace::CloseOtherItems::default()),
                // IntelliJ's Close All Tabs closes the pinned ones too
                MenuItem::action(
                    "Close All Tabs",
                    workspace::CloseAllItems {
                        close_pinned: true,
                        ..Default::default()
                    },
                ),
                MenuItem::action(
                    "Close Unmodified Tabs",
                    workspace::CloseCleanItems::default(),
                ),
                MenuItem::action("Close All but Pinned", workspace::CloseAllItems::default()),
                MenuItem::action(
                    "Close Tabs to the Left",
                    workspace::CloseItemsToTheLeft::default(),
                ),
                MenuItem::action(
                    "Close Tabs to the Right",
                    workspace::CloseItemsToTheRight::default(),
                ),
                MenuItem::separator(),
                MenuItem::action("Reopen Closed Tab", workspace::ReopenClosedItem),
                MenuItem::action("Split Right", workspace::SplitRight::default()),
                MenuItem::action("Split and Move Right", workspace::SplitAndMoveRight),
                MenuItem::action("Split Down", workspace::SplitDown::default()),
                MenuItem::action("Split and Move Down", workspace::SplitAndMoveDown),
                MenuItem::action(
                    "Change Splitter Orientation",
                    workspace::ChangeSplitOrientation,
                ),
                MenuItem::action("Unsplit", workspace::Unsplit),
                MenuItem::action("Unsplit All", workspace::JoinAll),
                MenuItem::action("Go to Next Splitter", workspace::ActivateNextPane),
                MenuItem::action("Go to Previous Splitter", workspace::ActivatePreviousPane),
            ])),
            MenuItem::submenu(Menu::new("Notifications").items([
                MenuItem::action("Close First", workspace::CloseFirstNotification),
                MenuItem::action("Close All", workspace::ClearAllNotifications),
            ])),
            MenuItem::separator(),
            MenuItem::action("Next Project Window", workspace::ActivateNextWindow),
            MenuItem::action("Previous Project Window", workspace::ActivatePreviousWindow),
            MenuItem::action("Merge All Project Windows", title_bar::MergeAllWindows),
        ]),
        Menu::new("Help").items([
            MenuItem::action("Find Action…", spork_navigate::navigate::FindAction),
            MenuItem::separator(),
            MenuItem::action("Configure the New UI", onboarding::ShowWelcome),
            MenuItem::separator(),
            MenuItem::action(
                if cfg!(target_os = "macos") {
                    "Show Log in Finder"
                } else {
                    "Show Log in File Manager"
                },
                workspace::RevealLogInFileManager,
            ),
        ]),
    ]
}
