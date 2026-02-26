use std::collections::BTreeMap;
use std::io::Write;

use anyhow::{Result, bail};
use tantivy::{
	Index, TantivyDocument, Term,
	collector::TopDocs,
	query::{QueryParser, TermQuery},
	schema::{IndexRecordOption, Schema},
};

use crate::{
	doc::{self, SchemaFields},
	index::{build_schema, index_dir},
	tui::theme,
};

/// Info tuple for a session: (session_id, project_name, git_branch,
/// latest_timestamp, message_count).
pub type SessionInfo = (String, String, String, String, usize);

pub struct SearchOptions {
	pub query: String,
	pub project: Option<String>,
	pub branch: Option<String>,
	pub after: Option<String>,
	pub before: Option<String>,
	pub verbose: bool,
	pub json: bool,
	pub limit: usize,
	pub context: Option<usize>,
}

#[derive(serde::Serialize, Clone)]
pub struct SearchHit {
	pub score: f32,
	pub session_id: String,
	pub project_name: String,
	pub git_branch: String,
	pub role: String,
	pub timestamp: String,
	pub content: String,
}

#[derive(serde::Serialize)]
struct SessionGroupJson {
	session_id: String,
	project_name: String,
	git_branch: String,
	best_score: f32,
	hit_count: usize,
	latest_timestamp: String,
}

pub fn run_search(opts: SearchOptions, writer: &mut dyn Write) -> Result<()> {
	let dir = index_dir();
	if !dir.exists() || !dir.join("meta.json").exists() {
		bail!("No index found. Run `ccq index` first.");
	}

	let schema = build_schema();
	let fields = SchemaFields::resolve(&schema)?;
	let index = Index::open_in_dir(&dir)?;
	let reader = index.reader()?;
	let searcher = reader.searcher();

	let query_parser = QueryParser::for_index(&index, vec![fields.content]);
	let query = query_parser.parse_query(&opts.query)?;

	let top_docs = searcher.search(&query, &TopDocs::with_limit(opts.limit))?;

	let mut hits: Vec<SearchHit> = Vec::new();
	for (score, doc_address) in top_docs {
		let d: TantivyDocument = searcher.doc(doc_address)?;

		hits.push(SearchHit {
			score,
			session_id: doc::get_text(&d, fields.session_id),
			project_name: doc::get_text(&d, fields.project_name),
			git_branch: doc::get_text(&d, fields.git_branch),
			role: doc::get_text(&d, fields.role),
			timestamp: doc::get_datetime(&d, fields.timestamp)
				.map(doc::format_datetime)
				.unwrap_or_default(),
			content: doc::get_text(&d, fields.content),
		});
	}

	// Post-search filters.
	if let Some(ref project_filter) = opts.project {
		let filter_lower = project_filter.to_lowercase();
		hits.retain(|h| h.project_name.to_lowercase().contains(&filter_lower));
	}
	if let Some(ref branch_filter) = opts.branch {
		hits.retain(|h| h.git_branch == *branch_filter);
	}
	if let Some(ref after) = opts.after {
		hits.retain(|h| h.timestamp.as_str() >= after.as_str());
	}
	if let Some(ref before) = opts.before {
		hits.retain(|h| h.timestamp.as_str() <= before.as_str());
	}

	if hits.is_empty() {
		if opts.json {
			writeln!(writer, "[]")?;
		} else {
			writeln!(writer, "No results found.")?;
		}
		return Ok(());
	}

	if opts.json {
		if opts.verbose {
			writeln!(writer, "{}", serde_json::to_string_pretty(&hits)?)?;
		} else {
			let mut groups: BTreeMap<String, SessionGroupJson> = BTreeMap::new();
			for hit in &hits {
				let entry = groups
					.entry(hit.session_id.clone())
					.or_insert_with(|| SessionGroupJson {
						session_id: hit.session_id.clone(),
						project_name: hit.project_name.clone(),
						git_branch: hit.git_branch.clone(),
						best_score: hit.score,
						hit_count: 0,
						latest_timestamp: String::new(),
					});
				if hit.score > entry.best_score {
					entry.best_score = hit.score;
				}
				entry.hit_count += 1;
				if hit.timestamp > entry.latest_timestamp {
					entry.latest_timestamp = hit.timestamp.clone();
				}
			}
			let mut sorted: Vec<SessionGroupJson> = groups.into_values().collect();
			sorted.sort_by(|a, b| {
				b.best_score
					.partial_cmp(&a.best_score)
					.unwrap_or(std::cmp::Ordering::Equal)
			});
			writeln!(writer, "{}", serde_json::to_string_pretty(&sorted)?)?;
		}
		return Ok(());
	}

	if opts.verbose {
		// Optionally fetch context messages for each hit.
		let context_data: Vec<Vec<(String, String, String)>> = if let Some(n) = opts.context {
			hits
				.iter()
				.map(|hit| {
					fetch_context(&searcher, &schema, &hit.session_id, &hit.timestamp, &hit.role, n)
						.unwrap_or_default()
				})
				.collect()
		} else {
			Vec::new()
		};
		print_verbose(&hits, &opts.query, &context_data, writer)?;
	} else {
		print_grouped(&hits, writer)?;
	}

	Ok(())
}

/// Searches the index and returns matching hits.
///
/// Opens the index, builds a query parser on the `content` field,
/// and returns up to `limit` results sorted by relevance score.
/// Returns an empty Vec if the index directory does not exist.
pub fn search_hits(query: &str, limit: usize) -> Result<Vec<SearchHit>> {
	let dir = index_dir();
	if !dir.exists() || !dir.join("meta.json").exists() {
		return Ok(Vec::new());
	}

	let schema = build_schema();
	let fields = SchemaFields::resolve(&schema)?;
	let index = Index::open_in_dir(&dir)?;
	let reader = index.reader()?;
	let searcher = reader.searcher();

	let query_parser = QueryParser::for_index(&index, vec![fields.content]);
	let parsed_query = query_parser.parse_query(query)?;

	let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(limit))?;

	let mut hits = Vec::new();
	for (score, doc_address) in top_docs {
		let d: TantivyDocument = searcher.doc(doc_address)?;
		hits.push(SearchHit {
			score,
			session_id: doc::get_text(&d, fields.session_id),
			project_name: doc::get_text(&d, fields.project_name),
			git_branch: doc::get_text(&d, fields.git_branch),
			role: doc::get_text(&d, fields.role),
			timestamp: doc::get_datetime(&d, fields.timestamp)
				.map(doc::format_datetime)
				.unwrap_or_default(),
			content: doc::get_text(&d, fields.content),
		});
	}

	Ok(hits)
}

/// Returns all sessions from the index.
///
/// Scans the entire index using `AllQuery`, groups documents by `session_id`,
/// and returns `(session_id, project_name, git_branch, latest_timestamp,
/// message_count)` sorted by latest timestamp descending.
pub fn all_sessions() -> Result<Vec<SessionInfo>> {
	let dir = index_dir();
	if !dir.exists() || !dir.join("meta.json").exists() {
		return Ok(Vec::new());
	}

	let schema = build_schema();
	let fields = SchemaFields::resolve(&schema)?;
	let index = Index::open_in_dir(&dir)?;
	let reader = index.reader()?;
	let searcher = reader.searcher();

	let top_docs = searcher.search(&tantivy::query::AllQuery, &TopDocs::with_limit(1_000_000))?;

	let mut groups: BTreeMap<String, (String, String, String, usize)> = BTreeMap::new();

	for (_score, doc_address) in top_docs {
		let d: TantivyDocument = searcher.doc(doc_address)?;
		let session_id = doc::get_text(&d, fields.session_id);
		if session_id.is_empty() {
			continue;
		}
		let project_name = doc::get_text(&d, fields.project_name);
		let git_branch = doc::get_text(&d, fields.git_branch);
		let timestamp = doc::get_datetime(&d, fields.timestamp)
			.map(doc::format_datetime)
			.unwrap_or_default();

		let entry = groups
			.entry(session_id)
			.or_insert_with(|| (project_name.clone(), git_branch.clone(), String::new(), 0));

		entry.3 += 1;
		if timestamp > entry.2 {
			entry.2 = timestamp;
		}
		if entry.0.is_empty() && !project_name.is_empty() {
			entry.0 = project_name;
		}
		if entry.1.is_empty() && !git_branch.is_empty() {
			entry.1 = git_branch;
		}
	}

	let mut result: Vec<SessionInfo> = groups
		.into_iter()
		.map(|(sid, (pname, branch, ts, count))| (sid, pname, branch, ts, count))
		.collect();

	// Sort by latest timestamp descending.
	result.sort_by(|a, b| b.3.cmp(&a.3));

	Ok(result)
}

/// Returns all messages for a given session, sorted by timestamp ascending.
///
/// Uses a `TermQuery` on the `session_id` field to find all documents
/// belonging to the session and returns `(timestamp, role, content)` tuples.
pub fn session_messages(session_id: &str) -> Result<Vec<(String, String, String)>> {
	let dir = index_dir();
	if !dir.exists() || !dir.join("meta.json").exists() {
		return Ok(Vec::new());
	}

	let schema = build_schema();
	let fields = SchemaFields::resolve(&schema)?;
	let index = Index::open_in_dir(&dir)?;
	let reader = index.reader()?;
	let searcher = reader.searcher();

	let term = Term::from_field_text(fields.session_id, session_id);
	let term_query = TermQuery::new(term, IndexRecordOption::Basic);

	let top_docs = searcher.search(&term_query, &TopDocs::with_limit(100_000))?;

	let mut messages = Vec::new();
	for (_score, doc_address) in top_docs {
		let d: TantivyDocument = searcher.doc(doc_address)?;
		let timestamp = doc::get_datetime(&d, fields.timestamp)
			.map(doc::format_datetime)
			.unwrap_or_default();
		let role = doc::get_text(&d, fields.role);
		let content = doc::get_text(&d, fields.content);
		messages.push((timestamp, role, content));
	}

	// Sort by timestamp ascending.
	messages.sort_by(|a, b| a.0.cmp(&b.0));

	Ok(messages)
}

fn extract_snippet(content: &str, query: &str, window: usize) -> String {
	let stop_words: &[&str] = &["and", "or", "not"];
	let terms: Vec<String> = query
		.split_whitespace()
		.filter(|w| !stop_words.contains(&w.to_lowercase().as_str()))
		.map(|w| w.to_lowercase())
		.collect();

	let content_lower = content.to_lowercase();

	// Find first matching term position (byte-level, then align to char
	// boundaries).
	let first_match = terms.iter().find_map(|term| {
		content_lower.find(term.as_str()).map(|byte_pos| {
			// Find the char index corresponding to this byte position.
			content
				.char_indices()
				.position(|(idx, _)| idx == byte_pos)
				.unwrap_or(0)
		})
	});

	// Collect char indices for safe slicing.
	let chars: Vec<(usize, char)> = content.char_indices().collect();
	let char_count = chars.len();

	let snippet = if let Some(match_char_pos) = first_match {
		let start_char = match_char_pos.saturating_sub(window);
		let end_char = (match_char_pos + window).min(char_count);

		let start_byte = chars[start_char].0;
		let end_byte = if end_char < char_count {
			chars[end_char].0
		} else {
			content.len()
		};

		let mut s = String::new();
		if start_char > 0 {
			s.push_str("...");
		}
		s.push_str(&content[start_byte..end_byte]);
		if end_char < char_count {
			s.push_str("...");
		}
		s
	} else {
		// No match found — fall back to truncating at window * 2 chars.
		let limit = (window * 2).min(char_count);
		let end_byte = if limit < char_count {
			chars[limit].0
		} else {
			content.len()
		};
		let mut s = content[..end_byte].to_string();
		if limit < char_count {
			s.push_str("...");
		}
		s
	};

	// Highlight all matching terms (case-insensitive).
	let mut result = snippet;
	for term in &terms {
		let mut highlighted = String::new();
		let lower = result.to_lowercase();
		let mut last = 0;
		for (byte_pos, _) in lower.match_indices(term.as_str()) {
			highlighted.push_str(&result[last..byte_pos]);
			let matched_text = &result[byte_pos..byte_pos + term.len()];
			highlighted.push_str(&theme::styled_highlight(matched_text));
			last = byte_pos + term.len();
		}
		highlighted.push_str(&result[last..]);
		result = highlighted;
	}

	result
}

/// Fetches context messages surrounding a hit in the same session.
///
/// Returns a vec of `(timestamp, role, content)` tuples for the messages
/// immediately before and after the hit (up to `context_size` each).
fn fetch_context(
	searcher: &tantivy::Searcher,
	schema: &Schema,
	session_id: &str,
	hit_timestamp: &str,
	hit_role: &str,
	context_size: usize,
) -> Result<Vec<(String, String, String)>> {
	let fields = SchemaFields::resolve(schema)?;
	let term = Term::from_field_text(fields.session_id, session_id);
	let term_query = TermQuery::new(term, IndexRecordOption::Basic);

	// Collect all messages in this session (cap at a generous limit).
	let top_docs = searcher.search(&term_query, &TopDocs::with_limit(10_000))?;

	let mut messages: Vec<(String, String, String)> = Vec::new();
	for (_score, doc_address) in top_docs {
		let d: TantivyDocument = searcher.doc(doc_address)?;
		let ts = doc::get_datetime(&d, fields.timestamp)
			.map(doc::format_datetime)
			.unwrap_or_default();
		let role = doc::get_text(&d, fields.role);
		let content = doc::get_text(&d, fields.content);
		messages.push((ts, role, content));
	}

	// Sort by timestamp.
	messages.sort_by(|a, b| a.0.cmp(&b.0));

	// Find the index of the hit message by matching timestamp + role.
	let hit_idx = messages
		.iter()
		.position(|(ts, role, _)| ts == hit_timestamp && role == hit_role);

	let Some(hit_idx) = hit_idx else {
		return Ok(Vec::new());
	};

	// Gather context_size messages before and after, excluding the hit itself.
	let start = hit_idx.saturating_sub(context_size);
	let end = (hit_idx + context_size + 1).min(messages.len());

	let mut context_msgs = Vec::new();
	for (i, msg) in messages[start..end].iter().enumerate() {
		let actual_idx = start + i;
		if actual_idx != hit_idx {
			context_msgs.push(msg.clone());
		}
	}

	Ok(context_msgs)
}

fn print_verbose(
	hits: &[SearchHit],
	query: &str,
	context_data: &[Vec<(String, String, String)>],
	writer: &mut dyn Write,
) -> Result<()> {
	writeln!(writer, "Found {} result(s):\n", theme::styled_bold(&hits.len().to_string()))?;
	for (i, hit) in hits.iter().enumerate() {
		let content_preview = extract_snippet(&hit.content, query, 80);

		// Print context messages before the hit (those with timestamp < hit timestamp).
		if let Some(ctx) = context_data.get(i) {
			let before_msgs: Vec<&(String, String, String)> = ctx
				.iter()
				.filter(|(ts, ..)| ts.as_str() <= hit.timestamp.as_str())
				.collect();
			if !before_msgs.is_empty() {
				writeln!(writer, "  {}", theme::styled_dim("--- context ---"))?;
				for (ts, role, content) in &before_msgs {
					let truncated = truncate_content(content, 120);
					writeln!(
						writer,
						"  {} {} [{}] {}",
						theme::styled_dim(ts),
						theme::styled_dim(role),
						theme::styled_dim("ctx"),
						theme::styled_dim(&truncated),
					)?;
				}
				writeln!(writer, "  {}", theme::styled_dim("---"))?;
			}
		}

		writeln!(writer, "  Score:     {}", theme::styled_score(&format!("{:.4}", hit.score)))?;
		writeln!(writer, "  Session:   {}", theme::styled_session_id(&hit.session_id))?;
		writeln!(writer, "  Project:   {}", theme::styled_project(&hit.project_name))?;
		writeln!(writer, "  Branch:    {}", theme::styled_branch(&hit.git_branch))?;
		writeln!(writer, "  Role:      {}", theme::styled_role(&hit.role))?;
		writeln!(writer, "  Timestamp: {}", hit.timestamp)?;
		writeln!(writer, "  Content:   {}", content_preview)?;

		// Print context messages after the hit (those with timestamp > hit timestamp).
		if let Some(ctx) = context_data.get(i) {
			let after_msgs: Vec<&(String, String, String)> = ctx
				.iter()
				.filter(|(ts, ..)| ts.as_str() > hit.timestamp.as_str())
				.collect();
			if !after_msgs.is_empty() {
				writeln!(writer, "  {}", theme::styled_dim("---"))?;
				for (ts, role, content) in &after_msgs {
					let truncated = truncate_content(content, 120);
					writeln!(
						writer,
						"  {} {} [{}] {}",
						theme::styled_dim(ts),
						theme::styled_dim(role),
						theme::styled_dim("ctx"),
						theme::styled_dim(&truncated),
					)?;
				}
				writeln!(writer, "  {}", theme::styled_dim("--- context ---"))?;
			}
		}

		writeln!(writer)?;
	}
	Ok(())
}

/// Truncates content to the given max number of characters, appending "..." if
/// truncated.
fn truncate_content(content: &str, max_chars: usize) -> String {
	let chars: Vec<char> = content.chars().collect();
	if chars.len() <= max_chars {
		content.to_string()
	} else {
		let mut s: String = chars[..max_chars].iter().collect();
		s.push_str("...");
		s
	}
}

fn print_grouped(hits: &[SearchHit], writer: &mut dyn Write) -> Result<()> {
	// Group by session_id, keeping track of best score per session.
	let mut groups: BTreeMap<String, SessionGroup> = BTreeMap::new();

	for hit in hits {
		let entry = groups
			.entry(hit.session_id.clone())
			.or_insert_with(|| SessionGroup {
				best_score: hit.score,
				project_name: hit.project_name.clone(),
				git_branch: hit.git_branch.clone(),
				message_count: 0,
				latest_timestamp: String::new(),
			});

		if hit.score > entry.best_score {
			entry.best_score = hit.score;
		}
		entry.message_count += 1;
		if hit.timestamp > entry.latest_timestamp {
			entry.latest_timestamp = hit.timestamp.clone();
		}
	}

	// Sort sessions by best score descending.
	let mut sorted: Vec<(String, SessionGroup)> = groups.into_iter().collect();
	sorted.sort_by(|a, b| {
		b.1.best_score
			.partial_cmp(&a.1.best_score)
			.unwrap_or(std::cmp::Ordering::Equal)
	});

	writeln!(
		writer,
		"Found {} matching message(s) across {} session(s):\n",
		theme::styled_bold(&hits.len().to_string()),
		theme::styled_bold(&sorted.len().to_string()),
	)?;
	for (session_id, group) in &sorted {
		writeln!(
			writer,
			"  {} session {} ({} hit(s))",
			theme::styled_score(&format!("[{:.4}]", group.best_score)),
			theme::styled_session_id(session_id),
			theme::styled_bold(&group.message_count.to_string()),
		)?;
		writeln!(
			writer,
			"           project: {}  branch: {}  last: {}",
			theme::styled_project(&group.project_name),
			theme::styled_branch(&group.git_branch),
			group.latest_timestamp,
		)?;
	}
	Ok(())
}

struct SessionGroup {
	best_score: f32,
	project_name: String,
	git_branch: String,
	message_count: usize,
	latest_timestamp: String,
}
