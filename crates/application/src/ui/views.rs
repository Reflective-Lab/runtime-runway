//! UI Rendering Functions
//!
//! This module contains all the rendering functions for the Converge TUI.
//! It handles the visual presentation of:
//!
//! - Jobs list with status
//! - Job detail with context and agents
//! - Packs list with descriptions
//! - Submit form
//! - Context facts visualization
//! - Suggestor status display

use super::app::{App, JobStatus, View};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
};

/// Main draw function - renders the entire UI
pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs
            Constraint::Length(1), // Breadcrumb
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(f.area());

    draw_tabs(f, app, chunks[0]);
    draw_breadcrumb(f, app, chunks[1]);
    draw_main(f, app, chunks[2]);
    draw_status_bar(f, app, chunks[3]);
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = View::all()
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let num = format!("[{}] ", i + 1);
            let style = if *v == app.current_view
                || (app.current_view == View::JobDetail && *v == View::Jobs)
            {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(vec![
                Span::styled(num, Style::default().fg(Color::DarkGray)),
                Span::styled(v.title(), style),
            ])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Converge  [Tab or Ctrl+1-5 to switch] "),
        )
        .highlight_style(Style::default().fg(Color::Yellow))
        .select(
            View::all()
                .iter()
                .position(|v| {
                    *v == app.current_view
                        || (app.current_view == View::JobDetail && *v == View::Jobs)
                })
                .unwrap_or(0),
        );

    f.render_widget(tabs, area);
}

fn draw_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    if app.breadcrumb.is_empty() {
        return;
    }

    let mut spans = Vec::new();

    for (i, segment) in app.breadcrumb.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" > ", Style::default().fg(Color::DarkGray)));
        }

        let style = if i == app.breadcrumb.len() - 1 {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };

        spans.push(Span::styled(segment.label.clone(), style));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(paragraph, area);
}

fn draw_main(f: &mut Frame, app: &mut App, area: Rect) {
    match app.current_view {
        View::Jobs => draw_jobs(f, app, area),
        View::JobDetail => draw_job_detail(f, app, area),
        View::Packs => draw_packs(f, app, area),
        View::Submit => draw_submit(f, app, area),
        View::Context => draw_context(f, app, area),
        View::Agents => draw_agents(f, app, area),
    }
}

fn draw_jobs(f: &mut Frame, app: &mut App, area: Rect) {
    let selected_idx = app.job_state.selected().unwrap_or(0);
    let total = app.jobs.len();

    let title = format!(
        " Jobs ({}/{}) [Enter to view details] ",
        selected_idx + 1,
        total
    );

    let header = Row::new(vec![
        Cell::from("ID").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Pack").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Status").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Cycles").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Facts").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Created").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let selected = app.job_state.selected() == Some(i);
            let row_style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };

            let status_style = match job.status {
                JobStatus::Converged => Style::default().fg(Color::Green),
                JobStatus::Running => Style::default().fg(Color::Yellow),
                JobStatus::Failed => Style::default().fg(Color::Red),
                JobStatus::Paused => Style::default().fg(Color::Magenta),
                JobStatus::Pending => Style::default().fg(Color::Gray),
            };

            let prefix = if selected { "▶ " } else { "  " };

            Row::new(vec![
                Cell::from(format!("{}{}", prefix, job.id)).style(row_style),
                Cell::from(job.pack.clone()).style(row_style),
                Cell::from(job.status.as_str()).style(if selected {
                    row_style
                } else {
                    status_style
                }),
                Cell::from(format!("{}", job.cycles)).style(row_style),
                Cell::from(format!("{}", job.facts)).style(row_style),
                Cell::from(job.created_at.clone()).style(row_style),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title));

    f.render_stateful_widget(table, area, &mut app.job_state);
}

fn draw_job_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(ref detail) = app.job_detail else {
        let msg = Paragraph::new("No job selected")
            .block(Block::default().borders(Borders::ALL).title(" Job Detail "));
        f.render_widget(msg, area);
        return;
    };

    // Split into left (info + facts) and right (agents + proposals)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left side: Info + Facts
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(chunks[0]);

    // Job info
    let info_text = vec![
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &detail.info.id,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Pack: ", Style::default().fg(Color::Gray)),
            Span::styled(&detail.info.pack, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                detail.info.status.as_str(),
                Style::default().fg(match detail.info.status {
                    JobStatus::Converged => Color::Green,
                    JobStatus::Running => Color::Yellow,
                    JobStatus::Failed => Color::Red,
                    _ => Color::White,
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cycles: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", detail.info.cycles),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Facts: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", detail.info.facts),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    let info_para =
        Paragraph::new(info_text).block(Block::default().borders(Borders::ALL).title(" Job Info "));
    f.render_widget(info_para, left_chunks[0]);

    // Facts
    let fact_items: Vec<ListItem> = detail
        .facts
        .iter()
        .map(|fact| {
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        &fact.key,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" [{}]", fact.id),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(Span::styled(
                    &fact.content,
                    Style::default().fg(Color::White),
                )),
            ])
        })
        .collect();

    let facts_list = List::new(fact_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Facts ({}) ", detail.facts.len())),
    );
    f.render_widget(facts_list, left_chunks[1]);

    // Right side: Agents + Proposals
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Agents
    let agent_items: Vec<ListItem> = detail
        .agents
        .iter()
        .map(|agent| {
            let status_color = if agent.status == "Running" {
                Color::Yellow
            } else {
                Color::Green
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        &agent.name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" [{}]", agent.status),
                        Style::default().fg(status_color),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Facts produced: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{}", agent.facts_produced),
                        Style::default().fg(Color::White),
                    ),
                ]),
            ])
        })
        .collect();

    let agents_list = List::new(agent_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Agents ({}) ", detail.agents.len())),
    );
    f.render_widget(agents_list, right_chunks[0]);

    // Proposals
    let proposal_items: Vec<ListItem> = detail
        .proposals
        .iter()
        .map(|prop| {
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        &prop.key,
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" by {}", prop.agent),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(Span::styled(
                    &prop.content,
                    Style::default().fg(Color::White),
                )),
                Line::from(vec![
                    Span::styled("Confidence: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{:.0}%", prop.confidence * 100.0),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
            ])
        })
        .collect();

    let proposals_list =
        List::new(proposal_items).block(Block::default().borders(Borders::ALL).title(format!(
            " Proposals ({}) [y/n to approve/reject] ",
            detail.proposals.len()
        )));
    f.render_widget(proposals_list, right_chunks[1]);
}

fn draw_packs(f: &mut Frame, app: &mut App, area: Rect) {
    let selected_idx = app.pack_state.selected().unwrap_or(0);
    let total = app.packs.len();

    // Split into list and detail
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Pack list
    let items: Vec<ListItem> = app
        .packs
        .iter()
        .enumerate()
        .map(|(i, pack)| {
            let selected = app.pack_state.selected() == Some(i);
            let style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{}{}", prefix, pack.name),
                    style.add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  v{}", pack.version),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
        " Packs ({}/{}) ",
        selected_idx + 1,
        total
    )));
    f.render_stateful_widget(list, chunks[0], &mut app.pack_state);

    // Pack detail
    if let Some(pack) = app.packs.get(selected_idx) {
        let detail_text = vec![
            Line::from(vec![
                Span::styled(
                    &pack.name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" v{}", pack.version),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                &pack.description,
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Agents:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
        ];

        let mut lines = detail_text;
        for agent in &pack.agents {
            lines.push(Line::from(Span::styled(
                format!("  - {agent}"),
                Style::default().fg(Color::White),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Invariants:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for inv in &pack.invariants {
            lines.push(Line::from(Span::styled(
                format!("  - {inv}"),
                Style::default().fg(Color::Green),
            )));
        }

        let detail = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Pack Details "),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(detail, chunks[1]);
    }
}

fn draw_submit(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Pack field
            Constraint::Length(5), // Seeds field
            Constraint::Length(3), // Max cycles field
            Constraint::Length(3), // Status/error
            Constraint::Min(0),    // Help
        ])
        .split(area);

    let form = &app.submit_form;

    // Pack field
    let pack_style = if form.selected_field == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let pack_input = Paragraph::new(form.pack.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Pack ")
            .border_style(pack_style),
    );
    f.render_widget(pack_input, chunks[0]);

    // Seeds field
    let seeds_style = if form.selected_field == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let seeds_input = Paragraph::new(form.seeds.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Seeds (JSON) ")
                .border_style(seeds_style),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(seeds_input, chunks[1]);

    // Max cycles field
    let cycles_style = if form.selected_field == 2 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let cycles_input = Paragraph::new(form.max_cycles.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Max Cycles ")
            .border_style(cycles_style),
    );
    f.render_widget(cycles_input, chunks[2]);

    // Status/error
    let status_text = if let Some(ref err) = form.error {
        Span::styled(err, Style::default().fg(Color::Red))
    } else if let Some(ref success) = form.success {
        Span::styled(success, Style::default().fg(Color::Green))
    } else {
        Span::styled("", Style::default())
    };
    let status = Paragraph::new(status_text).block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[3]);

    // Help
    let help = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Available packs:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "    - growth-strategy",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            "    - sdr-pipeline",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  ↑/↓: Navigate fields  Enter: Submit  Esc: Clear",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Submit Job "));
    f.render_widget(help, chunks[4]);
}

fn draw_context(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .context_facts
        .iter()
        .enumerate()
        .map(|(i, fact)| {
            let selected = app.fact_state.selected() == Some(i);
            let style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{}{}", prefix, fact.key),
                        style.add_modifier(Modifier::BOLD).fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!(" [{}]", fact.id),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(Span::styled(format!("  {}", fact.content), style)),
                Line::from(vec![
                    Span::styled("  Confidence: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{:.0}%", fact.confidence * 100.0),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
            ])
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Context Facts ({}) ", app.context_facts.len())),
    );
    f.render_stateful_widget(list, area, &mut app.fact_state);
}

fn draw_agents(f: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Suggestor").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Status").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Last Run").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Facts").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let selected = app.agent_state.selected() == Some(i);
            let row_style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };

            let status_style = if agent.status == "Running" {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            };

            let prefix = if selected { "▶ " } else { "  " };

            Row::new(vec![
                Cell::from(format!("{}{}", prefix, agent.name)).style(row_style),
                Cell::from(agent.status.clone()).style(if selected {
                    row_style
                } else {
                    status_style
                }),
                Cell::from(agent.last_run.clone().unwrap_or_else(|| "-".to_string()))
                    .style(row_style),
                Cell::from(format!("{}", agent.facts_produced)).style(row_style),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(25),
            Constraint::Length(12),
            Constraint::Length(15),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Agents ({}) ", app.agents.len())),
    );

    f.render_stateful_widget(table, area, &mut app.agent_state);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let help_text = match app.current_view {
        View::Jobs => " ↑/↓:Select  Enter:Details  Tab:Switch view  q:Quit ",
        View::JobDetail => " b:Back  ←:Back  y/n:Approve/Reject proposal  q:Quit ",
        View::Packs | View::Context | View::Agents => " ↑/↓:Select  Tab:Switch view  q:Quit ",
        View::Submit => " ↑/↓:Fields  Enter:Submit  Esc:Clear  Tab:Switch view ",
    };

    let status = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(status, area);
}
