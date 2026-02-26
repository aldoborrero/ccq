use std::collections::HashMap;
use std::io::Write;

use anyhow::{Context, Result};
use tantivy::{
	Index, Term,
	collector::TopDocs,
	query::{AllQuery, TermQuery},
	schema::{Field, IndexRecordOption, OwnedValue, Schema},
};

use crate::{
	index::{build_schema, index_dir},
	tui::theme,
};

/// Extract a text field from a document, returning an empty string if missing.
fn get_text(doc: &tantivy::TantivyDocument, field: Field) -> String {
	match doc.get_first(field) {
		Some(OwnedValue::Str(s)) => s.clone(),
		_ => String::new(),
	}
}

/// Extract a tantivy DateTime from a document, returning None if missing.
fn get_datetime(doc: &tantivy::TantivyDocument, field: Field) -> Option<tantivy::DateTime> {
	match doc.get_first(field) {
		Some(OwnedValue::Date(dt)) => Some(*dt),
		_ => None,
	}
}

/// Format a tantivy DateTime for display using chrono.
fn format_datetime(dt: tantivy::DateTime) -> String {
	let secs = dt.into_timestamp_secs();
	let chrono_dt = chrono::DateTime::from_timestamp(secs, 0);
	match chrono_dt {
		Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
		None => "unknown".to_string(),
	}
}

/// Format a tantivy DateTime as a date-only string.
fn format_date(dt: tantivy::DateTime) -> String {
	let secs = dt.into_timestamp_secs();
	let chrono_dt = chrono::DateTime::from_timestamp(secs, 0);
	match chrono_dt {
		Some(t) => t.format("%Y-%m-%d").to_string(),
		None => "unknown".to_string(),
	}
}

/// Holds aggregated info about a session for listing.
struct SessionInfo {
	session_id: String,
	project_name: String,
	git_branch: String,
	earliest: Option<tantivy::DateTime>,
	count: usize,
}

/// Entry point for the sessions command.
///
/// Two modes:
/// - No `session_id`: list all sessions grouped by session_id.
/// - With `session_id`: show all messages for that session.
pub fn run_sessions(
	session_id: Option<String>,
	project: Option<String>,
	json: bool,
	head: Option<usize>,
	tail: Option<usize>,
	writer: &mut dyn Write,
) -> Result<()> {
	let dir = index_dir();
	let index =
		Index::open_in_dir(&dir).context("failed to open index — have you run `ccq index`?")?;
	let schema = build_schema();
	let reader = index.reader().context("failed to open index reader")?;
	let searcher = reader.searcher();

	match session_id {
		None => list_sessions(&searcher, &schema, project, json, writer),
		Some(sid) => show_session(&searcher, &schema, &sid, json, head, tail, writer),
	}
}

/// Serializable representation of a session for JSON output.
#[derive(serde::Serialize)]
struct SessionJson {
	session_id: String,
	project_name: String,
	git_branch: String,
	date: String,
	message_count: usize,
}

/// Serializable representation of a message for JSON output.
#[derive(serde::Serialize)]
struct MessageJson {
	timestamp: String,
	role: String,
	content: String,
}

/// List all sessions, optionally filtered by project name.
fn list_sessions(
	searcher: &tantivy::Searcher,
	schema: &Schema,
	project: Option<String>,
	json: bool,
	writer: &mut dyn Write,
) -> Result<()> {
	let f_session_id = schema.get_field("session_id").unwrap();
	let f_project_name = schema.get_field("project_name").unwrap();
	let f_git_branch = schema.get_field("git_branch").unwrap();
	let f_timestamp = schema.get_field("timestamp").unwrap();

	let top_docs = TopDocs::with_limit(1_000_000);
	let results = searcher
		.search(&AllQuery, &top_docs)
		.context("search failed")?;

	// Group documents by session_id.
	let mut sessions: HashMap<String, SessionInfo> = HashMap::new();

	for (_score, doc_addr) in results {
		let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;

		let sid = get_text(&doc, f_session_id);
		if sid.is_empty() {
			continue;
		}

		let pname = get_text(&doc, f_project_name);
		let branch = get_text(&doc, f_git_branch);
		let ts = get_datetime(&doc, f_timestamp);

		// Apply project filter if provided.
		if let Some(ref filter) = project
			&& !pname.to_lowercase().contains(&filter.to_lowercase())
		{
			continue;
		}

		let entry = sessions.entry(sid.clone()).or_insert_with(|| SessionInfo {
			session_id: sid,
			project_name: pname.clone(),
			git_branch: branch.clone(),
			earliest: ts,
			count: 0,
		});

		entry.count += 1;

		// Track earliest timestamp.
		if let Some(current_ts) = ts {
			match entry.earliest {
				Some(existing) if current_ts < existing => {
					entry.earliest = Some(current_ts);
				},
				None => {
					entry.earliest = Some(current_ts);
				},
				_ => {},
			}
		}

		// Update project_name/branch if they were empty before.
		if entry.project_name.is_empty() && !pname.is_empty() {
			entry.project_name = pname;
		}
		if entry.git_branch.is_empty() && !branch.is_empty() {
			entry.git_branch = branch;
		}
	}

	if sessions.is_empty() {
		if json {
			writeln!(writer, "[]")?;
		} else {
			writeln!(writer, "No sessions found.")?;
		}
		return Ok(());
	}

	// Collect and sort by earliest timestamp descending.
	let mut session_list: Vec<SessionInfo> = sessions.into_values().collect();
	session_list.sort_by(|a, b| {
		let a_ts = a.earliest.map(|d| d.into_timestamp_secs()).unwrap_or(0);
		let b_ts = b.earliest.map(|d| d.into_timestamp_secs()).unwrap_or(0);
		b_ts.cmp(&a_ts)
	});

	if json {
		let json_list: Vec<SessionJson> = session_list
			.iter()
			.map(|info| SessionJson {
				session_id: info.session_id.clone(),
				project_name: info.project_name.clone(),
				git_branch: info.git_branch.clone(),
				date: match info.earliest {
					Some(dt) => format_date(dt),
					None => "unknown".to_string(),
				},
				message_count: info.count,
			})
			.collect();
		writeln!(writer, "{}", serde_json::to_string_pretty(&json_list)?)?;
		return Ok(());
	}

	for info in &session_list {
		let date_str = match info.earliest {
			Some(dt) => format_date(dt),
			None => "unknown".to_string(),
		};

		let branch_display = if info.git_branch.is_empty() {
			String::new()
		} else {
			format!(" ({})", theme::styled_branch(&info.git_branch))
		};

		let sid_prefix = if info.session_id.len() > 8 {
			&info.session_id[..8]
		} else {
			&info.session_id
		};

		writeln!(
			writer,
			"[{}] {}{} — {} — {} messages",
			date_str,
			theme::styled_project(&info.project_name),
			branch_display,
			theme::styled_session_id(sid_prefix),
			theme::styled_bold(&info.count.to_string()),
		)?;
	}

	writeln!(writer, "\nTotal: {} sessions", theme::styled_bold(&session_list.len().to_string()))?;

	Ok(())
}

/// Resolve a (possibly prefix) session ID to the full session ID.
///
/// If the given string matches a session_id exactly, return it.
/// Otherwise scan all docs and find session IDs that start with the
/// given prefix. If exactly one match is found, return it.
fn resolve_session_id(
	searcher: &tantivy::Searcher,
	schema: &Schema,
	input: &str,
) -> Result<String> {
	let f_session_id = schema.get_field("session_id").unwrap();

	// First try an exact match.
	let term = Term::from_field_text(f_session_id, input);
	let query = TermQuery::new(term, IndexRecordOption::Basic);
	let exact = searcher
		.search(&query, &TopDocs::with_limit(1))
		.context("search failed")?;
	if !exact.is_empty() {
		return Ok(input.to_string());
	}

	// Prefix search: scan all docs and collect matching session IDs.
	let all_docs = searcher
		.search(&AllQuery, &TopDocs::with_limit(1_000_000))
		.context("search failed")?;

	let mut matches: HashMap<String, bool> = HashMap::new();
	for (_score, doc_addr) in all_docs {
		let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
		let sid = get_text(&doc, f_session_id);
		if sid.starts_with(input) {
			matches.insert(sid, true);
		}
	}

	match matches.len() {
		0 => anyhow::bail!("no session found matching: {input}"),
		1 => Ok(matches.into_keys().next().unwrap()),
		n => {
			let ids: Vec<String> = matches.into_keys().collect();
			anyhow::bail!(
				"ambiguous session prefix '{input}' matches {n} sessions:\n  {}",
				ids.join("\n  ")
			);
		},
	}
}

/// Show all messages for a specific session, sorted by timestamp.
fn show_session(
	searcher: &tantivy::Searcher,
	schema: &Schema,
	session_id: &str,
	json: bool,
	head: Option<usize>,
	tail: Option<usize>,
	writer: &mut dyn Write,
) -> Result<()> {
	let f_session_id = schema.get_field("session_id").unwrap();
	let f_role = schema.get_field("role").unwrap();
	let f_timestamp = schema.get_field("timestamp").unwrap();
	let f_content = schema.get_field("content").unwrap();

	// Resolve prefix to full session ID.
	let full_sid = resolve_session_id(searcher, schema, session_id)?;

	let term = Term::from_field_text(f_session_id, &full_sid);
	let query = TermQuery::new(term, IndexRecordOption::Basic);

	let top_docs = TopDocs::with_limit(1_000_000);
	let results = searcher
		.search(&query, &top_docs)
		.context("search failed")?;

	if results.is_empty() {
		if json {
			writeln!(writer, "[]")?;
		} else {
			writeln!(writer, "No messages found for session: {full_sid}")?;
		}
		return Ok(());
	}

	// Collect all messages with their timestamps for sorting.
	let mut messages: Vec<(Option<tantivy::DateTime>, String, String)> = Vec::new();

	for (_score, doc_addr) in results {
		let doc: tantivy::TantivyDocument = searcher.doc(doc_addr)?;
		let role = get_text(&doc, f_role);
		let content = get_text(&doc, f_content);
		let ts = get_datetime(&doc, f_timestamp);
		messages.push((ts, role, content));
	}

	// Sort by timestamp ascending.
	messages.sort_by(|a, b| {
		let a_ts = a.0.map(|d| d.into_timestamp_secs()).unwrap_or(0);
		let b_ts = b.0.map(|d| d.into_timestamp_secs()).unwrap_or(0);
		a_ts.cmp(&b_ts)
	});

	let total = messages.len();

	// Apply head/tail slicing.
	let range_note = if let Some(n) = head {
		let n = n.min(total);
		messages.truncate(n);
		Some(format!("showing first {n}"))
	} else if let Some(n) = tail {
		let n = n.min(total);
		let start = total.saturating_sub(n);
		messages = messages.split_off(start);
		Some(format!("showing last {n}"))
	} else {
		None
	};

	if json {
		let json_messages: Vec<MessageJson> = messages
			.iter()
			.map(|(ts, role, content)| MessageJson {
				timestamp: match ts {
					Some(dt) => format_datetime(*dt),
					None => "unknown".to_string(),
				},
				role: role.clone(),
				content: content.clone(),
			})
			.collect();
		writeln!(writer, "{}", serde_json::to_string_pretty(&json_messages)?)?;
		return Ok(());
	}

	writeln!(writer, "Session: {}", theme::styled_session_id(&full_sid))?;
	match range_note {
		Some(note) => writeln!(
			writer,
			"Messages: {} ({})\n",
			theme::styled_bold(&total.to_string()),
			note,
		)?,
		None => writeln!(writer, "Messages: {}\n", theme::styled_bold(&total.to_string()))?,
	}

	for (ts, role, content) in &messages {
		let ts_str = match ts {
			Some(dt) => format_datetime(*dt),
			None => "unknown".to_string(),
		};
		writeln!(writer, "[{}] {}:", theme::styled_score(&ts_str), theme::styled_role(role))?;
		// Indent content lines.
		for line in content.lines() {
			writeln!(writer, "  {line}")?;
		}
		writeln!(writer)?;
	}

	Ok(())
}
