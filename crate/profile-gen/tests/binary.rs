use std::process::Command;

#[test]
fn binary_reports_missing_config() {
	let dir = tempfile::TempDir::new().unwrap();
	let output = Command::new(env!("CARGO_BIN_EXE_profile-gen"))
		.args(["render", "--config"])
		.arg(dir.path().join("missing.toml"))
		.args(["--out"])
		.arg(dir.path().join("out"))
		.args(["--work"])
		.arg(dir.path().join("work"))
		.env("GITHUB_TOKEN", "unused")
		.output()
		.unwrap();
	assert!(!output.status.success());
	assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
}
