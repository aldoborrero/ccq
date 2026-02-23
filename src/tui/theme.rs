use crossterm::style::Stylize;

// ---------------------------------------------------------------------------
// Color constants (re-exported for use in ratatui widgets and elsewhere)
// ---------------------------------------------------------------------------

pub const COLOR_SCORE: crossterm::style::Color = crossterm::style::Color::DarkGrey;
pub const COLOR_SESSION_ID: crossterm::style::Color = crossterm::style::Color::Cyan;
pub const COLOR_PROJECT: crossterm::style::Color = crossterm::style::Color::Green;
pub const COLOR_BRANCH: crossterm::style::Color = crossterm::style::Color::Yellow;
pub const COLOR_ROLE_USER: crossterm::style::Color = crossterm::style::Color::Blue;
pub const COLOR_ROLE_ASSISTANT: crossterm::style::Color = crossterm::style::Color::Magenta;
pub const COLOR_HIGHLIGHT: crossterm::style::Color = crossterm::style::Color::Red;
pub const COLOR_DIM: crossterm::style::Color = crossterm::style::Color::DarkGrey;

// ---------------------------------------------------------------------------
// Styled-string helpers (return ANSI-escaped strings for terminal output)
// ---------------------------------------------------------------------------

pub fn styled_score(s: &str) -> String {
	s.with(COLOR_SCORE).to_string()
}

pub fn styled_session_id(s: &str) -> String {
	s.with(COLOR_SESSION_ID).to_string()
}

pub fn styled_project(s: &str) -> String {
	s.with(COLOR_PROJECT).to_string()
}

pub fn styled_branch(s: &str) -> String {
	s.with(COLOR_BRANCH).to_string()
}

pub fn styled_role(role: &str) -> String {
	match role {
		"user" => role.with(COLOR_ROLE_USER).to_string(),
		"assistant" => role.with(COLOR_ROLE_ASSISTANT).to_string(),
		_ => role.to_string(),
	}
}

pub fn styled_bold(s: &str) -> String {
	s.bold().to_string()
}

pub fn styled_highlight(s: &str) -> String {
	s.with(COLOR_HIGHLIGHT).bold().to_string()
}

pub fn styled_dim(s: &str) -> String {
	s.with(COLOR_DIM).to_string()
}
