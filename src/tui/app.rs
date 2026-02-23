#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pane {
	Sessions,
	Messages,
	Preview,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
	Normal,
	Search,
	FilterMessages,
	FilterProject,
	FilterBranch,
	Help,
}

pub struct SessionEntry {
	pub session_id: String,
	pub project_name: String,
	pub git_branch: String,
	pub hit_count: usize,
	pub best_score: f32,
	pub latest_timestamp: String,
}

#[derive(Clone)]
pub struct MessageEntry {
	pub timestamp: String,
	pub role: String,
	pub content: String,
	pub is_hit: bool,
}

pub struct App {
	pub running: bool,
	pub active_pane: Pane,
	pub input_mode: InputMode,
	pub search_input: String,
	pub query: String,
	pub sessions: Vec<SessionEntry>,
	pub session_index: usize,
	pub messages: Vec<MessageEntry>,
	pub message_index: usize,
	pub search_dirty: bool,
	pub last_keystroke: std::time::Instant,
	pub preview_scroll: u16,
	pub filter_project: Option<String>,
	pub filter_branch: Option<String>,
	pub available_projects: Vec<String>,
	pub available_branches: Vec<String>,
	pub filter_index: usize,
	pub messages_maximized: bool,
	/// Timestamps of search hits in the current query, keyed by session_id
	pub hit_timestamps: std::collections::HashMap<String, Vec<String>>,
	/// Unfiltered messages for the current session (used when filtering)
	pub all_messages: Vec<MessageEntry>,
	/// Current message filter text
	pub message_filter: String,
	/// Approximate visible rows in the main pane area (updated each frame)
	pub visible_rows: u16,
}

impl App {
	pub fn new(query: Option<String>) -> Self {
		App {
			running: true,
			active_pane: Pane::Sessions,
			input_mode: InputMode::Normal,
			search_input: query.clone().unwrap_or_default(),
			query: query.unwrap_or_default(),
			sessions: Vec::new(),
			session_index: 0,
			messages: Vec::new(),
			message_index: 0,
			search_dirty: false,
			last_keystroke: std::time::Instant::now(),
			preview_scroll: 0,
			filter_project: None,
			filter_branch: None,
			available_projects: Vec::new(),
			available_branches: Vec::new(),
			filter_index: 0,
			messages_maximized: false,
			hit_timestamps: std::collections::HashMap::new(),
			all_messages: Vec::new(),
			message_filter: String::new(),
			visible_rows: 20,
		}
	}

	pub fn selected_session(&self) -> Option<&SessionEntry> {
		self.sessions.get(self.session_index)
	}

	pub fn selected_message(&self) -> Option<&MessageEntry> {
		self.messages.get(self.message_index)
	}

	/// Find the next hit message index after the current one (wraps around).
	pub fn next_hit(&self) -> Option<usize> {
		if self.messages.is_empty() {
			return None;
		}
		let len = self.messages.len();
		for offset in 1..=len {
			let idx = (self.message_index + offset) % len;
			if self.messages[idx].is_hit {
				return Some(idx);
			}
		}
		None
	}

	/// Find the previous hit message index before the current one (wraps around).
	pub fn prev_hit(&self) -> Option<usize> {
		if self.messages.is_empty() {
			return None;
		}
		let len = self.messages.len();
		for offset in 1..=len {
			let idx = (self.message_index + len - offset) % len;
			if self.messages[idx].is_hit {
				return Some(idx);
			}
		}
		None
	}
}
