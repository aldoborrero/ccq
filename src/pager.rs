use std::io::{self, IsTerminal, Write};
use std::process::{Child, Command, Stdio};

pub struct Pager {
	child: Option<Child>,
	writer: Box<dyn Write>,
}

impl Pager {
	pub fn new(no_pager: bool) -> Self {
		if no_pager || !io::stdout().is_terminal() {
			return Self {
				child: None,
				writer: Box::new(io::stdout()),
			};
		}

		let pager_cmd = std::env::var("PAGER").unwrap_or_else(|_| "less -R".to_string());
		let parts: Vec<&str> = pager_cmd.split_whitespace().collect();
		let (program, args) = match parts.split_first() {
			Some((prog, args)) => (*prog, args),
			None => ("less", ["-R"].as_slice()),
		};

		let mut cmd = Command::new(program);
		cmd.args(args).stdin(Stdio::piped());

		// Set LESS default if not already set (same convention as git).
		// Passed to the child process only — no global env mutation.
		if std::env::var("LESS").is_err() {
			cmd.env("LESS", "FRX");
		}

		match cmd.spawn()
		{
			Ok(mut child) => {
				let stdin = child.stdin.take().expect("failed to open pager stdin");
				Self {
					child: Some(child),
					writer: Box::new(stdin),
				}
			}
			Err(_) => {
				// Pager failed to spawn — fall back to stdout.
				Self {
					child: None,
					writer: Box::new(io::stdout()),
				}
			}
		}
	}

	pub fn writer(&mut self) -> &mut dyn Write {
		&mut *self.writer
	}
}

impl Drop for Pager {
	fn drop(&mut self) {
		// Close the stdin pipe so the pager sees EOF.
		drop(std::mem::replace(&mut self.writer, Box::new(io::sink())));
		// Wait for pager to exit.
		if let Some(ref mut child) = self.child {
			let _ = child.wait();
		}
	}
}
