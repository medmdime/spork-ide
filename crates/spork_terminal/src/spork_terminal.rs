//! A terminal inside the app, like IntelliJ's Terminal tool window: the user's shell in a
//! folder, drawn with GPUI, its screen kept by `alacritty_terminal`.
// shortcut: no text selection or copy, no IME / dead keys, wide characters take one cell; add
// them when the terminal becomes the IDE's.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{
    Color, CursorShape, NamedColor, Processor, Rgb, StdSyncHandler,
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

#[derive(Clone)]
struct Listener(smol::channel::Sender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.try_send(event).ok();
    }
}

/// The grid's size, for `Term`.
#[derive(Clone, Copy, PartialEq)]
struct Grid {
    cols: usize,
    rows: usize,
}

impl Dimensions for Grid {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// A shell running in a folder, and its screen.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    focus: FocusHandle,
    grid: Grid,
    exited: bool,
    _events: Task<()>,
    _keys: Subscription,
}

impl Terminal {
    /// Starts the user's login shell (`$SHELL`) in `dir`. `history`: what an earlier session
    /// showed (`text`), put back on screen above the new prompt. Focus it with `focus_handle`.
    pub fn start(
        dir: PathBuf,
        history: Option<&str>,
        cx: &mut App,
    ) -> std::io::Result<Entity<Self>> {
        let grid = Grid { cols: 80, rows: 24 };
        let (tx, rx) = smol::channel::unbounded();
        let listener = Listener(tx);
        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &grid,
            listener.clone(),
        )));
        if let Some(text) = history.filter(|t| !t.trim().is_empty()) {
            let replay = format!(
                "{}\r\n\x1b[2m(restored)\x1b[0m\r\n",
                text.replace('\n', "\r\n")
            );
            Processor::<StdSyncHandler>::new().advance(&mut *term.lock(), replay.as_bytes());
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ]);
        let options = tty::Options {
            shell: Some(tty::Shell::new(shell, vec!["-l".into()])),
            working_directory: Some(dir),
            drain_on_exit: true,
            env,
            #[cfg(not(windows))]
            child_signal_mask: None,
            #[cfg(target_os = "windows")]
            escape_args: true,
        };
        let pty = tty::new(&options, window_size(grid, size(px(8.), px(16.))), 0)?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        event_loop.spawn();
        Ok(cx.new(|cx| Self::new(term, sender, rx, cx)))
    }

    fn new(
        term: Arc<FairMutex<Term<Listener>>>,
        pty: EventLoopSender,
        rx: smol::channel::Receiver<Event>,
        cx: &mut Context<Self>,
    ) -> Self {
        let events = cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                // a burst of output is many wake-ups: draw once for all of them
                let mut events = vec![event];
                events.extend(std::iter::from_fn(|| rx.try_recv().ok()));
                let alive = this.update(cx, |this, cx| {
                    for event in events {
                        match event {
                            Event::PtyWrite(text) => this.write(text.into_bytes()),
                            Event::Exit | Event::ChildExit(_) => this.exited = true,
                            _ => {}
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        // keys go to the shell before the app's own bindings (arrows, backspace, space…);
        // ⌘ shortcuts stay the app's
        let focus = cx.focus_handle();
        let (keys_focus, view) = (focus.clone(), cx.entity().downgrade());
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if !keys_focus.is_focused(window) {
                return;
            }
            let handled = view
                .update(cx, |this, cx| this.key(&event.keystroke, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        Self {
            term,
            pty,
            focus,
            grid: Grid { cols: 80, rows: 24 },
            exited: false,
            _events: events,
            _keys: keys,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The shell has ended (`exit`, or it crashed).
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// What the terminal holds, scrollback included, as plain text: wrapped lines joined,
    /// trailing blanks dropped; the last `MAX_SAVED` lines.
    pub fn text(&self) -> String {
        grid_text(&self.term.lock())
    }

    /// Types `command` into the shell and runs it.
    pub fn run(&self, command: &str) {
        self.write(format!("{command}\r").into_bytes());
    }

    fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.pty.send(Msg::Input(bytes.into())).ok();
    }

    fn key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        let mode = *self.term.lock().mode();
        if key.modifiers.platform && key.key == "v" {
            let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
                return true;
            };
            let text = text.replace("\r\n", "\r").replace('\n', "\r");
            let text = if mode.contains(TermMode::BRACKETED_PASTE) {
                format!("\x1b[200~{text}\x1b[201~")
            } else {
                text
            };
            self.write(text.into_bytes());
            return true;
        }
        let Some(bytes) = to_esc(key, mode.contains(TermMode::APP_CURSOR)) else {
            return false;
        };
        // typing brings the screen back to the bottom
        self.term.lock().scroll_display(Scroll::Bottom);
        self.write(bytes);
        true
    }

    fn scroll(&mut self, lines: i32, cx: &mut Context<Self>) {
        let alt_screen = self.term.lock().mode().contains(TermMode::ALT_SCREEN);
        if alt_screen {
            // full-screen programs (less, vim) scroll with the arrow keys
            let arrow = if lines > 0 { "\x1b[A" } else { "\x1b[B" };
            self.write(arrow.repeat(lines.unsigned_abs() as usize).into_bytes());
        } else {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }
        cx.notify();
    }

    /// Fits the grid to `bounds`, telling the shell its new size.
    fn fit(&mut self, bounds: Bounds<Pixels>, cell: Size<Pixels>) {
        let grid = Grid {
            cols: ((bounds.size.width / cell.width) as usize).max(2),
            rows: ((bounds.size.height / cell.height) as usize).max(1),
        };
        if grid != self.grid {
            self.grid = grid;
            self.term.lock().resize(grid);
            self.pty.send(Msg::Resize(window_size(grid, cell))).ok();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.pty.send(Msg::Shutdown).ok();
    }
}

/// `Terminal::text` for any terminal.
fn grid_text<T>(term: &Term<T>) -> String {
    const MAX_SAVED: usize = 2000;
    let grid = term.grid();
    let mut lines: Vec<String> = Vec::new();
    let mut wrapped = false;
    for line in grid.topmost_line().0..=grid.bottommost_line().0 {
        let row = &grid[Line(line)];
        let text: String = (0..grid.columns())
            .map(|c| &row[Column(c)])
            .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
            .map(|cell| cell.c)
            .collect();
        match (wrapped, lines.last_mut()) {
            (true, Some(last)) => last.push_str(&text),
            _ => lines.push(text),
        }
        wrapped = row[Column(grid.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE);
        if !wrapped && let Some(last) = lines.last_mut() {
            last.truncate(last.trim_end().len());
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let skip = lines.len().saturating_sub(MAX_SAVED);
    lines[skip..].join("\n")
}

fn window_size(grid: Grid, cell: Size<Pixels>) -> WindowSize {
    WindowSize {
        num_lines: grid.rows as u16,
        num_cols: grid.cols as u16,
        cell_width: f32::from(cell.width) as u16,
        cell_height: f32::from(cell.height) as u16,
    }
}

/// The bytes a key sends to the shell, as xterm sends them; `None` for keys the app keeps.
fn to_esc(key: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &key.modifiers;
    if m.platform {
        return None;
    }
    let cursor = |c: char| match app_cursor {
        true => format!("\x1bO{c}"),
        false => format!("\x1b[{c}"),
    };
    let text = match key.key.as_str() {
        "enter" => "\r".to_owned(),
        "backspace" if m.alt => "\x1b\x7f".to_owned(),
        "backspace" => "\x7f".to_owned(),
        "tab" if m.shift => "\x1b[Z".to_owned(),
        "tab" => "\t".to_owned(),
        "escape" => "\x1b".to_owned(),
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => "\x1b[2~".to_owned(),
        "delete" => "\x1b[3~".to_owned(),
        "pageup" => "\x1b[5~".to_owned(),
        "pagedown" => "\x1b[6~".to_owned(),
        "space" if m.control => "\0".to_owned(),
        k if m.control && k.chars().count() == 1 => {
            let c = k.chars().next()?.to_ascii_lowercase();
            let byte = match c {
                'a'..='z' => c as u8 - b'a' + 1,
                '[' => 0x1b,
                '\\' => 0x1c,
                ']' => 0x1d,
                '^' => 0x1e,
                '_' => 0x1f,
                _ => return None,
            };
            return Some(vec![byte]);
        }
        _ => key.key_char.clone()?,
    };
    Some(text.into_bytes())
}

/// xterm's 256 colors: the 16 ANSI ones (IntelliJ-like, by theme), a 6×6×6 cube and grays.
fn indexed(i: u8, dark: bool) -> Hsla {
    const DARK: [u32; 16] = [
        0x1E1F22, 0xF0524F, 0x5C962C, 0xA68A0D, 0x3993D4, 0xA771BF, 0x00A3A3, 0xB4B8BF, 0x595959,
        0xFF4050, 0x4FC414, 0xE5BF00, 0x1FB0FF, 0xED7EED, 0x00E5E5, 0xFFFFFF,
    ];
    const LIGHT: [u32; 16] = [
        0x000000, 0xC4303B, 0x2B7A0B, 0x8F6A00, 0x2F5EB9, 0x8E3FA8, 0x00787A, 0x6C707E, 0x5A5D63,
        0xE55765, 0x369650, 0xC27D04, 0x3871E1, 0xA571E6, 0x039BA1, 0x000000,
    ];
    let hex = match i {
        0..16 => (if dark { DARK } else { LIGHT })[i as usize],
        16..232 => {
            let n = i as u32 - 16;
            let level = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
            (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
        }
        _ => {
            let g = 8 + 10 * (i as u32 - 232);
            (g << 16) | (g << 8) | g
        }
    };
    rgb(hex).into()
}

fn spec(c: Rgb) -> Hsla {
    rgb(((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32).into()
}

/// What a cell's color means on screen: the shell's own choice (OSC) first, else the palette.
fn resolve(
    color: Color,
    overrides: &alacritty_terminal::term::color::Colors,
    fg: Hsla,
    bg: Hsla,
    dark: bool,
) -> Hsla {
    match color {
        Color::Spec(c) => spec(c),
        Color::Indexed(i) => overrides[i as usize].map_or_else(|| indexed(i, dark), spec),
        Color::Named(n) => match overrides[n] {
            Some(c) => spec(c),
            None => match n {
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => fg,
                NamedColor::Background => bg,
                NamedColor::DimForeground => fg.opacity(0.66),
                n if (n as usize) < 16 => indexed(n as u8, dark),
                // the dim colors follow the normal ones
                n => {
                    indexed((n as usize - NamedColor::DimBlack as usize) as u8, dark).opacity(0.66)
                }
            },
        },
    }
}

impl Render for Terminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, bg, dark) = (theme.foreground, theme.background, theme.mode.is_dark());
        let font = Font {
            family: theme.mono_font_family.clone(),
            ..Default::default()
        };
        let font_size = theme.mono_font_size;
        let line_height = (font_size * 1.35).round();
        let focused = self.focus.is_focused(window);
        let view = cx.entity();
        let term = self.term.clone();
        let measure_font = font.clone();
        div()
            .id("terminal")
            .size_full()
            .track_focus(&self.focus)
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                }),
            )
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, _, cx| {
                let lines = (event.delta.pixel_delta(line_height).y / line_height).round() as i32;
                if lines != 0 {
                    this.scroll(lines, cx);
                }
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let font_id = window.text_system().resolve_font(&measure_font);
                        let width = window
                            .text_system()
                            .advance(font_id, font_size, 'm')
                            .map_or(font_size * 0.6, |s| s.width);
                        let cell = size(width, line_height);
                        view.update(cx, |this, _| this.fit(bounds, cell));
                        cell
                    },
                    move |bounds, cell, window, cx| {
                        let term = term.lock();
                        let content = term.renderable_content();
                        let offset = content.display_offset as i32;
                        let rows = term.screen_lines();
                        let mut lines: Vec<(String, Vec<TextRun>)> = vec![Default::default(); rows];
                        for indexed in content.display_iter {
                            let row = indexed.point.line.0 + offset;
                            let Some((text, runs)) =
                                lines.get_mut(row.max(0) as usize).filter(|_| row >= 0)
                            else {
                                continue;
                            };
                            let cell_flags = indexed.cell.flags;
                            if cell_flags.contains(Flags::WIDE_CHAR_SPACER) {
                                continue;
                            }
                            let (mut cfg, mut cbg) = (
                                resolve(indexed.cell.fg, content.colors, fg, bg, dark),
                                resolve(indexed.cell.bg, content.colors, fg, bg, dark),
                            );
                            if cell_flags.contains(Flags::INVERSE) {
                                std::mem::swap(&mut cfg, &mut cbg);
                            }
                            if cell_flags.contains(Flags::DIM) {
                                cfg = cfg.opacity(0.66);
                            }
                            let c = if cell_flags.contains(Flags::HIDDEN) {
                                ' '
                            } else {
                                indexed.cell.c
                            };
                            let run_font = Font {
                                weight: if cell_flags.contains(Flags::BOLD) {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                },
                                style: if cell_flags.contains(Flags::ITALIC) {
                                    FontStyle::Italic
                                } else {
                                    FontStyle::Normal
                                },
                                ..font.clone()
                            };
                            let background = (cbg != bg).then_some(cbg);
                            let underline =
                                cell_flags.intersects(Flags::ALL_UNDERLINES).then(|| {
                                    UnderlineStyle {
                                        thickness: px(1.),
                                        color: Some(cfg),
                                        wavy: false,
                                    }
                                });
                            let len = c.len_utf8();
                            text.push(c);
                            match runs.last_mut() {
                                Some(r)
                                    if r.color == cfg
                                        && r.background_color == background
                                        && r.font == run_font
                                        && r.underline == underline =>
                                {
                                    r.len += len
                                }
                                _ => runs.push(TextRun {
                                    len,
                                    font: run_font,
                                    color: cfg,
                                    background_color: background,
                                    underline,
                                    strikethrough: None,
                                }),
                            }
                        }
                        for (row, (text, runs)) in lines.into_iter().enumerate() {
                            if text.is_empty() {
                                continue;
                            }
                            let origin =
                                point(bounds.origin.x, bounds.origin.y + cell.height * row as f32);
                            let line = window.text_system().shape_line(
                                text.into(),
                                font_size,
                                &runs,
                                Some(cell.width),
                            );
                            line.paint_background(
                                origin,
                                cell.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            )
                            .ok();
                            line.paint(origin, cell.height, TextAlign::Left, None, window, cx)
                                .ok();
                        }
                        let cursor = content.cursor;
                        let row = cursor.point.line.0 + offset;
                        if cursor.shape != CursorShape::Hidden && (0..rows as i32).contains(&row) {
                            let at = point(
                                bounds.origin.x + cell.width * cursor.point.column.0 as f32,
                                bounds.origin.y + cell.height * row as f32,
                            );
                            let block = Bounds::new(at, cell);
                            if focused {
                                window.paint_quad(fill(block, fg.opacity(0.55)));
                            } else {
                                window.paint_quad(outline(
                                    block,
                                    fg.opacity(0.55),
                                    BorderStyle::Solid,
                                ));
                            }
                        }
                    },
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Grid, Processor, StdSyncHandler, Term, grid_text};
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;

    #[test]
    fn saved_text_joins_wrapped_lines_and_replays() {
        let mut term = Term::new(Config::default(), &Grid { cols: 10, rows: 5 }, VoidListener);
        let mut parser = Processor::<StdSyncHandler>::new();
        parser.advance(&mut term, b"$ ls\r\na-very-long-name.txt\r\n$ ");
        assert_eq!(grid_text(&term), "$ ls\na-very-long-name.txt\n$");
    }
}
