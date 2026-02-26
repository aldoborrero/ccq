use std::{
	collections::HashSet,
	fs::File,
	io::{BufRead, BufReader},
	path::Path,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use anyhow::{Context, Result};
use tantivy::{DateTime, IndexWriter, schema::TantivyDocument};

use crate::{
	discovery::discover_sessions,
	doc::SchemaFields,
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
	let fields = SchemaFields::resolve(&schema)?;

	// Set up Ctrl+C handler so we can commit partial progress.
	let interrupted = Arc::new(AtomicBool::new(false));
	let flag = Arc::clone(&interrupted);
	ctrlc::set_handler(move || {
		flag.store(true, Ordering::Relaxed);
	})
	.context("failed to set Ctrl+C handler")?;

	let mut indexed_sessions: u64 = 0;
	let mut indexed_messages: u64 = 0;
	let mut skipped: u64 = 0;
	let mut removed: u64 = 0;

	// Track which file paths we see so we can detect deletions.
	let mut seen_files: HashSet<String> = HashSet::new();

	for session in &sessions {
		if interrupted.load(Ordering::Relaxed) {
			eprintln!("\nInterrupted — committing progress so far...");
			break;
		}
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
			&fields,
		)?;

		indexed_sessions += 1;
		indexed_messages += msg_count;
		meta.files.insert(file_key, current_mtime);
	}

	// Remove documents for sessions that no longer exist on disk.
	// Skip this step if interrupted — we only saw a partial set of files.
	if !interrupted.load(Ordering::Relaxed) {
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
fn index_session_file(
	path: &Path,
	session_id: &str,
	project_path: &str,
	project_name: &str,
	writer: &mut IndexWriter<TantivyDocument>,
	fields: &SchemaFields,
) -> Result<u64> {
	let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
	let reader = BufReader::new(file);
	let mut count: u64 = 0;
	let mut total_lines: u64 = 0;
	let mut skipped_lines: u64 = 0;

	for line_result in reader.lines() {
		let line = match line_result {
			Ok(l) => l,
			Err(_) => {
				total_lines += 1;
				skipped_lines += 1;
				continue;
			},
		};

		if line.trim().is_empty() {
			continue;
		}

		total_lines += 1;

		let parsed = match parse_line(&line, project_path, project_name) {
			Ok(Some(msg)) => msg,
			Ok(None) => continue,
			Err(_) => {
				skipped_lines += 1;
				continue;
			},
		};

		// Parse the timestamp with chrono and convert to tantivy DateTime.
		let tantivy_dt = match chrono::DateTime::parse_from_rfc3339(&parsed.timestamp) {
			Ok(dt) => DateTime::from_timestamp_secs(dt.timestamp()),
			Err(_) => DateTime::from_timestamp_secs(0),
		};

		let mut doc = TantivyDocument::new();
		doc.add_text(fields.id, &parsed.message_uuid);
		doc.add_text(fields.project, &parsed.project);
		doc.add_text(fields.project_name, &parsed.project_name);
		doc.add_text(fields.session_id, &parsed.session_id);
		doc.add_text(fields.git_branch, &parsed.git_branch);
		doc.add_text(fields.role, &parsed.role);
		doc.add_date(fields.timestamp, tantivy_dt);
		doc.add_text(fields.content, &parsed.content);

		writer.add_document(doc)?;
		count += 1;
	}

	if skipped_lines > 0 && total_lines > 0 {
		let skip_pct = (skipped_lines as f64 / total_lines as f64) * 100.0;
		if skip_pct > 10.0 {
			eprintln!(
				"warning: {skipped_lines}/{total_lines} lines ({skip_pct:.0}%) skipped in {} \
				 (session {session_id}) — file may be corrupt",
				path.display(),
			);
		}
	}

	Ok(count)
}
