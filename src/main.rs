use std::io::{self, Stdout, Write};
use std::process;

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tmussh::{TmuxAction, list_sessions, run_remote_tmux, validate_new_session_name};

#[derive(Debug, Parser)]
#[command(name = "tmussh")]
#[command(about = "Pick or create a remote tmux session over ssh")]
struct Args {
    /// SSH destination, for example user@example.com.
    destination: String,

    /// Remote path to tmux, used when automatic discovery cannot find it.
    #[arg(long, value_name = "PATH")]
    tmux_path: Option<String>,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Args::parse();
    let tmux_path = args.tmux_path.as_deref();
    let sessions = list_sessions(&args.destination, tmux_path)?;

    let mut tui = Tui::new()?;
    let action = if sessions.is_empty() {
        let title = format!("No tmux sessions found on {}", args.destination);
        match tui.prompt_new_session_name(&title)? {
            Some(name) => TmuxAction::New(name),
            None => TmuxAction::Quit,
        }
    } else {
        tui.pick_session(&args.destination, &sessions)?
    };
    tui.restore()?;
    drop(tui);

    match action {
        TmuxAction::Quit => Ok(()),
        TmuxAction::Attach(_) | TmuxAction::New(_) => {
            let status = run_remote_tmux(&args.destination, &action, tmux_path)?;
            process::exit(status.code().unwrap_or(1));
        }
    }
}

struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    restored: bool,
}

impl Tui {
    fn new() -> Result<Self> {
        let stdout = io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).wrap_err("failed to initialize terminal")?;

        terminal::enable_raw_mode().wrap_err("failed to enable raw mode")?;
        if let Err(error) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
            let _ = terminal::disable_raw_mode();
            return Err(error).wrap_err("failed to enter alternate screen");
        }

        Ok(Self {
            terminal,
            restored: false,
        })
    }

    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }

        terminal::disable_raw_mode().wrap_err("failed to disable raw mode")?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            ResetColor,
            SetAttribute(Attribute::Reset),
            cursor::Show
        )
        .wrap_err("failed to restore terminal screen")?;

        self.terminal
            .backend_mut()
            .write_all(b"\x1b[0m\x1b(B\x1b[?25h")
            .wrap_err("failed to reset terminal state")?;
        self.terminal
            .backend_mut()
            .flush()
            .wrap_err("failed to flush terminal reset")?;
        self.restored = true;

        Ok(())
    }

    fn pick_session(&mut self, destination: &str, sessions: &[String]) -> Result<TmuxAction> {
        let mut selected = 0usize;
        let new_index = sessions.len();

        loop {
            self.terminal.draw(|frame| {
                draw_picker(frame, destination, sessions, selected);
            })?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(TmuxAction::Quit);
                    }
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(TmuxAction::Quit),
                    KeyCode::Char('n') => {
                        if let Some(name) = self.prompt_new_session_name("New tmux session")? {
                            return Ok(TmuxAction::New(name));
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = (selected + 1).min(new_index);
                    }
                    KeyCode::Enter => {
                        if selected == new_index {
                            if let Some(name) = self.prompt_new_session_name("New tmux session")? {
                                return Ok(TmuxAction::New(name));
                            }
                        } else if let Some(session) = sessions.get(selected) {
                            return Ok(TmuxAction::Attach(session.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn prompt_new_session_name(&mut self, title: &str) -> Result<Option<String>> {
        let mut input = String::new();
        let mut error: Option<&'static str> = None;

        loop {
            self.terminal.draw(|frame| {
                draw_name_prompt(frame, title, &input, error);
            })?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Enter => match validate_new_session_name(&input) {
                        Ok(()) => return Ok(Some(input)),
                        Err(message) => error = Some(message),
                    },
                    KeyCode::Backspace => {
                        input.pop();
                        error = None;
                    }
                    KeyCode::Char(ch) => {
                        input.push(ch);
                        error = None;
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn draw_picker(frame: &mut Frame<'_>, destination: &str, sessions: &[String], selected: usize) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            "tmussh",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(destination, Style::default().fg(Color::Cyan))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, chunks[0]);

    let mut items: Vec<ListItem<'_>> = sessions
        .iter()
        .map(|session| ListItem::new(Line::from(session.as_str())))
        .collect();
    items.push(ListItem::new(Line::from(vec![
        Span::styled("+", Style::default().fg(Color::Green)),
        Span::raw(" start new session"),
    ])));

    let mut state = ListState::default();
    state.select(Some(selected));

    let list = List::new(items)
        .block(Block::default().title("Sessions").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let help = Paragraph::new("Up/Down or j/k move | Enter attach | n new | Esc or q quit")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(help, chunks[2]);
}

fn draw_name_prompt(frame: &mut Frame<'_>, title: &str, input: &str, error: Option<&str>) {
    let area = centered_rect(frame.area(), 64, 9);
    frame.render_widget(Clear, area);

    let input_display = if input.is_empty() { " " } else { input };
    let mut lines = vec![
        Line::from("Session name"),
        Line::from(input_display),
        Line::from(""),
    ];

    if let Some(message) = error {
        lines.push(Line::from(Span::styled(
            message,
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(
            "Letters, numbers, underscores, periods, hyphens",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Enter create | Esc cancel"));

    let prompt = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(prompt, area);
}

fn centered_rect(area: Rect, max_width: u16, height: u16) -> Rect {
    let width = if area.width < 20 {
        area.width
    } else {
        area.width.min(max_width)
    };
    let height = if area.height < 7 {
        area.height
    } else {
        area.height.min(height)
    };

    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
