use ratatui::{
	Frame,
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::app::{App, InputMode, Pane};

// ---------------------------------------------------------------------------
// Border helpers
// ---------------------------------------------------------------------------

fn pane_border_style(app: &App, pane: Pane) -> Style {
	if app.active_pane == pane {
		Style::default().fg(Color::Cyan)
	} else {
		Style::default().fg(Color::DarkGray)
	}
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn draw(f: &mut Frame, app: &mut App) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(3), // search bar
			Constraint::Min(1),    // main area
			Constraint::Length(1), // status bar
		])
		.split(f.area());

	// Track visible rows for half-page scrolling (subtract borders)
	app.visible_rows = chunks[1].height.saturating_sub(2);

	draw_search_bar(f, app, chunks[0]);
	draw_main(f, app, chunks[1]);
	draw_status_bar(f, app, chunks[2]);

	// Render filter popup overlay if in filter mode
	if matches!(app.input_mode, InputMode::FilterProject | InputMode::FilterBranch) {
		draw_filter_popup(f, app);
	}

	// Render help overlay
	if app.input_mode == InputMode::Help {
		draw_help_popup(f);
	}
}

// ---------------------------------------------------------------------------
// Search bar
// ---------------------------------------------------------------------------

fn draw_search_bar(f: &mut Frame, app: &App, area: Rect) {
	let (border_style, title, display_text, is_active) = match app.input_mode {
		InputMode::Search => {
			(Style::default().fg(Color::Yellow), " Search (/) ", app.search_input.as_str(), true)
		},
		InputMode::FilterMessages => (
			Style::default().fg(Color::Magenta),
			" Filter messages (/) ",
			app.message_filter.as_str(),
			true,
		),
		_ => {
			let text = if !app.message_filter.is_empty() {
				app.message_filter.as_str()
			} else if app.query.is_empty() {
				""
			} else {
				app.query.as_str()
			};
			let title = if !app.message_filter.is_empty() {
				" Filter messages (/) "
			} else {
				" Search (/) "
			};
			(Style::default().fg(Color::DarkGray), title, text, false)
		},
	};

	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(border_style)
		.title(title);

	let paragraph = Paragraph::new(display_text).block(block);
	f.render_widget(paragraph, area);

	if is_active {
		let input_len = match app.input_mode {
			InputMode::FilterMessages => app.message_filter.len(),
			_ => app.search_input.len(),
		};
		let cursor_x = area.x + 1 + input_len as u16;
		let cursor_y = area.y + 1;
		f.set_cursor_position((cursor_x, cursor_y));
	}
}

// ---------------------------------------------------------------------------
// Main area (three panes)
// ---------------------------------------------------------------------------

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
	// Horizontal split: sessions (35%) | right side (65%)
	let h_chunks = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
		.split(area);

	if app.messages_maximized {
		// Side-by-side: narrow message list (30%) | wide preview (70%)
		let h_chunks_max = Layout::default()
			.direction(Direction::Horizontal)
			.constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
			.split(area);

		draw_messages_pane(f, app, h_chunks_max[0]);
		draw_preview_pane(f, app, h_chunks_max[1]);
	} else {
		draw_sessions_pane(f, app, h_chunks[0]);

		// Vertical split on the right: messages (40%) | preview (60%)
		let v_chunks = Layout::default()
			.direction(Direction::Vertical)
			.constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
			.split(h_chunks[1]);

		draw_messages_pane(f, app, v_chunks[0]);
		draw_preview_pane(f, app, v_chunks[1]);
	}
}

// ---------------------------------------------------------------------------
// Sessions pane
// ---------------------------------------------------------------------------

fn draw_sessions_pane(f: &mut Frame, app: &App, area: Rect) {
	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(pane_border_style(app, Pane::Sessions))
		.title(" Sessions ");

	if app.sessions.is_empty() {
		let empty = Paragraph::new("  No sessions. Press / to search.")
			.style(Style::default().fg(Color::DarkGray))
			.block(block);
		f.render_widget(empty, area);
		return;
	}

	let items: Vec<ListItem> = app
		.sessions
		.iter()
		.map(|session| {
			// First line: project (green) + branch (yellow)
			let branch_display = if session.git_branch.is_empty() {
				String::new()
			} else {
				format!(" ({})", session.git_branch)
			};

			let first_line = Line::from(vec![
				Span::styled(session.project_name.clone(), Style::default().fg(Color::Green)),
				Span::styled(branch_display, Style::default().fg(Color::Yellow)),
			]);

			// Second line: session ID prefix + hit count + date
			let sid_prefix = if session.session_id.len() > 8 {
				&session.session_id[..8]
			} else {
				&session.session_id
			};
			let second_line = Line::from(vec![
				Span::raw("  "),
				Span::styled(
					format!("{sid_prefix} "),
					Style::default()
						.fg(Color::DarkGray)
						.add_modifier(Modifier::DIM),
				),
				Span::styled(
					format!(
						"{} hit{} \u{00b7} {}",
						session.hit_count,
						if session.hit_count == 1 { "" } else { "s" },
						&session.latest_timestamp,
					),
					Style::default().fg(Color::DarkGray),
				),
			]);

			ListItem::new(vec![first_line, second_line])
		})
		.collect();

	let list = List::new(items)
		.block(block)
		.highlight_style(Style::default().bg(Color::DarkGray));

	let mut state = ListState::default().with_selected(Some(app.session_index));
	f.render_stateful_widget(list, area, &mut state);
}

// ---------------------------------------------------------------------------
// Messages pane
// ---------------------------------------------------------------------------

fn draw_messages_pane(f: &mut Frame, app: &App, area: Rect) {
	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(pane_border_style(app, Pane::Messages))
		.title(" Messages ");

	if app.messages.is_empty() {
		let empty = Paragraph::new("  Select a session to view messages.")
			.style(Style::default().fg(Color::DarkGray))
			.block(block);
		f.render_widget(empty, area);
		return;
	}

	let items: Vec<ListItem> = app
		.messages
		.iter()
		.map(|msg| {
			let role_color = match msg.role.as_str() {
				"user" => Color::Blue,
				"assistant" => Color::Magenta,
				_ => Color::White,
			};

			// Compact timestamp: show just HH:MM
			let short_ts = if msg.timestamp.len() >= 16 {
				&msg.timestamp[11..16]
			} else {
				&msg.timestamp
			};

			// Truncate content to first line for the list view
			let content_preview: String = msg
				.content
				.lines()
				.next()
				.unwrap_or("")
				.chars()
				.take(60)
				.collect();

			// Hit indicator
			let hit_marker = if msg.is_hit {
				Span::styled("* ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
			} else {
				Span::raw("  ")
			};

			let line = Line::from(vec![
				hit_marker,
				Span::styled(
					format!("{} ", short_ts),
					Style::default()
						.fg(Color::DarkGray)
						.add_modifier(Modifier::DIM),
				),
				Span::styled(format!("{}: ", msg.role), Style::default().fg(role_color)),
				Span::raw(content_preview),
			]);

			ListItem::new(line)
		})
		.collect();

	let list = List::new(items)
		.block(block)
		.highlight_style(Style::default().bg(Color::DarkGray));

	let mut state = ListState::default().with_selected(Some(app.message_index));
	f.render_stateful_widget(list, area, &mut state);
}

// ---------------------------------------------------------------------------
// Preview pane
// ---------------------------------------------------------------------------

fn draw_preview_pane(f: &mut Frame, app: &App, area: Rect) {
	let content = match app.selected_message() {
		Some(msg) => {
			let header = Line::from(vec![
				Span::styled(format!("[{}] ", msg.timestamp), Style::default().fg(Color::DarkGray)),
				Span::styled(
					msg.role.clone(),
					Style::default().fg(match msg.role.as_str() {
						"user" => Color::Blue,
						"assistant" => Color::Magenta,
						_ => Color::White,
					}),
				),
			]);

			let mut lines = vec![header, Line::from("")];
			for text_line in msg.content.lines() {
				lines.push(Line::from(text_line.to_string()));
			}
			lines
		},
		None => {
			vec![Line::from(Span::styled(
				"No message selected.",
				Style::default().fg(Color::DarkGray),
			))]
		},
	};

	let total_lines = content.len();
	let visible_height = area.height.saturating_sub(2) as usize; // minus borders
	let scroll = app.preview_scroll as usize;
	let title = if total_lines > visible_height {
		let end = (scroll + visible_height).min(total_lines);
		format!(" Preview [{}-{}/{}] ", scroll + 1, end, total_lines)
	} else {
		" Preview ".to_string()
	};

	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(pane_border_style(app, Pane::Preview))
		.title(title);

	let paragraph = Paragraph::new(content)
		.wrap(Wrap { trim: false })
		.scroll((app.preview_scroll, 0))
		.block(block);
	f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
	let pane_name = match app.active_pane {
		Pane::Sessions => "Sessions",
		Pane::Messages => "Messages",
		Pane::Preview => "Preview",
	};

	let mode_hint = match app.input_mode {
		InputMode::Normal => {
			"j/k: navigate  /: search  Tab: pane  m: maximize  f: project  b: branch  ?: help  q: quit"
		},
		InputMode::Search => "Enter: search  Esc: cancel",
		InputMode::FilterMessages => "Enter: apply  Esc: clear & cancel",
		InputMode::FilterProject | InputMode::FilterBranch => {
			"j/k: navigate  Enter: select  d: clear filter  Esc: cancel"
		},
		InputMode::Help => "Press any key to close",
	};

	let mut spans = vec![
		Span::styled(
			format!(" {} ", pane_name),
			Style::default()
				.fg(Color::Black)
				.bg(Color::Cyan)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw(" "),
		Span::styled(mode_hint, Style::default().fg(Color::DarkGray)),
	];

	// Show active filters
	if let Some(ref project) = app.filter_project {
		spans.push(Span::raw("  "));
		spans
			.push(Span::styled(format!("[project: {}]", project), Style::default().fg(Color::Green)));
	}
	if let Some(ref branch) = app.filter_branch {
		spans.push(Span::raw("  "));
		spans.push(Span::styled(format!("[branch: {}]", branch), Style::default().fg(Color::Yellow)));
	}

	let line = Line::from(spans);

	let paragraph = Paragraph::new(line);
	f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Filter popup
// ---------------------------------------------------------------------------

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
	let v = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Percentage((100 - percent_y) / 2),
			Constraint::Percentage(percent_y),
			Constraint::Percentage((100 - percent_y) / 2),
		])
		.split(area);
	Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Percentage((100 - percent_x) / 2),
			Constraint::Percentage(percent_x),
			Constraint::Percentage((100 - percent_x) / 2),
		])
		.split(v[1])[1]
}

fn draw_filter_popup(f: &mut Frame, app: &App) {
	let area = centered_rect(40, 50, f.area());

	// Clear the popup area
	f.render_widget(Clear, area);

	let (title, items, current_filter) = match app.input_mode {
		InputMode::FilterProject => {
			(" Filter Project ", &app.available_projects, &app.filter_project)
		},
		InputMode::FilterBranch => (" Filter Branch ", &app.available_branches, &app.filter_branch),
		_ => return,
	};

	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(Style::default().fg(Color::Cyan))
		.title(title);

	if items.is_empty() {
		let empty = Paragraph::new("  No options available.")
			.style(Style::default().fg(Color::DarkGray))
			.block(block);
		f.render_widget(empty, area);
		return;
	}

	// Build list items
	let mut list_items: Vec<ListItem> = items
		.iter()
		.enumerate()
		.map(|(i, value)| {
			let is_selected = i == app.filter_index;
			let is_active = current_filter.as_deref() == Some(value.as_str());

			let mut spans = vec![];

			// Selection indicator
			if is_selected {
				spans.push(Span::raw("\u{25b8} "));
			} else {
				spans.push(Span::raw("  "));
			}

			// Active filter indicator
			if is_active {
				spans.push(Span::styled(
					value.clone(),
					Style::default()
						.fg(Color::Green)
						.add_modifier(Modifier::BOLD),
				));
				spans.push(Span::styled(" (active)", Style::default().fg(Color::DarkGray)));
			} else {
				spans.push(Span::raw(value.clone()));
			}

			let style = if is_selected {
				Style::default().bg(Color::DarkGray)
			} else {
				Style::default()
			};

			ListItem::new(Line::from(spans)).style(style)
		})
		.collect();

	// Add footer hint if a filter is active
	if current_filter.is_some() {
		list_items.push(ListItem::new(""));
		list_items.push(ListItem::new(Line::from(Span::styled(
			"  [d] clear filter",
			Style::default()
				.fg(Color::Yellow)
				.add_modifier(Modifier::DIM),
		))));
	}

	let list = List::new(list_items).block(block);
	f.render_widget(list, area);
}

// ---------------------------------------------------------------------------
// Help popup
// ---------------------------------------------------------------------------

fn draw_help_popup(f: &mut Frame) {
	let area = centered_rect(60, 70, f.area());
	f.render_widget(Clear, area);

	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(Style::default().fg(Color::Cyan))
		.title(" Help — press any key to close ");

	let key_style = Style::default()
		.fg(Color::Yellow)
		.add_modifier(Modifier::BOLD);
	let desc_style = Style::default().fg(Color::White);
	let header_style = Style::default()
		.fg(Color::Cyan)
		.add_modifier(Modifier::BOLD);

	let lines = vec![
		Line::from(Span::styled("Navigation", header_style)),
		Line::from(vec![
			Span::styled("  j/k, ↑/↓   ", key_style),
			Span::styled("Move up/down", desc_style),
		]),
		Line::from(vec![
			Span::styled("  Ctrl+d/u   ", key_style),
			Span::styled("Half-page down/up", desc_style),
		]),
		Line::from(vec![
			Span::styled("  PgDn/PgUp  ", key_style),
			Span::styled("Half-page down/up", desc_style),
		]),
		Line::from(vec![
			Span::styled("  Tab        ", key_style),
			Span::styled("Next pane", desc_style),
		]),
		Line::from(vec![
			Span::styled("  Shift+Tab  ", key_style),
			Span::styled("Previous pane", desc_style),
		]),
		Line::from(vec![
			Span::styled("  Enter      ", key_style),
			Span::styled("Select / open", desc_style),
		]),
		Line::from(vec![
			Span::styled("  Esc        ", key_style),
			Span::styled("Back to sessions pane", desc_style),
		]),
		Line::from(""),
		Line::from(Span::styled("Search & Filter", header_style)),
		Line::from(vec![
			Span::styled("  /          ", key_style),
			Span::styled("Search (sessions) / filter messages (messages/preview)", desc_style),
		]),
		Line::from(vec![
			Span::styled("  n / N      ", key_style),
			Span::styled("Next / previous search hit", desc_style),
		]),
		Line::from(vec![
			Span::styled("  f          ", key_style),
			Span::styled("Filter by project", desc_style),
		]),
		Line::from(vec![
			Span::styled("  b          ", key_style),
			Span::styled("Filter by branch", desc_style),
		]),
		Line::from(""),
		Line::from(Span::styled("Preview", header_style)),
		Line::from(vec![
			Span::styled("  g / G      ", key_style),
			Span::styled("Scroll to top / bottom", desc_style),
		]),
		Line::from(vec![
			Span::styled("  y          ", key_style),
			Span::styled("Copy message to clipboard", desc_style),
		]),
		Line::from(""),
		Line::from(Span::styled("Other", header_style)),
		Line::from(vec![
			Span::styled("  m          ", key_style),
			Span::styled("Toggle maximized messages view", desc_style),
		]),
		Line::from(vec![
			Span::styled("  e          ", key_style),
			Span::styled("Open session file in $EDITOR", desc_style),
		]),
		Line::from(vec![
			Span::styled("  ?          ", key_style),
			Span::styled("Show this help", desc_style),
		]),
		Line::from(vec![Span::styled("  q          ", key_style), Span::styled("Quit", desc_style)]),
	];

	let paragraph = Paragraph::new(lines).block(block);
	f.render_widget(paragraph, area);
}
