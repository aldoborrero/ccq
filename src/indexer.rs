use std::{
	collections::HashSet,
	fs::File,
	io::{BufRead, BufReader},
	path::Path,
};

use anyhow::{Context, Result};
use tantivy::{DateTime, IndexWriter, schema::TantivyDocument};

use crate::{
	discovery::discover_sessions,
	index::{build_schema, delete_session, index_dir, open_or_create_index},
	meta::IndexMeta,
	parser::parse_line,
};

/// Run the full indexing pipeline: discover sessions, compare mtimes,
/// index new/changed files, remove deleted sessions, and commit.
pub fn run_index(claude_dir: &Path, force: bool) -> Result<()> {
	let index = open_or_create_index(force)?;
	let schema = build_schema();
	let idx_dir = index_dir();

	let mut meta = if force {
		IndexMeta::default()
	} else {
		IndexMeta::load(&idx_dir).context("failed to load index metadata")?
	};

	let sessions = discover_sessions(claude_dir).context("failed to discover sessions")?;

	let mut writer: IndexWriter<TantivyDocument> = index
		.writer(50_000_000)
		.context("failed to create index writer")?;

	// Resolve schema fields once.
	let f_id = schema.get_field("id").unwrap();
	let f_project = schema.get_field("project").unwrap();
	let f_project_name = schema.get_field("project_name").unwrap();
	let f_session_id = schema.get_field("session_id").unwrap();
	let f_git_branch = schema.get_field("git_branch").unwrap();
	let f_role = schema.get_field("role").unwrap();
	let f_timestamp = schema.get_field("timestamp").unwrap();
	let f_content = schema.get_field("content").unwrap();

	let mut indexed_sessions: u64 = 0;
	let mut indexed_messages: u64 = 0;
	let mut skipped: u64 = 0;
	let mut removed: u64 = 0;

	// Track which file paths we see so we can detect deletions.
	let mut seen_files: HashSet<String> = HashSet::new();

	for session in &sessions {
		let file_key = session.path.to_string_lossy().to_string();
		seen_files.insert(file_key.clone());

		let current_mtime = IndexMeta::mtime_to_u64(session.mtime);

		// Skip unchanged files.
		if let Some(&stored_mtime) = meta.files.get(&file_key) {
			if stored_mtime == current_mtime {
				skipped += 1;
				continue;
			}
			// File changed — delete old documents before re-indexing.
			delete_session(&writer, &schema, &session.session_id)?;
		}

		// Index the session file.
		let msg_count = index_session_file(
			&session.path,
			&session.session_id,
			&session.project_path,
			&session.project_name,
			&mut writer,
			f_id,
			f_project,
			f_project_name,
			f_session_id,
			f_git_branch,
			f_role,
			f_timestamp,
			f_content,
		)?;

		indexed_sessions += 1;
		indexed_messages += msg_count;
		meta.files.insert(file_key, current_mtime);
	}

	// Remove documents for sessions that no longer exist on disk.
	let stale_keys: Vec<String> = meta
		.files
		.keys()
		.filter(|k| !seen_files.contains(k.as_str()))
		.cloned()
		.collect();

	for key in &stale_keys {
		// Extract session_id from the file path (stem of the .jsonl).
		if let Some(session_id) = Path::new(key).file_stem().and_then(|s| s.to_str()) {
			delete_session(&writer, &schema, session_id)?;
		}
		meta.files.remove(key);
		removed += 1;
	}

	writer.commit().context("failed to commit index writer")?;
	meta
		.save(&idx_dir)
		.context("failed to save index metadata")?;

	println!(
		"Indexed {indexed_sessions} sessions ({indexed_messages} messages), skipped {skipped} \
		 unchanged, removed {removed}",
	);

	Ok(())
}

/// Index a single JSONL session file, adding one document per parsed
/// message. Returns the number of messages successfully indexed.
#[allow(clippy::too_many_arguments)]
fn index_session_file(
	path: &Path,
	session_id: &str,
	project_path: &str,
	project_name: &str,
	writer: &mut IndexWriter<TantivyDocument>,
	f_id: tantivy::schema::Field,
	f_project: tantivy::schema::Field,
	f_project_name: tantivy::schema::Field,
	f_session_id: tantivy::schema::Field,
	f_git_branch: tantivy::schema::Field,
	f_role: tantivy::schema::Field,
	f_timestamp: tantivy::schema::Field,
	f_content: tantivy::schema::Field,
) -> Result<u64> {
	let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
	let reader = BufReader::new(file);
	let mut count: u64 = 0;

	for line_result in reader.lines() {
		let line = match line_result {
			Ok(l) => l,
			Err(e) => {
				eprintln!("warning: failed to read line from {}: {e}", path.display());
				continue;
			},
		};

		if line.trim().is_empty() {
			continue;
		}

		let parsed = match parse_line(&line, project_path, project_name) {
			Ok(Some(msg)) => msg,
			Ok(None) => continue,
			Err(e) => {
				eprintln!(
					"warning: skipping malformed line in {} (session {}): {e}",
					path.display(),
					session_id,
				);
				continue;
			},
		};

		// Parse the timestamp with chrono and convert to tantivy DateTime.
		let tantivy_dt = match chrono::DateTime::parse_from_rfc3339(&parsed.timestamp) {
			Ok(dt) => DateTime::from_timestamp_secs(dt.timestamp()),
			Err(_) => {
				eprintln!(
					"warning: unparseable timestamp '{}' in session {}, using epoch",
					parsed.timestamp, session_id,
				);
				DateTime::from_timestamp_secs(0)
			},
		};

		let mut doc = TantivyDocument::new();
		doc.add_text(f_id, &parsed.message_uuid);
		doc.add_text(f_project, &parsed.project);
		doc.add_text(f_project_name, &parsed.project_name);
		doc.add_text(f_session_id, &parsed.session_id);
		doc.add_text(f_git_branch, &parsed.git_branch);
		doc.add_text(f_role, &parsed.role);
		doc.add_date(f_timestamp, tantivy_dt);
		doc.add_text(f_content, &parsed.content);

		writer.add_document(doc)?;
		count += 1;
	}

	Ok(count)
}
