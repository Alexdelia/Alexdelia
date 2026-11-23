use std::path::Path;
use std::process::Command;

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
		.env("GIT_CONFIG_NOSYSTEM", "1");
	command
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
	fn global_and_system_config_are_disabled() {
		let command = command(Path::new("."));
		assert!(is_set_to(&command, "GIT_CONFIG_GLOBAL", "/dev/null"));
		assert!(is_set_to(&command, "GIT_CONFIG_NOSYSTEM", "1"));
	}
}
