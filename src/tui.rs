#[cfg(feature = "tui")]
use anyhow::{Context, Result};

#[cfg(feature = "tui")]
use crate::agent::AgentTraceEntry;
#[cfg(feature = "tui")]
use crate::drift::DriftReport;
#[cfg(feature = "tui")]
use crate::model::{ArtifactManifest, CaseResult, RunMetadata, Summary};
#[cfg(feature = "tui")]
use crate::tooling::ToolTranscriptRecord;

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
pub fn launch(
    seed: u64,
    runs: u32,
    results: &[CaseResult],
    summary: &Summary,
    metadata: &RunMetadata,
    manifest: &ArtifactManifest,
) -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let pr_url = std::env::var("COGITATOR_PR_URL").ok();

    let pass_count = results.iter().filter(|result| result.passed).count();
    let fail_count = results.len().saturating_sub(pass_count);
    let entropy_sources = metadata.witnessed.entropy_sources.join(", ");

    loop {
        terminal.draw(|frame| {
            let size = frame.area();
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(7),
                ])
                .split(size);

            let title = Paragraph::new(Line::from(vec![
                Span::styled(
                    "Cogitator Run Summary",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  •  Seed {}", seed)),
            ]))
            .wrap(Wrap { trim: true });
            frame.render_widget(title, layout[0]);

            let summary_text = vec![
                Line::from(format!("Runs: {}", runs)),
                Line::from(format!("Pass rate: {:.2}%", summary.pass_rate * 100.0)),
                Line::from(format!("Average score: {:.3}", summary.avg_score)),
                Line::from(format!("Passed: {}  Failed: {}", pass_count, fail_count)),
                Line::from(format!(
                    "Entropy sources: {}",
                    if entropy_sources.is_empty() {
                        "none declared".to_string()
                    } else {
                        entropy_sources.clone()
                    }
                )),
                Line::from(format!(
                    "Trace schema: v{}  |  Parallel: {}",
                    metadata.witnessed.schema_version, metadata.witnessed.parallel
                )),
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

            let mut artifact_text = vec![
                Line::from("Artifacts:"),
                Line::from(format!("meta.json → {}", manifest.meta_json)),
                Line::from(format!("trace.jsonl → {}", manifest.trace_jsonl)),
                Line::from(format!("results.csv → {}", manifest.results_csv)),
                Line::from(format!("results.json → {}", manifest.results_json)),
                Line::from(format!("summary.json → {}", manifest.summary_json)),
                Line::from(format!("analysis.json → {}", manifest.analysis_json)),
                Line::from(format!("witness_root.txt → {}", manifest.witness_root_txt)),
            ];
            if let Some(path) = &manifest.nix_provenance_json {
                artifact_text.push(Line::from(format!("nix_provenance.json → {}", path)));
            }
            if let Some(path) = &manifest.agent_trace_json {
                artifact_text.push(Line::from(format!("agent_trace.json → {}", path)));
            }
            if let Some(path) = &manifest.tool_transcript_json {
                artifact_text.push(Line::from(format!("tool_transcript.json → {}", path)));
            }
            if let Some(path) = &manifest.witness_manifest_json {
                artifact_text.push(Line::from(format!("witness_manifest.json → {}", path)));
            }
            if let Some(path) = &manifest.hash_chain_txt {
                artifact_text.push(Line::from(format!("hash_chain.txt → {}", path)));
            }
            if let Some(path) = &manifest.drift_report_json {
                artifact_text.push(Line::from(format!("drift_report.json → {}", path)));
            }
            if let Some(path) = &manifest.chaos_profile_json {
                artifact_text.push(Line::from(format!("chaos_profile.json → {}", path)));
            }
            let artifact_block = Paragraph::new(artifact_text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Artifact bundle"),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(artifact_block, button_layout[0]);

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
pub fn launch_agent(
    agent_name: &str,
    run_id: u32,
    seed: u64,
    agent_trace: &[AgentTraceEntry],
    tool_transcript: &ToolTranscriptRecord,
    drift_report: &DriftReport,
    replay_mode: bool,
    manifest: &ArtifactManifest,
) -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let warning_style = if drift_report.drifted {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    loop {
        terminal.draw(|frame| {
            let size = frame.area();
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(8),
                ])
                .split(size);

            let mut title_spans = vec![
                Span::styled(
                    "Cogitator Agent Observatory",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  •  Agent {}  •  Run {}", agent_name, run_id)),
                Span::raw(format!("  •  Seed {}", seed)),
            ];
            if replay_mode {
                title_spans.push(Span::raw("  •  "));
                title_spans.push(Span::styled(
                    "REPLAY MODE",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if drift_report.drifted {
                title_spans.push(Span::raw("  •  "));
                title_spans.push(Span::styled("DRIFT DETECTED", warning_style));
            }

            let title = Paragraph::new(Line::from(title_spans)).wrap(Wrap { trim: true });
            frame.render_widget(title, layout[0]);

            let body_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(layout[1]);

            let mut timeline_lines = Vec::new();
            for entry in agent_trace {
                timeline_lines.push(Line::from(format!(
                    "step {}: {} | {}",
                    entry.step, entry.thought, entry.action
                )));
            }
            if timeline_lines.is_empty() {
                timeline_lines.push(Line::from("No agent steps recorded."));
            }

            let timeline_block = Paragraph::new(timeline_lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Agent step timeline"),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(timeline_block, body_layout[0]);

            let mut tool_lines = Vec::new();
            for call in &tool_transcript.entries {
                tool_lines.push(Line::from(format!(
                    "step {}: {} → {}",
                    call.step, call.request.tool_name, call.response.success
                )));
            }
            if tool_lines.is_empty() {
                tool_lines.push(Line::from("No tool calls recorded."));
            }

            let tool_block = Paragraph::new(tool_lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Tool call log"),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(tool_block, body_layout[1]);

            let mut artifact_text = vec![
                Line::from("Artifacts:"),
                Line::from(format!("meta.json → {}", manifest.meta_json)),
            ];
            if let Some(path) = &manifest.agent_trace_json {
                artifact_text.push(Line::from(format!("agent_trace.json → {}", path)));
            }
            if let Some(path) = &manifest.tool_transcript_json {
                artifact_text.push(Line::from(format!("tool_transcript.json → {}", path)));
            }
            if let Some(path) = &manifest.witness_manifest_json {
                artifact_text.push(Line::from(format!("witness_manifest.json → {}", path)));
            }
            if let Some(path) = &manifest.hash_chain_txt {
                artifact_text.push(Line::from(format!("hash_chain.txt → {}", path)));
            }
            if let Some(path) = &manifest.drift_report_json {
                artifact_text.push(Line::from(format!("drift_report.json → {}", path)));
            }
            if let Some(path) = &manifest.chaos_profile_json {
                artifact_text.push(Line::from(format!("chaos_profile.json → {}", path)));
            }
            artifact_text.push(Line::from("Press q to exit."));

            let artifact_block = Paragraph::new(artifact_text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Witness bundle"),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(artifact_block, layout[2]);
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => break,
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
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
