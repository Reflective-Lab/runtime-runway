// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Application State and Logic
//!
//! This module defines the core application state and business logic for the
//! Converge TUI. It manages:
//!
//! - Application state (jobs, contexts, agents)
//! - User input handling and navigation
//! - Job submission and monitoring
//! - View management and transitions

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::{ListState, TableState},
};
use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use converge_core::traits::DynChatBackend;
use converge_core::{Context, ContextKey, Engine};
use strum::IntoEnumIterator;

use crate::agents::{RiskAssessmentAgent, StrategicInsightAgent};
use crate::llm_backend;
use crate::packs;

pub type AppResult<T> = Result<T>;

/// Available views in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Jobs,
    JobDetail,
    Packs,
    Submit,
    Context,
    Agents,
}

impl View {
    pub fn all() -> Vec<View> {
        vec![
            View::Jobs,
            View::Packs,
            View::Submit,
            View::Context,
            View::Agents,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            View::Jobs => "Jobs",
            View::JobDetail => "Job Details",
            View::Packs => "Packs",
            View::Submit => "Submit",
            View::Context => "Context",
            View::Agents => "Agents",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub view: View,
    pub data_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Running,
    Converged,
    Failed,
    Paused,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Pending => "Pending",
            JobStatus::Running => "Running",
            JobStatus::Converged => "Converged",
            JobStatus::Failed => "Failed",
            JobStatus::Paused => "Paused",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: String,
    pub pack: String,
    pub status: JobStatus,
    pub cycles: u32,
    pub facts: usize,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PackInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub agents: Vec<String>,
    pub invariants: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub status: String,
    pub last_run: Option<String>,
    pub facts_produced: usize,
}

#[derive(Debug, Clone)]
pub struct FactInfo {
    pub key: String,
    pub id: String,
    pub content: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct JobDetail {
    pub info: JobInfo,
    pub facts: Vec<FactInfo>,
    pub agents: Vec<AgentInfo>,
    pub proposals: Vec<ProposalInfo>,
}

#[derive(Debug, Clone)]
pub struct ProposalInfo {
    pub id: String,
    pub agent: String,
    pub key: String,
    pub content: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SubmitForm {
    pub pack: String,
    pub seeds: String,
    pub max_cycles: String,
    pub selected_field: usize,
    pub error: Option<String>,
    pub success: Option<String>,
}

impl SubmitForm {
    pub fn new() -> Self {
        Self {
            pack: String::new(),
            seeds: String::new(),
            max_cycles: "50".to_string(),
            selected_field: 0,
            error: None,
            success: None,
        }
    }
}

/// Main application state
pub struct App {
    pub running: bool,
    pub current_view: View,
    pub breadcrumb: Vec<BreadcrumbSegment>,

    pub jobs: Vec<JobInfo>,
    pub job_state: TableState,
    pub job_detail: Option<JobDetail>,
    pub job_details_cache: std::collections::HashMap<String, JobDetail>,

    pub packs: Vec<PackInfo>,
    pub pack_state: ListState,

    pub submit_form: SubmitForm,

    pub context_facts: Vec<FactInfo>,
    pub fact_state: ListState,

    pub agents: Vec<AgentInfo>,
    pub agent_state: TableState,

    pub status_message: Option<String>,
    pub loading: bool,
}

impl App {
    pub fn new() -> Self {
        let mut job_state = TableState::default();
        job_state.select(Some(0));

        let mut pack_state = ListState::default();
        pack_state.select(Some(0));

        let mut fact_state = ListState::default();
        fact_state.select(Some(0));

        let mut agent_state = TableState::default();
        agent_state.select(Some(0));

        let mut app = Self {
            running: true,
            current_view: View::Jobs,
            breadcrumb: Vec::new(),
            jobs: Vec::new(),
            job_state,
            job_detail: None,
            job_details_cache: std::collections::HashMap::new(),
            packs: Vec::new(),
            pack_state,
            submit_form: SubmitForm::new(),
            context_facts: Vec::new(),
            fact_state,
            agents: Vec::new(),
            agent_state,
            status_message: None,
            loading: false,
        };
        app.update_breadcrumb();
        app.load_demo_data();
        app
    }

    fn load_demo_data(&mut self) {
        let available = packs::available_packs();
        self.packs = available
            .iter()
            .map(|name| {
                let info = packs::pack_info(name);
                PackInfo {
                    name: info.name,
                    version: info.version,
                    description: info.description,
                    agents: Vec::new(),
                    invariants: info.invariants,
                }
            })
            .collect();

        self.agents = vec![
            AgentInfo {
                name: "StrategicInsightAgent".to_string(),
                status: "Ready".to_string(),
                last_run: None,
                facts_produced: 0,
            },
            AgentInfo {
                name: "RiskAssessmentAgent".to_string(),
                status: "Ready".to_string(),
                last_run: None,
                facts_produced: 0,
            },
        ];

        self.jobs = Vec::new();
        self.context_facts = Vec::new();
    }

    pub fn update_breadcrumb(&mut self) {
        self.breadcrumb.clear();

        match self.current_view {
            View::Jobs => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Jobs".to_string(),
                    view: View::Jobs,
                    data_id: None,
                });
            }
            View::JobDetail => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Jobs".to_string(),
                    view: View::Jobs,
                    data_id: None,
                });
                if let Some(ref detail) = self.job_detail {
                    self.breadcrumb.push(BreadcrumbSegment {
                        label: detail.info.id.clone(),
                        view: View::JobDetail,
                        data_id: Some(detail.info.id.clone()),
                    });
                }
            }
            View::Packs => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Packs".to_string(),
                    view: View::Packs,
                    data_id: None,
                });
            }
            View::Submit => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Submit".to_string(),
                    view: View::Submit,
                    data_id: None,
                });
            }
            View::Context => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Context".to_string(),
                    view: View::Context,
                    data_id: None,
                });
            }
            View::Agents => {
                self.breadcrumb.push(BreadcrumbSegment {
                    label: "Agents".to_string(),
                    view: View::Agents,
                    data_id: None,
                });
            }
        }
    }

    pub fn next_view(&mut self) {
        let views = View::all();
        let current_idx = views
            .iter()
            .position(|v| *v == self.current_view)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % views.len();
        self.current_view = views[next_idx];
        self.update_breadcrumb();
    }

    pub fn prev_view(&mut self) {
        let views = View::all();
        let current_idx = views
            .iter()
            .position(|v| *v == self.current_view)
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            views.len() - 1
        } else {
            current_idx - 1
        };
        self.current_view = views[prev_idx];
        self.update_breadcrumb();
    }

    pub fn goto_view(&mut self, index: usize) {
        let views = View::all();
        if index < views.len() {
            self.current_view = views[index];
            self.update_breadcrumb();
        }
    }

    pub fn navigate_back(&mut self) {
        if self.breadcrumb.len() > 1 {
            self.breadcrumb.pop();
            if let Some(segment) = self.breadcrumb.last() {
                self.current_view = segment.view;
            }
        }
    }

    pub fn select_next(&mut self) {
        match self.current_view {
            View::Jobs => {
                let len = self.jobs.len();
                if len > 0 {
                    let i = self.job_state.selected().unwrap_or(0);
                    self.job_state.select(Some((i + 1) % len));
                }
            }
            View::Packs => {
                let len = self.packs.len();
                if len > 0 {
                    let i = self.pack_state.selected().unwrap_or(0);
                    self.pack_state.select(Some((i + 1) % len));
                }
            }
            View::Context => {
                let len = self.context_facts.len();
                if len > 0 {
                    let i = self.fact_state.selected().unwrap_or(0);
                    self.fact_state.select(Some((i + 1) % len));
                }
            }
            View::Agents => {
                let len = self.agents.len();
                if len > 0 {
                    let i = self.agent_state.selected().unwrap_or(0);
                    self.agent_state.select(Some((i + 1) % len));
                }
            }
            View::Submit => {
                self.submit_form.selected_field = (self.submit_form.selected_field + 1) % 3;
            }
            View::JobDetail => {}
        }
    }

    pub fn select_prev(&mut self) {
        match self.current_view {
            View::Jobs => {
                let len = self.jobs.len();
                if len > 0 {
                    let i = self.job_state.selected().unwrap_or(0);
                    self.job_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            View::Packs => {
                let len = self.packs.len();
                if len > 0 {
                    let i = self.pack_state.selected().unwrap_or(0);
                    self.pack_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            View::Context => {
                let len = self.context_facts.len();
                if len > 0 {
                    let i = self.fact_state.selected().unwrap_or(0);
                    self.fact_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            View::Agents => {
                let len = self.agents.len();
                if len > 0 {
                    let i = self.agent_state.selected().unwrap_or(0);
                    self.agent_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            View::Submit => {
                self.submit_form.selected_field = if self.submit_form.selected_field == 0 {
                    2
                } else {
                    self.submit_form.selected_field - 1
                };
            }
            View::JobDetail => {}
        }
    }

    pub fn handle_char(&mut self, c: char) {
        if self.current_view == View::Submit {
            let field = match self.submit_form.selected_field {
                0 => &mut self.submit_form.pack,
                1 => &mut self.submit_form.seeds,
                2 => &mut self.submit_form.max_cycles,
                _ => return,
            };
            field.push(c);
            self.submit_form.error = None;
        }
    }

    pub fn handle_backspace(&mut self) {
        if self.current_view == View::Submit {
            let field = match self.submit_form.selected_field {
                0 => &mut self.submit_form.pack,
                1 => &mut self.submit_form.seeds,
                2 => &mut self.submit_form.max_cycles,
                _ => return,
            };
            field.pop();
        }
    }

    pub fn enter_job_detail(&mut self) {
        if let Some(idx) = self.job_state.selected() {
            if let Some(job) = self.jobs.get(idx) {
                if let Some(detail) = self.job_details_cache.get(&job.id) {
                    self.job_detail = Some(detail.clone());
                    self.context_facts = detail.facts.clone();
                } else {
                    self.job_detail = Some(JobDetail {
                        info: job.clone(),
                        facts: Vec::new(),
                        agents: self.agents.clone(),
                        proposals: Vec::new(),
                    });
                }
                self.current_view = View::JobDetail;
                self.update_breadcrumb();
            }
        }
    }

    pub async fn submit_job(&mut self) {
        if self.submit_form.pack.is_empty() {
            self.submit_form.error = Some("Pack name is required".to_string());
            return;
        }

        if !self.packs.iter().any(|p| p.name == self.submit_form.pack) {
            self.submit_form.error = Some(format!("Pack '{}' not found", self.submit_form.pack));
            return;
        }

        let job_id = format!("job-{:03}", self.jobs.len() + 1);
        let pack_name = self.submit_form.pack.clone();
        let seeds_json = self.submit_form.seeds.clone();

        let mut context = Context::new();
        if !seeds_json.is_empty() {
            match serde_json::from_str::<Vec<crate::packs::SeedFact>>(&seeds_json) {
                Ok(seed_facts) => {
                    for seed in seed_facts {
                        if let Err(e) = context.add_input(ContextKey::Seeds, seed.id, seed.content)
                        {
                            self.submit_form.error = Some(format!("Failed to add seed: {e}"));
                            return;
                        }
                    }
                }
                Err(e) => {
                    self.submit_form.error = Some(format!("Invalid seeds JSON: {e}"));
                    return;
                }
            }
        }

        let mut engine = Engine::new();

        // Register generic LLM agents
        let llm_provider = create_chat_backend();
        engine.register_suggestor(StrategicInsightAgent::new(llm_provider.clone()));
        engine.register_suggestor(RiskAssessmentAgent::new(llm_provider));

        match engine.run(context).await {
            Ok(result) => {
                let total_facts: usize = ContextKey::iter()
                    .map(|key| result.context.get(key).len())
                    .sum();

                let status = if result.converged {
                    JobStatus::Converged
                } else {
                    JobStatus::Failed
                };

                let facts: Vec<FactInfo> = ContextKey::iter()
                    .flat_map(|key| {
                        result
                            .context
                            .get(key)
                            .iter()
                            .map(|fact| FactInfo {
                                key: format!("{:?}", fact.key()),
                                id: fact.id.clone(),
                                content: fact.content.clone(),
                                confidence: 1.0,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();

                self.context_facts.clone_from(&facts);

                let job = JobInfo {
                    id: job_id.clone(),
                    pack: pack_name.clone(),
                    status,
                    cycles: result.cycles,
                    facts: total_facts,
                    created_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                };

                let detail = JobDetail {
                    info: job.clone(),
                    facts: facts.clone(),
                    agents: self.agents.clone(),
                    proposals: Vec::new(),
                };

                self.job_details_cache
                    .insert(job_id.clone(), detail.clone());
                self.job_detail = Some(detail);
                self.jobs.insert(0, job);

                let status_msg = if result.converged {
                    format!(
                        "Job {} converged in {} cycles with {} facts",
                        job_id, result.cycles, total_facts
                    )
                } else {
                    format!(
                        "Job {} halted after {} cycles with {} facts",
                        job_id, result.cycles, total_facts
                    )
                };
                self.submit_form.success = Some(status_msg);
            }
            Err(e) => {
                self.jobs.insert(
                    0,
                    JobInfo {
                        id: job_id.clone(),
                        pack: pack_name,
                        status: JobStatus::Failed,
                        cycles: 0,
                        facts: 0,
                        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                    },
                );
                self.submit_form.error = Some(format!("Job failed: {e}"));
            }
        }

        self.submit_form.pack.clear();
        self.submit_form.seeds.clear();
        self.submit_form.max_cycles = "50".to_string();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Main event loop
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
) -> AppResult<()> {
    loop {
        terminal.draw(|f| super::views::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if app.current_view == View::JobDetail {
                                app.navigate_back();
                            } else if app.current_view == View::Submit
                                && !app.submit_form.pack.is_empty()
                            {
                                app.submit_form = SubmitForm::new();
                            } else {
                                app.running = false;
                            }
                        }
                        KeyCode::Tab | KeyCode::Right => {
                            app.next_view();
                        }
                        KeyCode::BackTab => {
                            app.prev_view();
                        }
                        KeyCode::Left => {
                            if app.current_view == View::JobDetail {
                                app.navigate_back();
                            } else {
                                app.prev_view();
                            }
                        }
                        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.goto_view(0);
                        }
                        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.goto_view(1);
                        }
                        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.goto_view(2);
                        }
                        KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.goto_view(3);
                        }
                        KeyCode::Char('5') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.goto_view(4);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.select_next();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.select_prev();
                        }
                        KeyCode::Enter => match app.current_view {
                            View::Jobs => {
                                app.enter_job_detail();
                            }
                            View::Submit => {
                                if app.submit_form.selected_field == 2 {
                                    app.submit_job().await;
                                } else {
                                    app.submit_form.selected_field += 1;
                                }
                            }
                            _ => {}
                        },
                        KeyCode::Char('b') => {
                            if app.breadcrumb.len() > 1 {
                                app.navigate_back();
                            }
                        }
                        KeyCode::Char(c) => {
                            app.handle_char(c);
                        }
                        KeyCode::Backspace => {
                            app.handle_backspace();
                        }
                        _ => {}
                    }
                }
            }
        }

        if !app.running {
            return Ok(());
        }
    }
}

/// Creates a chat backend from environment variables.
fn create_chat_backend() -> Arc<dyn DynChatBackend> {
    llm_backend::create_chat_backend_or_mock()
}
