use std::{
	fs,
	path::{Path, PathBuf},
	time::SystemTime,
};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Info about a discovered session file
pub struct SessionFile {
	pub path: PathBuf,
	pub session_id: String,
	pub project_path: String,
	pub project_name: String,
	pub mtime: SystemTime,
}

#[derive(Deserialize)]
struct SessionsIndex {
	#[serde(rename = "originalPath")]
	original_path: Option<String>,
}

/// Discover all JSONL session files under `claude_dir/projects/`.
///
/// Each subdirectory under `projects/` is a project (encoded path like
/// `-home-aldo-Dev-foo`). Within each project dir, `.jsonl` files whose
/// filenames are UUIDs are collected as session files.
pub fn discover_sessions(claude_dir: &Path) -> Result<Vec<SessionFile>> {
	let projects_dir = claude_dir.join("projects");
	if !projects_dir.is_dir() {
		return Ok(Vec::new());
	}

	let mut sessions = Vec::new();

	let project_entries = fs::read_dir(&projects_dir)
		.with_context(|| format!("failed to read {}", projects_dir.display()))?;

	for project_entry in project_entries {
		let project_entry = project_entry?;
		let project_dir = project_entry.path();
		if !project_dir.is_dir() {
			continue;
		}

		let encoded_name = match project_dir.file_name().and_then(|n| n.to_str()) {
			Some(name) => name.to_string(),
			None => continue,
		};

		let project_path = resolve_project_path(&project_dir, &encoded_name);
		let project_name = project_path
			.rsplit('/')
			.find(|s| !s.is_empty())
			.unwrap_or(&encoded_name)
			.to_string();

		let dir_entries = fs::read_dir(&project_dir)
			.with_context(|| format!("failed to read {}", project_dir.display()))?;

		for entry in dir_entries {
			let entry = entry?;
			let file_path = entry.path();

			if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
				continue;
			}

			let session_id = match file_path.file_stem().and_then(|s| s.to_str()) {
				Some(s) if is_uuid(s) => s.to_string(),
				_ => continue,
			};

			let metadata = fs::metadata(&file_path)
				.with_context(|| format!("failed to stat {}", file_path.display()))?;
			let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

			sessions.push(SessionFile {
				path: file_path,
				session_id,
				project_path: project_path.clone(),
				project_name: project_name.clone(),
				mtime,
			});
		}
	}

	Ok(sessions)
}

/// Resolve the real project path for a project directory.
///
/// Reads `sessions-index.json` in the project dir and uses the
/// `originalPath` field if available. Falls back to the lossy
/// `decode_project_path()` if the file is missing or unreadable.
pub fn resolve_project_path(project_dir: &Path, encoded_name: &str) -> String {
	let index_path = project_dir.join("sessions-index.json");
	if let Ok(contents) = fs::read_to_string(&index_path)
		&& let Ok(index) = serde_json::from_str::<SessionsIndex>(&contents)
		&& let Some(original) = index.original_path
		&& !original.is_empty()
	{
		return original;
	}
	decode_project_path(encoded_name)
}

/// Naively decode an encoded project directory name back to a path.
///
/// The encoding replaces `/` with `-`, so `-home-aldo-Dev-foo` becomes
/// `/home/aldo/Dev/foo`. This is lossy: directory names that contain
/// dashes (e.g., `l2-deployer`) will be incorrectly split.
fn decode_project_path(encoded: &str) -> String {
	if encoded.is_empty() {
		return String::new();
	}
	// The encoded name starts with `-` which represents the leading `/`.
	// After that, all `-` are replaced with `/`.
	let without_leading = encoded.strip_prefix('-').unwrap_or(encoded);
	format!("/{}", without_leading.replace('-', "/"))
}

/// Check whether a string looks like a UUID (8-4-4-4-12 hex pattern).
fn is_uuid(s: &str) -> bool {
	let parts: Vec<&str> = s.split('-').collect();
	if parts.len() != 5 {
		return false;
	}
	let expected_lens = [8, 4, 4, 4, 12];
	parts
		.iter()
		.zip(expected_lens.iter())
		.all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_is_uuid() {
		assert!(is_uuid("ae39f778-4341-49a4-b5c9-e86376125be6"));
		assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
		assert!(is_uuid("ABCDEF12-3456-7890-abcd-ef1234567890"));
	}

	#[test]
	fn test_is_uuid_invalid() {
		assert!(!is_uuid("sessions-index"));
		assert!(!is_uuid("not-a-uuid"));
		assert!(!is_uuid(""));
		assert!(!is_uuid("ae39f778-4341-49a4-b5c9")); // too few segments
		assert!(!is_uuid("ae39f778-4341-49a4-b5c9-e86376125be6-extra")); // too many
		assert!(!is_uuid("ae39f77g-4341-49a4-b5c9-e86376125be6")); // 'g' is not hex
	}

	#[test]
	fn test_decode_project_path() {
		assert_eq!(decode_project_path("-home-aldo-Dev-foo"), "/home/aldo/Dev/foo");
		assert_eq!(
			decode_project_path("-home-aldo-Dev-aldoborrero-ccindex"),
			"/home/aldo/Dev/aldoborrero/ccindex"
		);
		assert_eq!(decode_project_path(""), "");
	}

	#[test]
	fn test_decode_project_path_lossy() {
		// Dashed directory names are incorrectly split — this is a known
		// limitation of the naive decode. `l2-deployer` becomes
		// `l2/deployer` instead of staying as `l2-deployer`.
		let decoded = decode_project_path("-home-aldo-Dev-numtide-arkiv-l2-deployer");
		assert_eq!(decoded, "/home/aldo/Dev/numtide/arkiv/l2/deployer");
		// This is wrong! The real path is:
		// /home/aldo/Dev/numtide/arkiv/l2-deployer
		// That's why we prefer sessions-index.json's originalPath.
	}

	#[test]
	fn test_resolve_project_path_fallback() {
		// When no sessions-index.json exists, falls back to decode.
		let path = resolve_project_path(Path::new("/nonexistent/path"), "-home-aldo-Dev-foo");
		assert_eq!(path, "/home/aldo/Dev/foo");
	}

	#[test]
	fn test_discover_sessions_empty_dir() {
		let tmp = std::env::temp_dir().join("ccq-test-discovery");
		let _ = fs::remove_dir_all(&tmp);
		fs::create_dir_all(tmp.join("projects")).unwrap();

		let sessions = discover_sessions(&tmp).unwrap();
		assert!(sessions.is_empty());

		let _ = fs::remove_dir_all(&tmp);
	}

	#[test]
	fn test_discover_sessions_finds_uuid_jsonl() {
		let tmp = std::env::temp_dir().join("ccq-test-discovery-find");
		let _ = fs::remove_dir_all(&tmp);

		let project_dir = tmp.join("projects").join("-home-aldo-Dev-foo");
		fs::create_dir_all(&project_dir).unwrap();

		// Write a UUID .jsonl file
		let uuid = "ae39f778-4341-49a4-b5c9-e86376125be6";
		fs::write(project_dir.join(format!("{uuid}.jsonl")), "{}").unwrap();

		// Write a non-UUID file that should be skipped
		fs::write(project_dir.join("not-a-uuid.jsonl"), "{}").unwrap();

		// Write a non-jsonl file that should be skipped
		fs::write(project_dir.join(format!("{uuid}.json")), "{}").unwrap();

		let sessions = discover_sessions(&tmp).unwrap();
		assert_eq!(sessions.len(), 1);
		assert_eq!(sessions[0].session_id, uuid);
		assert_eq!(sessions[0].project_path, "/home/aldo/Dev/foo");
		assert_eq!(sessions[0].project_name, "foo");

		let _ = fs::remove_dir_all(&tmp);
	}

	#[test]
	fn test_discover_sessions_uses_sessions_index() {
		let tmp = std::env::temp_dir().join("ccq-test-discovery-index");
		let _ = fs::remove_dir_all(&tmp);

		let encoded = "-home-aldo-Dev-numtide-arkiv-l2-deployer";
		let project_dir = tmp.join("projects").join(encoded);
		fs::create_dir_all(&project_dir).unwrap();

		// Write sessions-index.json with originalPath
		let index_content = serde_json::json!({
			"version": 1,
			"entries": [],
			"originalPath": "/home/aldo/Dev/numtide/arkiv/l2-deployer"
		});
		fs::write(
			project_dir.join("sessions-index.json"),
			serde_json::to_string(&index_content).unwrap(),
		)
		.unwrap();

		// Write a UUID .jsonl file
		let uuid = "11111111-2222-3333-4444-555555555555";
		fs::write(project_dir.join(format!("{uuid}.jsonl")), "{}").unwrap();

		let sessions = discover_sessions(&tmp).unwrap();
		assert_eq!(sessions.len(), 1);
		// Should use originalPath, NOT the lossy decode
		assert_eq!(sessions[0].project_path, "/home/aldo/Dev/numtide/arkiv/l2-deployer");
		assert_eq!(sessions[0].project_name, "l2-deployer");

		let _ = fs::remove_dir_all(&tmp);
	}

	#[test]
	fn test_discover_sessions_nonexistent_dir() {
		let result = discover_sessions(Path::new("/nonexistent/path"));
		assert!(result.is_ok());
		assert!(result.unwrap().is_empty());
	}
}
