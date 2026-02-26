pub mod app;
pub mod theme;
pub mod ui;

use std::{
	collections::{BTreeMap, BTreeSet, HashMap},
	io,
};

use anyhow::{Context, bail};
use crossterm::{
	event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
	execute,
	terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use self::app::{App, InputMode, MessageEntry, Pane, SessionEntry};
use crate::{index::IndexHandle, search};

pub fn run_tui(query: Option<String>) -> anyhow::Result<()> {
	// Terminal setup
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// Install a panic hook that restores the terminal before printing the panic
	// message.
	let original_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic_info| {
		let _ = crossterm::terminal::disable_raw_mode();
		let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
		original_hook(panic_info);
	}));

	// Open the index once for the lifetime of the TUI.
	let handle = IndexHandle::open()?;

	// Create app state
	let mut app = App::new(query);

	// If there's an initial query, execute the search
	if !app.query.is_empty() {
		execute_search(&handle, &mut app)?;
	} else {
		load_all_sessions(&handle, &mut app)?;
	}

	// Load messages for the initially selected session
	if !app.sessions.is_empty() {
		let _ = load_session_messages(&handle, &mut app);
	}

	// Main loop
	let result = run_event_loop(&handle, &mut terminal, &mut app);

	// Terminal teardown
	disable_raw_mode()?;
	execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
	terminal.show_cursor()?;

	result
}

fn run_event_loop(
	handle: &IndexHandle,
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	app: &mut App,
) -> anyhow::Result<()> {
	loop {
		terminal.draw(|f| ui(f, &mut *app))?;

		if !app.running {
			break;
		}

		// Poll for events with 100ms timeout
		if event::poll(std::time::Duration::from_millis(100))?
			&& let Event::Key(key) = event::read()?
		{
			// Only handle key press events (not release/repeat)
			if key.kind != KeyEventKind::Press {
				continue;
			}

			// Ctrl+c always quits
			if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
				app.running = false;
				continue;
			}

			// Ctrl+d / Ctrl+u for half-page scroll in normal mode
			if app.input_mode == InputMode::Normal && key.modifiers.contains(KeyModifiers::CONTROL) {
				match key.code {
					KeyCode::Char('d') => {
						page_down(handle, app);
						continue;
					},
					KeyCode::Char('u') => {
						page_up(handle, app);
						continue;
					},
					_ => {},
				}
			}

			match app.input_mode {
				InputMode::Normal => handle_normal_key(handle, app, key.code)?,
				InputMode::Search => handle_search_key(handle, app, key.code)?,
				InputMode::FilterMessages => handle_filter_messages_key(app, key.code),
				InputMode::FilterProject | InputMode::FilterBranch => {
					handle_filter_key(handle, app, key.code)?;
				},
				InputMode::Help => {
					// Any key dismisses help
					app.input_mode = InputMode::Normal;
				},
			}
		}

		// Debounced search: if input changed and 300ms elapsed since last keystroke
		if app.search_dirty && app.last_keystroke.elapsed() > std::time::Duration::from_millis(300) {
			app.query = app.search_input.clone();
			if app.query.is_empty() {
				load_all_sessions(handle, app)?;
			} else {
				execute_search(handle, app)?;
			}
			app.search_dirty = false;
		}
	}
	Ok(())
}

fn handle_normal_key(handle: &IndexHandle, app: &mut App, code: KeyCode) -> anyhow::Result<()> {
	match code {
		KeyCode::Char('q') => {
			app.running = false;
		},
		KeyCode::Char('/') => {
			if app.active_pane == Pane::Sessions || app.messages.is_empty() {
				app.input_mode = InputMode::Search;
			} else {
				// In conversation mode: filter messages
				app.input_mode = InputMode::FilterMessages;
				app.message_filter.clear();
			}
		},
		KeyCode::Char('j') | KeyCode::Down => {
			navigate_down(handle, app);
		},
		KeyCode::Char('k') | KeyCode::Up => {
			navigate_up(handle, app);
		},
		KeyCode::PageDown => {
			page_down(handle, app);
		},
		KeyCode::PageUp => {
			page_up(handle, app);
		},
		KeyCode::Tab => {
			app.active_pane = match app.active_pane {
				Pane::Sessions => Pane::Messages,
				Pane::Messages => Pane::Preview,
				Pane::Preview => Pane::Sessions,
			};
		},
		KeyCode::BackTab => {
			app.active_pane = match app.active_pane {
				Pane::Sessions => Pane::Preview,
				Pane::Messages => Pane::Sessions,
				Pane::Preview => Pane::Messages,
			};
		},
		KeyCode::Enter => {
			handle_enter(handle, app)?;
		},
		KeyCode::Char('g') if app.active_pane == Pane::Preview => {
			app.preview_scroll = 0;
		},
		KeyCode::Char('G') if app.active_pane == Pane::Preview => {
			app.preview_scroll = u16::MAX;
		},
		KeyCode::Char('n') => {
			if let Some(idx) = app.next_hit() {
				app.message_index = idx;
				app.preview_scroll = 0;
				app.active_pane = Pane::Messages;
			}
		},
		KeyCode::Char('N') => {
			if let Some(idx) = app.prev_hit() {
				app.message_index = idx;
				app.preview_scroll = 0;
				app.active_pane = Pane::Messages;
			}
		},
		KeyCode::Char('m') => {
			app.messages_maximized = !app.messages_maximized;
		},
		KeyCode::Char('f') => {
			app.input_mode = InputMode::FilterProject;
			app.filter_index = 0;
		},
		KeyCode::Char('b') => {
			app.input_mode = InputMode::FilterBranch;
			app.filter_index = 0;
		},
		KeyCode::Char('y') => {
			if let Some(msg) = app.selected_message() {
				copy_to_clipboard(&msg.content);
			}
		},
		KeyCode::Char('e') => {
			if let Some(session) = app.selected_session()
				&& let Ok(path) = find_session_file(&session.session_id)
			{
				let _ = open_in_editor(path);
			}
		},
		KeyCode::Char('?') => {
			app.input_mode = InputMode::Help;
		},
		KeyCode::Esc => {
			app.active_pane = Pane::Sessions;
		},
		_ => {},
	}
	Ok(())
}

fn handle_search_key(handle: &IndexHandle, app: &mut App, code: KeyCode) -> anyhow::Result<()> {
	match code {
		KeyCode::Enter => {
			app.query = app.search_input.clone();
			app.input_mode = InputMode::Normal;
			app.search_dirty = false;
			if app.query.is_empty() {
				load_all_sessions(handle, app)?;
			} else {
				execute_search(handle, app)?;
			}
		},
		KeyCode::Esc => {
			app.input_mode = InputMode::Normal;
		},
		KeyCode::Char(c) => {
			app.search_input.push(c);
			app.search_dirty = true;
			app.last_keystroke = std::time::Instant::now();
		},
		KeyCode::Backspace => {
			app.search_input.pop();
			app.search_dirty = true;
			app.last_keystroke = std::time::Instant::now();
		},
		_ => {},
	}
	Ok(())
}

fn handle_filter_messages_key(app: &mut App, code: KeyCode) {
	match code {
		KeyCode::Char(c) => {
			app.message_filter.push(c);
			apply_message_filter(app);
		},
		KeyCode::Backspace => {
			app.message_filter.pop();
			apply_message_filter(app);
		},
		KeyCode::Enter => {
			// Keep the filtered results, return to normal mode
			app.input_mode = InputMode::Normal;
		},
		KeyCode::Esc => {
			// Cancel: restore all messages
			app.message_filter.clear();
			if !app.all_messages.is_empty() {
				app.messages = std::mem::take(&mut app.all_messages);
				app.message_index = 0;
				app.preview_scroll = 0;
			}
			app.input_mode = InputMode::Normal;
		},
		_ => {},
	}
}

fn apply_message_filter(app: &mut App) {
	// Save unfiltered messages on first filter keystroke
	if app.all_messages.is_empty() && !app.messages.is_empty() {
		app.all_messages = app.messages.clone();
	}

	if app.message_filter.is_empty() {
		// Restore all messages
		app.messages = app.all_messages.clone();
	} else {
		let filter_lower = app.message_filter.to_lowercase();
		app.messages = app
			.all_messages
			.iter()
			.filter(|m| {
				m.content.to_lowercase().contains(&filter_lower)
					|| m.role.to_lowercase().contains(&filter_lower)
			})
			.cloned()
			.collect();
	}
	app.message_index = 0;
	app.preview_scroll = 0;
}

fn navigate_down(handle: &IndexHandle, app: &mut App) {
	match app.active_pane {
		Pane::Sessions => {
			if app.session_index + 1 < app.sessions.len() {
				app.session_index += 1;
				let _ = load_session_messages(handle, app);
			}
		},
		Pane::Messages => {
			if app.message_index + 1 < app.messages.len() {
				app.message_index += 1;
				app.preview_scroll = 0;
			}
		},
		Pane::Preview => {
			app.preview_scroll = app.preview_scroll.saturating_add(1);
		},
	}
}

fn navigate_up(handle: &IndexHandle, app: &mut App) {
	match app.active_pane {
		Pane::Sessions => {
			if app.session_index > 0 {
				app.session_index = app.session_index.saturating_sub(1);
				let _ = load_session_messages(handle, app);
			}
		},
		Pane::Messages => {
			if app.message_index > 0 {
				app.message_index = app.message_index.saturating_sub(1);
				app.preview_scroll = 0;
			}
		},
		Pane::Preview => {
			app.preview_scroll = app.preview_scroll.saturating_sub(1);
		},
	}
}

fn page_down(handle: &IndexHandle, app: &mut App) {
	let half = (app.visible_rows / 2).max(1) as usize;
	match app.active_pane {
		Pane::Sessions => {
			if !app.sessions.is_empty() {
				app.session_index =
					(app.session_index + half).min(app.sessions.len().saturating_sub(1));
				let _ = load_session_messages(handle, app);
			}
		},
		Pane::Messages => {
			if !app.messages.is_empty() {
				app.message_index =
					(app.message_index + half).min(app.messages.len().saturating_sub(1));
				app.preview_scroll = 0;
			}
		},
		Pane::Preview => {
			app.preview_scroll = app.preview_scroll.saturating_add(half as u16);
		},
	}
}

fn page_up(handle: &IndexHandle, app: &mut App) {
	let half = (app.visible_rows / 2).max(1) as usize;
	match app.active_pane {
		Pane::Sessions => {
			app.session_index = app.session_index.saturating_sub(half);
			let _ = load_session_messages(handle, app);
		},
		Pane::Messages => {
			app.message_index = app.message_index.saturating_sub(half);
			app.preview_scroll = 0;
		},
		Pane::Preview => {
			app.preview_scroll = app.preview_scroll.saturating_sub(half as u16);
		},
	}
}

fn handle_enter(handle: &IndexHandle, app: &mut App) -> anyhow::Result<()> {
	match app.active_pane {
		Pane::Sessions => {
			if app.selected_session().is_some() {
				load_session_messages(handle, app)?;
				app.active_pane = Pane::Messages;
				app.message_index = 0;
				app.preview_scroll = 0;
			}
		},
		Pane::Messages => {
			app.active_pane = Pane::Preview;
		},
		Pane::Preview => {},
	}
	Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
	ui::draw(f, app);
}

fn execute_search(handle: &IndexHandle, app: &mut App) -> anyhow::Result<()> {
	let hits = search::search_hits(handle, &app.query, 200)?;

	// Collect hit timestamps per session for marking matched messages.
	let mut hit_timestamps: HashMap<String, Vec<String>> = HashMap::new();
	for hit in &hits {
		hit_timestamps
			.entry(hit.session_id.clone())
			.or_default()
			.push(hit.timestamp.clone());
	}

	// Group hits by session.
	let mut session_map: BTreeMap<String, SessionEntry> = BTreeMap::new();
	for hit in &hits {
		let entry = session_map
			.entry(hit.session_id.clone())
			.or_insert_with(|| SessionEntry {
				session_id: hit.session_id.clone(),
				project_name: hit.project_name.clone(),
				git_branch: hit.git_branch.clone(),
				hit_count: 0,
				best_score: 0.0,
				latest_timestamp: hit.timestamp.clone(),
			});
		entry.hit_count += 1;
		if hit.score > entry.best_score {
			entry.best_score = hit.score;
		}
		if hit.timestamp > entry.latest_timestamp {
			entry.latest_timestamp = hit.timestamp.clone();
		}
	}

	let mut sessions: Vec<SessionEntry> = session_map.into_values().collect();
	sessions.sort_by(|a, b| {
		b.best_score
			.partial_cmp(&a.best_score)
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	app.sessions = sessions;
	app.session_index = 0;
	app.messages.clear();
	app.message_index = 0;
	app.preview_scroll = 0;
	app.hit_timestamps = hit_timestamps;

	collect_filter_options(app);

	Ok(())
}

fn load_all_sessions(handle: &IndexHandle, app: &mut App) -> anyhow::Result<()> {
	let all = search::all_sessions(handle)?;
	app.sessions = all
		.into_iter()
		.map(|(sid, pname, branch, ts, count)| SessionEntry {
			session_id: sid,
			project_name: pname,
			git_branch: branch,
			hit_count: count,
			best_score: 0.0,
			latest_timestamp: ts,
		})
		.collect();
	app.session_index = 0;
	app.messages.clear();
	app.message_index = 0;
	app.preview_scroll = 0;
	app.hit_timestamps.clear();

	collect_filter_options(app);

	Ok(())
}

fn handle_filter_key(handle: &IndexHandle, app: &mut App, code: KeyCode) -> anyhow::Result<()> {
	let list_len = match app.input_mode {
		InputMode::FilterProject => app.available_projects.len(),
		InputMode::FilterBranch => app.available_branches.len(),
		_ => 0,
	};

	match code {
		KeyCode::Char('j') | KeyCode::Down => {
			if app.filter_index + 1 < list_len {
				app.filter_index += 1;
			}
		},
		KeyCode::Char('k') | KeyCode::Up => {
			if app.filter_index > 0 {
				app.filter_index = app.filter_index.saturating_sub(1);
			}
		},
		KeyCode::Enter => {
			match app.input_mode {
				InputMode::FilterProject => {
					if let Some(project) = app.available_projects.get(app.filter_index).cloned() {
						app.filter_project = Some(project);
					}
				},
				InputMode::FilterBranch => {
					if let Some(branch) = app.available_branches.get(app.filter_index).cloned() {
						app.filter_branch = Some(branch);
					}
				},
				_ => {},
			}
			app.input_mode = InputMode::Normal;
			apply_filters(handle, app)?;
		},
		KeyCode::Char('d') => {
			match app.input_mode {
				InputMode::FilterProject => app.filter_project = None,
				InputMode::FilterBranch => app.filter_branch = None,
				_ => {},
			}
			app.input_mode = InputMode::Normal;
			apply_filters(handle, app)?;
		},
		KeyCode::Esc => {
			app.input_mode = InputMode::Normal;
		},
		_ => {},
	}
	Ok(())
}

fn apply_filters(handle: &IndexHandle, app: &mut App) -> anyhow::Result<()> {
	// Re-load base data
	if app.query.is_empty() {
		load_all_sessions(handle, app)?;
	} else {
		execute_search(handle, app)?;
	}

	// Apply project filter
	if let Some(ref project) = app.filter_project.clone() {
		app.sessions.retain(|s| s.project_name == *project);
	}

	// Apply branch filter
	if let Some(ref branch) = app.filter_branch.clone() {
		app.sessions.retain(|s| s.git_branch == *branch);
	}

	Ok(())
}

fn collect_filter_options(app: &mut App) {
	app.available_projects = app
		.sessions
		.iter()
		.map(|s| s.project_name.clone())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	app.available_branches = app
		.sessions
		.iter()
		.map(|s| s.git_branch.clone())
		.filter(|b| !b.is_empty())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
}

/// Find the JSONL file for a session ID by scanning ~/.claude/projects/.
fn find_session_file(session_id: &str) -> anyhow::Result<std::path::PathBuf> {
	let claude_dir = dirs::home_dir()
		.context("could not determine home directory")?
		.join(".claude")
		.join("projects");
	if !claude_dir.is_dir() {
		bail!("projects directory not found: {}", claude_dir.display());
	}
	let filename = format!("{session_id}.jsonl");
	for entry in std::fs::read_dir(&claude_dir)
		.with_context(|| format!("failed to read {}", claude_dir.display()))?
	{
		let entry = entry?;
		if entry.file_type()?.is_dir() {
			let candidate = entry.path().join(&filename);
			if candidate.exists() {
				return Ok(candidate);
			}
		}
	}
	bail!("session file not found for {session_id}");
}

/// Open a file in $EDITOR, suspending the TUI while the editor runs.
fn open_in_editor(path: std::path::PathBuf) -> anyhow::Result<()> {
	let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

	// Temporarily restore the terminal for the editor
	disable_raw_mode()?;
	execute!(io::stdout(), LeaveAlternateScreen)?;

	let status = std::process::Command::new(&editor).arg(&path).status();

	// Re-enter TUI mode
	enable_raw_mode()?;
	execute!(io::stdout(), EnterAlternateScreen)?;

	match status {
		Ok(s) if s.success() => Ok(()),
		Ok(s) => anyhow::bail!("editor exited with status: {s}"),
		Err(e) => anyhow::bail!("failed to launch editor '{editor}': {e}"),
	}
}

/// Copy text to the system clipboard using the OSC 52 terminal escape sequence.
fn copy_to_clipboard(text: &str) {
	use base64::Engine;
	let encoded = base64::engine::general_purpose::STANDARD.encode(text);
	// OSC 52: \x1b]52;c;<base64>\x07
	let _ = execute!(io::stdout(), crossterm::style::Print(format!("\x1b]52;c;{encoded}\x07")));
}

fn load_session_messages(handle: &IndexHandle, app: &mut App) -> anyhow::Result<()> {
	let session_id = match app.selected_session() {
		Some(s) => s.session_id.clone(),
		None => return Ok(()),
	};
	let hit_ts = app.hit_timestamps.get(&session_id);
	let msgs = search::session_messages(handle, &session_id)?;
	app.messages = msgs
		.into_iter()
		.map(|(ts, role, content)| {
			let is_hit = hit_ts.is_some_and(|timestamps| timestamps.contains(&ts));
			MessageEntry { timestamp: ts, role, content, is_hit }
		})
		.collect();
	app.all_messages.clear();
	app.message_filter.clear();
	app.message_index = 0;
	app.preview_scroll = 0;

	Ok(())
}
