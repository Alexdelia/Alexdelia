use std::path::Path;
use std::process::Command;

use crate::error::Result;

const LOCAL_ENV_VARS: [&str; 16] = [
	"GIT_DIR",
	"GIT_WORK_TREE",
	"GIT_INDEX_FILE",
	"GIT_OBJECT_DIRECTORY",
	"GIT_ALTERNATE_OBJECT_DIRECTORIES",
	"GIT_CONFIG",
	"GIT_CONFIG_PARAMETERS",
	"GIT_CONFIG_COUNT",
	"GIT_NAMESPACE",
	"GIT_PREFIX",
	"GIT_COMMON_DIR",
	"GIT_GRAFT_FILE",
	"GIT_IMPLICIT_WORK_TREE",
	"GIT_NO_REPLACE_OBJECTS",
	"GIT_REPLACE_REF_BASE",
	"GIT_SHALLOW_FILE",
];

pub(crate) fn command(repo: &Path) -> Command {
	let mut command = Command::new("git");
	command.current_dir(repo);
	for name in LOCAL_ENV_VARS {
		command.env_remove(name);
	}
	command
		.env("GIT_CONFIG_GLOBAL", "/dev/null")
		.env("GIT_CONFIG_NOSYSTEM", "1")
		.env("GIT_TERMINAL_PROMPT", "0");
	command
}

pub(crate) fn failure(
	repo: &Path,
	args: &[&str],
	stderr: &[u8],
) -> Box<dyn std::error::Error + Send + Sync> {
	format!(
		"git {} in {}: {}",
		args[0],
		repo.display(),
		String::from_utf8_lossy(stderr).trim()
	)
	.into()
}

pub(crate) fn head_sha(repo: &Path) -> Result<Option<String>> {
	let output = command(repo)
		.args(["rev-parse", "--verify", "--quiet", "HEAD"])
		.output()?;
	match output.status.code() {
		Some(0) => Ok(Some(
			String::from_utf8_lossy(&output.stdout).trim().to_owned(),
		)),
		Some(1) => Ok(None),
		_ => Err(failure(repo, &["rev-parse"], &output.stderr)),
	}
}

pub(crate) fn unquote_path(printed: &str) -> String {
	let path = printed.split_once('\t').map_or(printed, |(path, _)| path);
	path.strip_prefix('"')
		.and_then(|quoted| quoted.strip_suffix('"'))
		.map_or_else(|| path.to_owned(), unescape)
}

fn unescape(quoted: &str) -> String {
	let mut bytes = Vec::new();
	let mut rest = quoted.as_bytes();
	while let Some((&byte, tail)) = rest.split_first() {
		rest = tail;
		if byte != b'\\' {
			bytes.push(byte);
		} else if let Some(value) = rest.get(..3).and_then(octal) {
			bytes.push(value);
			rest = rest.get(3..).unwrap_or_default();
		} else if let Some((&escaped, tail)) = rest.split_first() {
			bytes.push(control_byte(escaped));
			rest = tail;
		}
	}
	String::from_utf8_lossy(&bytes).into_owned()
}

fn octal(digits: &[u8]) -> Option<u8> {
	std::str::from_utf8(digits)
		.ok()
		.and_then(|text| u8::from_str_radix(text, 8).ok())
}

const fn control_byte(escaped: u8) -> u8 {
	match escaped {
		b'a' => 0x07,
		b'b' => 0x08,
		b't' => b'\t',
		b'n' => b'\n',
		b'v' => 0x0b,
		b'f' => 0x0c,
		b'r' => b'\r',
		other => other,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::ffi::OsStr;

	fn is_removed(command: &Command, name: &str) -> bool {
		command
			.get_envs()
			.any(|(key, value)| key == OsStr::new(name) && value.is_none())
	}

	fn is_set_to(command: &Command, name: &str, expected: &str) -> bool {
		command
			.get_envs()
			.any(|(key, value)| key == OsStr::new(name) && value == Some(OsStr::new(expected)))
	}

	#[test]
	fn local_git_env_is_cleared() {
		let command = command(Path::new("."));
		for name in LOCAL_ENV_VARS {
			assert!(is_removed(&command, name), "{name}");
		}
	}

	#[test]
	fn terminal_prompt_is_disabled() {
		assert!(is_set_to(
			&command(Path::new(".")),
			"GIT_TERMINAL_PROMPT",
			"0"
		));
	}

	#[test]
	fn global_and_system_config_are_disabled() {
		let command = command(Path::new("."));
		assert!(is_set_to(&command, "GIT_CONFIG_GLOBAL", "/dev/null"));
		assert!(is_set_to(&command, "GIT_CONFIG_NOSYSTEM", "1"));
	}

	#[test]
	fn printed_paths_are_unquoted() {
		let cases = [
			("plain.patch", "plain.patch"),
			("a b.patch\t", "a b.patch"),
			("b/a.c\t2026-01-01 10:00", "b/a.c"),
			(r#""\303\251t\303\251.patch""#, "\u{e9}t\u{e9}.patch"),
			(r#""q\"t\\.patch""#, "q\"t\\.patch"),
			(r#""\a\b\t\n\v\f\r""#, "\u{7}\u{8}\t\n\u{b}\u{c}\r"),
			(r#""trailing\""#, "trailing"),
			("\"", "\""),
		];
		for (printed, expected) in cases {
			assert_eq!(unquote_path(printed), expected, "{printed}");
		}
	}
}
