use std::process::Command;

#[test]
fn binary_fails_while_not_implemented() {
	let output = Command::new(env!("CARGO_BIN_EXE_profile-gen"))
		.arg("render")
		.output()
		.unwrap();
	assert!(!output.status.success());
	assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented"));
}
