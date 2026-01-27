#[cfg(feature = "tui")]
use anyhow::{Context, Result};

#[cfg(feature = "tui")]
use crate::model::{CaseResult, Summary};

#[cfg(feature = "tui")]
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

#[cfg(feature = "tui")]
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[cfg(feature = "tui")]
pub fn launch(seed: u64, runs: u32, results: &[CaseResult], summary: &Summary) -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let pr_url = std::env::var("COGITATOR_PR_URL").ok();

    let pass_count = results.iter().filter(|result| result.passed).count();
    let fail_count = results.len().saturating_sub(pass_count);

    loop {
        terminal.draw(|frame| {
            let size = frame.size();
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(6),
                    Constraint::Length(3),
                ])
                .split(size);

            let title = Paragraph::new(Line::from(vec![
                Span::styled("Cogitator Run Summary", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("  •  Seed {}", seed)),
            ]))
            .wrap(Wrap { trim: true });
            frame.render_widget(title, layout[0]);

            let summary_text = vec![
                Line::from(format!("Runs: {}", runs)),
                Line::from(format!("Pass rate: {:.2}%", summary.pass_rate * 100.0)),
                Line::from(format!("Average score: {:.3}", summary.avg_score)),
                Line::from(format!("Passed: {}  Failed: {}", pass_count, fail_count)),
                Line::from(""),
                Line::from(match pr_url.as_deref() {
                    Some(url) => format!("Pull request: {}", url),
                    None => "Pull request: not created yet (set COGITATOR_PR_URL)".to_string(),
                }),
                Line::from(""),
                Line::from("Press q to exit. Press p to copy/view PR link when available."),
            ];

            let summary_block = Paragraph::new(summary_text)
                .block(Block::default().borders(Borders::ALL).title("Summary"))
                .wrap(Wrap { trim: true });
            frame.render_widget(summary_block, layout[1]);

            let button_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layout[2]);

            let ask_button = Block::default()
                .borders(Borders::ALL)
                .title("Ask for changes");
            frame.render_widget(ask_button, button_layout[0]);

            let view_pr_title = if pr_url.is_some() {
                "View PR"
            } else {
                "View PR (disabled)"
            };
            let view_button = Block::default().borders(Borders::ALL).title(view_pr_title);
            frame.render_widget(view_button, button_layout[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => break,
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('p'), _) => {
                        if let Some(url) = pr_url.as_deref() {
                            let _ = copy_to_clipboard(url);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")?;

    Ok(())
}

#[cfg(feature = "tui")]
fn copy_to_clipboard(url: &str) -> Result<()> {
    if std::env::var("SSH_CONNECTION").is_ok() {
        return Ok(());
    }
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v pbcopy >/dev/null && pbcopy || command -v xclip >/dev/null && xclip -selection clipboard || command -v wl-copy >/dev/null && wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("spawn clipboard helper")?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(url.as_bytes())?;
    }
    let _ = child.wait();
    Ok(())
}
