mod discovery;
mod index;
mod indexer;
mod meta;
mod pager;
mod parser;
mod search;
mod sessions;
mod stats;
mod tui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ccq", about = "Search your Claude Code conversation history")]
struct Cli {
	/// Disable automatic pager
	#[arg(long, global = true)]
	no_pager: bool,

	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// Build or update the search index
	Index {
		/// Force full rebuild
		#[arg(long)]
		force: bool,
	},
	/// Search conversations
	Search {
		/// Search query
		query: String,
		/// Filter by project name
		#[arg(short, long)]
		project: Option<String>,
		/// Filter by git branch
		#[arg(long)]
		branch: Option<String>,
		/// Show results after this date (YYYY-MM-DD)
		#[arg(long)]
		after: Option<String>,
		/// Show results before this date (YYYY-MM-DD)
		#[arg(long)]
		before: Option<String>,
		/// Verbose output (show individual messages)
		#[arg(short, long)]
		verbose: bool,
		/// Output as JSON
		#[arg(long)]
		json: bool,
		/// Maximum number of results
		#[arg(long, default_value_t = 100)]
		limit: usize,
		/// Show N messages of context around each hit
		#[arg(long)]
		context: Option<usize>,
	},
	/// List and browse sessions
	Sessions {
		/// Session ID to show conversation
		session_id: Option<String>,
		/// Filter by project name
		#[arg(short, long)]
		project: Option<String>,
		/// Output as JSON
		#[arg(long)]
		json: bool,
		/// Show only the first N messages
		#[arg(long, conflicts_with = "tail")]
		head: Option<usize>,
		/// Show only the last N messages
		#[arg(long, conflicts_with = "head")]
		tail: Option<usize>,
	},
	/// Show index statistics
	Stats {
		/// Output as JSON
		#[arg(long)]
		json: bool,
	},
	/// Interactive TUI browser
	Tui {
		/// Optional initial search query
		query: Option<String>,
	},
}

fn main() -> anyhow::Result<()> {
	let cli = Cli::parse();
	match cli.command {
		Commands::Index { force } => {
			let claude_dir = dirs::home_dir()
				.expect("could not find home directory")
				.join(".claude");
			crate::indexer::run_index(&claude_dir, force)?;
		},
		Commands::Search { query, project, branch, after, before, verbose, json, limit, context } => {
			let mut pager = pager::Pager::new(cli.no_pager || json);
			crate::search::run_search(
				crate::search::SearchOptions {
					query,
					project,
					branch,
					after,
					before,
					verbose,
					json,
					limit,
					context,
				},
				pager.writer(),
			)?;
		},
		Commands::Sessions { session_id, project, json, head, tail } => {
			let mut pager = pager::Pager::new(cli.no_pager || json);
			crate::sessions::run_sessions(session_id, project, json, head, tail, pager.writer())?;
		},
		Commands::Stats { json } => {
			crate::stats::run_stats(json)?;
		},
		Commands::Tui { query } => {
			crate::tui::run_tui(query)?;
		},
	}
	Ok(())
}
