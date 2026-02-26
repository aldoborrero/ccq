use std::collections::HashMap;
use std::io::Write;

use anyhow::{Context, Result};
use tantivy::{
	Term,
	collector::DocSetCollector,
	query::TermQuery,
	schema::{IndexRecordOption, TantivyDocument},
};

use crate::{
	doc,
	index::IndexHandle,
	tui::theme,
};

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
	let handle = IndexHandle::open()?;

	match session_id {
		None => list_sessions(&handle, project, json, writer),
		Some(sid) => show_session(&handle, &sid, json, head, tail, writer),
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
///
/// Iterates over all segments directly instead of using a query with
/// a large `TopDocs` limit.
fn list_sessions(
	handle: &IndexHandle,
	project: Option<String>,
	json: bool,
	writer: &mut dyn Write,
) -> Result<()> {
	let fields = &handle.fields;
	let searcher = handle.searcher();

	// Group documents by session_id.
	let mut sessions: HashMap<String, SessionInfo> = HashMap::new();

	for segment_reader in searcher.segment_readers() {
		let store_reader = segment_reader
			.get_store_reader(64)
			.context("failed to open segment store reader")?;

		for doc_id in 0..segment_reader.max_doc() {
			if segment_reader.is_deleted(doc_id) {
				continue;
			}

			let d: TantivyDocument = store_reader
				.get(doc_id)
				.context("failed to retrieve document from store")?;

			let sid = doc::get_text(&d, fields.session_id);
			if sid.is_empty() {
				continue;
			}

			let pname = doc::get_text(&d, fields.project_name);
			let branch = doc::get_text(&d, fields.git_branch);
			let ts = doc::get_datetime(&d, fields.timestamp);

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
					Some(dt) => doc::format_date(dt),
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
			Some(dt) => doc::format_date(dt),
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
/// Otherwise iterate over all segments and find session IDs that
/// start with the given prefix. If exactly one match is found,
/// return it.
fn resolve_session_id(
	handle: &IndexHandle,
	input: &str,
) -> Result<String> {
	let searcher = handle.searcher();

	// First try an exact match.
	let term = Term::from_field_text(handle.fields.session_id, input);
	let query = TermQuery::new(term, IndexRecordOption::Basic);
	let exact = searcher
		.search(&query, &DocSetCollector)
		.context("search failed")?;
	if !exact.is_empty() {
		return Ok(input.to_string());
	}

	// Prefix search: iterate over all segments and collect matching session IDs.
	let mut matches: HashMap<String, bool> = HashMap::new();
	for segment_reader in searcher.segment_readers() {
		let store_reader = segment_reader
			.get_store_reader(64)
			.context("failed to open segment store reader")?;

		for doc_id in 0..segment_reader.max_doc() {
			if segment_reader.is_deleted(doc_id) {
				continue;
			}

			let d: TantivyDocument = store_reader
				.get(doc_id)
				.context("failed to retrieve document from store")?;

			let sid = doc::get_text(&d, handle.fields.session_id);
			if sid.starts_with(input) {
				matches.insert(sid, true);
			}
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
	handle: &IndexHandle,
	session_id: &str,
	json: bool,
	head: Option<usize>,
	tail: Option<usize>,
	writer: &mut dyn Write,
) -> Result<()> {
	let fields = &handle.fields;
	let searcher = handle.searcher();

	// Resolve prefix to full session ID.
	let full_sid = resolve_session_id(handle, session_id)?;

	let term = Term::from_field_text(fields.session_id, &full_sid);
	let query = TermQuery::new(term, IndexRecordOption::Basic);

	let results = searcher
		.search(&query, &DocSetCollector)
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

	for doc_addr in results {
		let d: TantivyDocument = searcher.doc(doc_addr)?;
		let role = doc::get_text(&d, fields.role);
		let content = doc::get_text(&d, fields.content);
		let ts = doc::get_datetime(&d, fields.timestamp);
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
					Some(dt) => doc::format_datetime_short(*dt),
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
			Some(dt) => doc::format_datetime_short(*dt),
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
