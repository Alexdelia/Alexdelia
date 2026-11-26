use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::error::Result;
use crate::git;

const API: &str = "https://api.github.com";
const USER_AGENT: &str = "profile-gen";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PAGES: u32 = 100;
const MIRROR_REFSPEC: &str = "+refs/heads/*:refs/heads/*";

pub trait Fetch {
	fn get(&self, url: &str, token: Option<&str>) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct Ureq;

impl Fetch for Ureq {
	fn get(&self, url: &str, token: Option<&str>) -> Result<String> {
		let agent: ureq::Agent = ureq::Agent::config_builder()
			.timeout_global(Some(REQUEST_TIMEOUT))
			.build()
			.into();
		let mut request = agent.get(url).header("User-Agent", USER_AGENT);
		if let Some(token) = token {
			request = request.header("Authorization", format!("Bearer {token}"));
		}
		let mut response = request.call().map_err(typed_status)?;
		Ok(response.body_mut().read_to_string()?)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotFound;

impl std::fmt::Display for NotFound {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("not found")
	}
}

impl std::error::Error for NotFound {}

fn typed_status(error: ureq::Error) -> Box<dyn std::error::Error + Send + Sync> {
	match error {
		ureq::Error::StatusCode(404) => Box::new(NotFound),
		other => other.into(),
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
	pub owner: String,
	pub name: String,
	pub clone_url: String,
}

impl RepoInfo {
	#[must_use]
	pub fn full_name(&self) -> String {
		full_name(&self.owner, &self.name)
	}
}

fn full_name(owner: &str, name: &str) -> String {
	format!("{owner}/{name}")
}

#[derive(Deserialize)]
struct Owner {
	login: String,
}

#[derive(Deserialize)]
struct Repo {
	owner: Owner,
	name: String,
	#[serde(default)]
	fork: bool,
	#[serde(default)]
	private: bool,
	clone_url: Option<String>,
	ssh_url: Option<String>,
}

fn list_pages(
	fetch: &dyn Fetch,
	token: Option<&str>,
	page_url: &dyn Fn(u32) -> String,
) -> Result<Vec<Repo>> {
	let mut repos = Vec::new();
	for page in 1..=MAX_PAGES {
		let body = fetch.get(&page_url(page), token)?;
		let page_repos: Vec<Repo> = serde_json::from_str(&body)?;
		if page_repos.is_empty() {
			return Ok(repos);
		}
		repos.extend(page_repos);
	}
	Err(format!("more than {MAX_PAGES} pages of repos").into())
}

impl Repo {
	fn is_excluded(&self, exclude: &[String]) -> bool {
		exclude.contains(&full_name(&self.owner.login, &self.name))
	}
}

fn into_info(owner: Owner, name: String, url: Option<String>) -> Result<RepoInfo> {
	let clone_url =
		url.ok_or_else(|| format!("repo {} has no clone url", full_name(&owner.login, &name)))?;
	Ok(RepoInfo {
		owner: owner.login,
		name,
		clone_url,
	})
}

pub fn list_public(
	fetch: &impl Fetch,
	owner: &str,
	token: Option<&str>,
	exclude: &[String],
) -> Result<Vec<RepoInfo>> {
	public_repos(fetch, owner, token, exclude)
}

fn public_repos(
	fetch: &dyn Fetch,
	owner: &str,
	token: Option<&str>,
	exclude: &[String],
) -> Result<Vec<RepoInfo>> {
	list_pages(fetch, token, &|page| {
		format!("{API}/users/{owner}/repos?type=owner&per_page=100&page={page}")
	})?
	.into_iter()
	.filter(|repo| !repo.fork && !repo.private && !repo.is_excluded(exclude))
	.map(|repo| into_info(repo.owner, repo.name, repo.clone_url))
	.collect()
}

pub fn list_private(fetch: &impl Fetch, token: &str, exclude: &[String]) -> Result<Vec<RepoInfo>> {
	private_repos(fetch, token, exclude)
}

fn private_repos(fetch: &dyn Fetch, token: &str, exclude: &[String]) -> Result<Vec<RepoInfo>> {
	list_pages(fetch, Some(token), &|page| {
		format!("{API}/user/repos?visibility=private&affiliation=owner&per_page=100&page={page}")
	})?
	.into_iter()
	.filter(|repo| !repo.fork && !repo.is_excluded(exclude))
	.map(|repo| into_info(repo.owner, repo.name, repo.ssh_url))
	.collect()
}

pub fn list_contributed(
	fetch: &impl Fetch,
	token: Option<&str>,
	list: &[String],
) -> Result<Vec<RepoInfo>> {
	contributed_repos(fetch, token, list)
}

fn contributed_repos(
	fetch: &dyn Fetch,
	token: Option<&str>,
	list: &[String],
) -> Result<Vec<RepoInfo>> {
	let mut repos = Vec::new();
	for entry in list {
		let body = match fetch.get(&format!("{API}/repos/{entry}"), token) {
			Ok(body) => body,
			Err(error) if error.is::<NotFound>() => continue,
			Err(error) => return Err(error),
		};
		let repo: Repo = serde_json::from_str(&body)?;
		if !repo.private {
			repos.push(into_info(repo.owner, repo.name, repo.clone_url)?);
		}
	}
	Ok(repos)
}

fn validate_name(name: &str) -> Result<()> {
	let is_safe_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
	if name.is_empty()
		|| name == "."
		|| name == ".."
		|| name.starts_with('-')
		|| !name.chars().all(is_safe_char)
	{
		return Err(format!("unsafe repo name {name:?}").into());
	}
	Ok(())
}

pub(crate) fn validate_full_name(entry: &str) -> Result<()> {
	let (owner, name) = entry
		.split_once('/')
		.ok_or_else(|| format!("{entry:?} is not <owner>/<repo>"))?;
	validate_name(owner)?;
	validate_name(name)
}

fn run_git(mut command: Command, cwd: &Path, subcommand: &str) -> Result<()> {
	let output = command.output()?;
	if output.status.success() {
		Ok(())
	} else {
		Err(git::failure(cwd, &[subcommand], &output.stderr))
	}
}

fn fetch_mirror(path: &Path) -> Result<()> {
	let mut command = git::command(path);
	command
		.env("GIT_DIR", ".")
		.args(["fetch", "-q", "--prune", "origin", MIRROR_REFSPEC]);
	run_git(command, path, "fetch")
}

fn clone_bare(info: &RepoInfo, owner_dir: &Path, dir_name: &str) -> Result<()> {
	std::fs::create_dir_all(owner_dir)?;
	let mut command = git::command(owner_dir);
	command
		.args(["clone", "-q", "--bare", "--"])
		.args([info.clone_url.as_str(), dir_name]);
	run_git(command, owner_dir, "clone")
}

pub fn sync_clone(info: &RepoInfo, work: &Path) -> Result<PathBuf> {
	validate_name(&info.owner)?;
	validate_name(&info.name)?;
	let owner_dir = work.join(&info.owner);
	let dir_name = format!("{}.git", info.name);
	let path = owner_dir.join(&dir_name);
	if path.exists() {
		fetch_mirror(&path)?;
	} else {
		clone_bare(info, &owner_dir, &dir_name)?;
	}
	Ok(path)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fixture::FixtureRepo;
	use crate::git;
	use std::cell::RefCell;
	use std::collections::HashMap;
	use std::io::{Read, Write};
	use std::net::TcpListener;
	use tempfile::TempDir;

	struct Fake {
		pages: HashMap<String, String>,
		seen: RefCell<Vec<(String, Option<String>)>>,
	}

	impl Fake {
		fn new(pages: &[(&str, &str)]) -> Self {
			Self {
				pages: pages
					.iter()
					.map(|(url, body)| ((*url).to_owned(), (*body).to_owned()))
					.collect(),
				seen: RefCell::default(),
			}
		}
	}

	impl Fetch for Fake {
		fn get(&self, url: &str, token: Option<&str>) -> Result<String> {
			self.seen
				.borrow_mut()
				.push((url.to_owned(), token.map(str::to_owned)));
			self.pages
				.get(url)
				.cloned()
				.ok_or_else(|| format!("no page for {url}").into())
		}
	}

	const PUBLIC_PAGE_1: &str = r#"[
		{"owner":{"login":"o"},"name":"keep","fork":false,"private":false,"clone_url":"https://github.com/o/keep.git","ssh_url":"git@github.com:o/keep.git","id":1},
		{"owner":{"login":"o"},"name":"forked","fork":true,"private":false,"clone_url":"https://github.com/o/forked.git"},
		{"owner":{"login":"o"},"name":"hidden","fork":false,"private":true,"clone_url":"https://github.com/o/hidden.git"},
		{"owner":{"login":"o"},"name":"skipped","fork":false,"private":false,"clone_url":"https://github.com/o/skipped.git"}
	]"#;
	const PUBLIC_PAGE_2: &str = r#"[
		{"owner":{"login":"o"},"name":"second","fork":false,"private":false,"clone_url":"https://github.com/o/second.git"}
	]"#;

	fn public_url(page: u32) -> String {
		format!("https://api.github.com/users/o/repos?type=owner&per_page=100&page={page}")
	}

	fn private_url(page: u32) -> String {
		format!(
			"https://api.github.com/user/repos?visibility=private&affiliation=owner&per_page=100&page={page}"
		)
	}

	fn repo_info(owner: &str, name: &str, clone_url: &str) -> RepoInfo {
		RepoInfo {
			owner: owner.to_owned(),
			name: name.to_owned(),
			clone_url: clone_url.to_owned(),
		}
	}

	fn info(name: &str, clone_url: &str) -> RepoInfo {
		repo_info("o", name, clone_url)
	}

	#[test]
	fn list_public_paginates_and_drops_forks_private_excluded() {
		let fake = Fake::new(&[
			(&public_url(1), PUBLIC_PAGE_1),
			(&public_url(2), PUBLIC_PAGE_2),
			(&public_url(3), "[]"),
		]);
		let list = list_public(&fake, "o", Some("tok"), &["o/skipped".to_owned()]).unwrap();
		assert_eq!(
			list,
			[
				info("keep", "https://github.com/o/keep.git"),
				info("second", "https://github.com/o/second.git"),
			]
		);
		assert!(
			fake.seen
				.borrow()
				.iter()
				.all(|(_, token)| token.as_deref() == Some("tok"))
		);
	}

	#[test]
	fn public_listing_excludes_by_full_name() {
		let fake = Fake::new(&[
			(&public_url(1), PUBLIC_PAGE_1),
			(&public_url(2), PUBLIC_PAGE_2),
			(&public_url(3), "[]"),
		]);
		let names = |exclude: &[&str]| -> Vec<String> {
			let exclude: Vec<String> = exclude.iter().map(|entry| (*entry).to_owned()).collect();
			list_public(&fake, "o", None, &exclude)
				.unwrap()
				.iter()
				.map(RepoInfo::full_name)
				.collect()
		};
		assert_eq!(
			names(&["skipped", "other/keep"]),
			["o/keep", "o/skipped", "o/second"]
		);
		assert_eq!(names(&["o/skipped", "o/keep"]), ["o/second"]);
	}

	fn contributed_url(full_name: &str) -> String {
		format!("https://api.github.com/repos/{full_name}")
	}

	struct Failing;

	impl Fetch for Failing {
		fn get(&self, url: &str, _token: Option<&str>) -> Result<String> {
			if url.ends_with("/gone") {
				Err(Box::new(NotFound))
			} else {
				Err(format!("network down for {url}").into())
			}
		}
	}

	#[test]
	fn contributed_repo_not_found_is_skipped() {
		let list = list_contributed(&Failing, None, &["friend/gone".to_owned()]).unwrap();
		assert_eq!(list, []);
	}

	#[test]
	fn contributed_private_repo_is_skipped() {
		let fake = Fake::new(&[
			(
				&contributed_url("friend/hidden"),
				r#"{"owner":{"login":"friend"},"name":"hidden","fork":false,"private":true,"clone_url":"https://github.com/friend/hidden.git"}"#,
			),
			(
				&contributed_url("friend/open"),
				r#"{"owner":{"login":"friend"},"name":"open","fork":false,"private":false,"clone_url":"https://github.com/friend/open.git"}"#,
			),
		]);
		let list = list_contributed(
			&fake,
			Some("tok"),
			&["friend/hidden".to_owned(), "friend/open".to_owned()],
		)
		.unwrap();
		assert_eq!(
			list,
			[repo_info(
				"friend",
				"open",
				"https://github.com/friend/open.git"
			)]
		);
		assert!(
			fake.seen
				.borrow()
				.iter()
				.all(|(_, token)| token.as_deref() == Some("tok"))
		);
	}

	#[test]
	fn contributed_network_error_fails() {
		let error = list_contributed(
			&Failing,
			None,
			&["friend/gone".to_owned(), "friend/up".to_owned()],
		)
		.unwrap_err();
		assert!(error.to_string().contains("network down"), "{error}");
		let fake = Fake::new(&[(&contributed_url("friend/bad"), "not json")]);
		assert!(list_contributed(&fake, None, &["friend/bad".to_owned()]).is_err());
	}

	#[test]
	fn contributed_fork_is_kept() {
		let fake = Fake::new(&[(
			&contributed_url("Friend/Fork"),
			r#"{"owner":{"login":"friend"},"name":"fork","fork":true,"private":false,"clone_url":"https://github.com/friend/fork.git"}"#,
		)]);
		let list = list_contributed(&fake, None, &["Friend/Fork".to_owned()]).unwrap();
		assert_eq!(
			list,
			[repo_info(
				"friend",
				"fork",
				"https://github.com/friend/fork.git"
			)]
		);
		assert_eq!(list[0].full_name(), "friend/fork");
	}

	#[test]
	fn list_public_without_token_sends_none() {
		let fake = Fake::new(&[(&public_url(1), "[]")]);
		assert_eq!(list_public(&fake, "o", None, &[]).unwrap(), []);
		assert_eq!(fake.seen.borrow()[0].1, None);
	}

	#[test]
	fn list_private_uses_ssh_url_and_drops_forks() {
		let page = r#"[
			{"owner":{"login":"o"},"name":"secret","fork":false,"private":true,"ssh_url":"git@github.com:o/secret.git","clone_url":"https://github.com/o/secret.git"},
			{"owner":{"login":"o"},"name":"forked","fork":true,"private":true,"ssh_url":"git@github.com:o/forked.git"},
			{"owner":{"login":"o"},"name":"skipped","fork":false,"private":true,"ssh_url":"git@github.com:o/skipped.git"}
		]"#;
		let fake = Fake::new(&[(&private_url(1), page), (&private_url(2), "[]")]);
		let list = list_private(&fake, "tok", &["o/skipped".to_owned()]).unwrap();
		assert_eq!(list, [info("secret", "git@github.com:o/secret.git")]);
		assert_eq!(fake.seen.borrow()[0].1.as_deref(), Some("tok"));
	}

	#[test]
	fn fetch_error_propagates() {
		let fake = Fake::new(&[]);
		let error = list_public(&fake, "o", None, &[]).unwrap_err();
		assert!(error.to_string().contains("no page for"));
		assert!(list_private(&fake, "tok", &[]).is_err());
	}

	#[test]
	fn invalid_json_is_an_error() {
		let fake = Fake::new(&[(&public_url(1), "not json")]);
		assert!(list_public(&fake, "o", None, &[]).is_err());
	}

	#[test]
	fn missing_url_field_is_an_error() {
		let fake = Fake::new(&[
			(
				&public_url(1),
				r#"[{"owner":{"login":"o"},"name":"x","fork":false}]"#,
			),
			(&public_url(2), "[]"),
		]);
		let error = list_public(&fake, "o", None, &[]).unwrap_err();
		assert!(error.to_string().contains("o/x"), "{error}");

		let fake = Fake::new(&[(
			&contributed_url("friend/y"),
			r#"{"owner":{"login":"friend"},"name":"y","fork":false,"private":false}"#,
		)]);
		let error = list_contributed(&fake, None, &["friend/y".to_owned()]).unwrap_err();
		assert!(error.to_string().contains("friend/y"), "{error}");
	}

	struct Endless;

	impl Fetch for Endless {
		fn get(&self, _url: &str, _token: Option<&str>) -> Result<String> {
			Ok(PUBLIC_PAGE_2.to_owned())
		}
	}

	#[test]
	fn endless_pagination_stops_with_error() {
		let error = list_public(&Endless, "o", None, &[]).unwrap_err();
		assert!(error.to_string().contains("pages"));
	}

	fn serve_once(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		let url = format!(
			"http://{}/users/o/repos?page=1",
			listener.local_addr().unwrap()
		);
		let handle = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut request = Vec::new();
			let mut buffer = [0_u8; 1024];
			while !request.ends_with(b"\r\n\r\n") {
				let read = stream.read(&mut buffer).unwrap();
				request.extend_from_slice(&buffer[..read]);
			}
			stream.write_all(response.as_bytes()).unwrap();
			String::from_utf8(request).unwrap()
		});
		(url, handle)
	}

	const OK_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\n[42]";

	#[test]
	fn ureq_sends_user_agent_and_bearer_token() {
		let (url, handle) = serve_once(OK_RESPONSE);
		assert_eq!(Ureq.get(&url, Some("secret-token")).unwrap(), "[42]");
		let request = handle.join().unwrap().to_ascii_lowercase();
		assert!(request.starts_with("get /users/o/repos?page=1 http/1.1\r\n"));
		assert!(request.contains("user-agent: profile-gen\r\n"));
		assert!(request.contains("authorization: bearer secret-token\r\n"));
	}

	#[test]
	fn ureq_omits_authorization_without_token() {
		let (url, handle) = serve_once(OK_RESPONSE);
		assert_eq!(Ureq.get(&url, None).unwrap(), "[42]");
		assert!(
			!handle
				.join()
				.unwrap()
				.to_ascii_lowercase()
				.contains("authorization")
		);
	}

	#[test]
	fn ureq_not_found_is_typed() {
		let (url, handle) =
			serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
		let error = Ureq.get(&url, None).unwrap_err();
		assert!(error.downcast_ref::<NotFound>().is_some(), "{error}");
		assert_eq!(error.to_string(), "not found");
		handle.join().unwrap();
	}

	#[test]
	fn ureq_http_error_status_is_an_error() {
		let (url, handle) =
			serve_once("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
		let error = Ureq.get(&url, Some("secret-token")).unwrap_err();
		assert!(error.downcast_ref::<NotFound>().is_none(), "{error}");
		handle.join().unwrap();
	}

	#[test]
	fn ureq_error_never_leaks_token() {
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		let url = format!("http://{}/", listener.local_addr().unwrap());
		drop(listener);
		let error = Ureq.get(&url, Some("secret-token")).unwrap_err();
		assert!(!format!("{error} {error:?}").contains("secret-token"));
	}

	#[test]
	fn ureq_truncated_body_is_an_error() {
		let (url, handle) =
			serve_once("HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n[42]");
		assert!(Ureq.get(&url, None).is_err());
		handle.join().unwrap();
	}

	#[test]
	fn sync_clone_into_a_file_is_an_error() {
		let work = TempDir::new().unwrap();
		let blocker = work.path().join("not-a-dir");
		std::fs::write(&blocker, "").unwrap();
		assert!(sync_clone(&info("repo", "irrelevant"), &blocker).is_err());
	}

	#[test]
	fn sync_clone_with_unrunnable_existing_target_is_an_error() {
		let work = TempDir::new().unwrap();
		std::fs::create_dir(work.path().join("o")).unwrap();
		std::fs::write(work.path().join("o/repo.git"), "").unwrap();
		assert!(sync_clone(&info("repo", "irrelevant"), work.path()).is_err());
	}

	fn clone_head(clone: &Path) -> String {
		let output = git::command(clone)
			.args(["rev-parse", "main"])
			.output()
			.unwrap();
		assert!(output.status.success());
		String::from_utf8(output.stdout).unwrap().trim().to_owned()
	}

	fn source_info(source: &FixtureRepo) -> RepoInfo {
		info("repo", source.path().to_str().unwrap())
	}

	#[test]
	fn sync_clone_clones_then_fetches_new_commits() {
		let source = FixtureRepo::new();
		source.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let work = TempDir::new().unwrap();
		let info = source_info(&source);

		let path = sync_clone(&info, work.path()).unwrap();
		assert_eq!(path, work.path().join("o/repo.git"));
		assert_eq!(clone_head(&path), source.head());

		source.commit("a@example.com", 2, &[("b.c", Some("int b;"))]);
		assert_eq!(sync_clone(&info, work.path()).unwrap(), path);
		assert_eq!(clone_head(&path), source.head());
	}

	#[test]
	fn sync_clone_nests_under_owner() {
		let mine = FixtureRepo::new();
		mine.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let theirs = FixtureRepo::new();
		theirs.commit("a@example.com", 2, &[("b.c", Some("int b;"))]);
		let work = TempDir::new().unwrap();
		let source = |owner: &str, repo: &FixtureRepo| {
			repo_info(owner, "alpha", repo.path().to_str().unwrap())
		};

		let mine_path = sync_clone(&source("me", &mine), work.path()).unwrap();
		let theirs_path = sync_clone(&source("friend", &theirs), work.path()).unwrap();

		assert_eq!(mine_path, work.path().join("me/alpha.git"));
		assert_eq!(theirs_path, work.path().join("friend/alpha.git"));
		assert_eq!(clone_head(&mine_path), mine.head());
		assert_eq!(clone_head(&theirs_path), theirs.head());
		assert!(!work.path().join("alpha.git").exists());
	}

	#[test]
	fn sync_clone_works_with_relative_work_dir() {
		let source = FixtureRepo::new();
		source.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let scratch = TempDir::new_in(".").unwrap();
		let work = Path::new(scratch.path().file_name().unwrap()).join("work");
		assert!(work.is_relative());
		let info = source_info(&source);
		let path = sync_clone(&info, &work).unwrap();

		source.commit("a@example.com", 2, &[("b.c", Some("int b;"))]);
		sync_clone(&info, &work).unwrap();
		assert_eq!(clone_head(&path), source.head());
	}

	#[test]
	fn sync_clone_follows_rewritten_history() {
		let source = FixtureRepo::new();
		source.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		source.commit("a@example.com", 2, &[("b.c", Some("int b;"))]);
		let work = TempDir::new().unwrap();
		let info = source_info(&source);
		let path = sync_clone(&info, work.path()).unwrap();

		source.amend("a@example.com", 3, &[("b.c", "int rewritten;")]);
		sync_clone(&info, work.path()).unwrap();
		assert_eq!(clone_head(&path), source.head());
	}

	#[test]
	fn sync_clone_prunes_deleted_branches() {
		let source = FixtureRepo::new();
		source.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let status = git::command(source.path())
			.args(["branch", "extra"])
			.status()
			.unwrap();
		assert!(status.success());
		let work = TempDir::new().unwrap();
		let info = source_info(&source);
		let path = sync_clone(&info, work.path()).unwrap();
		assert_eq!(clone_head(&path), source.head());
		assert!(
			git::command(&path)
				.args(["rev-parse", "extra"])
				.status()
				.unwrap()
				.success()
		);

		let status = git::command(source.path())
			.args(["branch", "-D", "extra"])
			.status()
			.unwrap();
		assert!(status.success());
		sync_clone(&info, work.path()).unwrap();
		assert!(
			!git::command(&path)
				.args(["rev-parse", "extra"])
				.status()
				.unwrap()
				.success()
		);
	}

	#[test]
	fn sync_clone_failure_reports_git_error() {
		let work = TempDir::new().unwrap();
		let missing = work.path().join("missing-source");
		let error = sync_clone(&info("repo", missing.to_str().unwrap()), work.path()).unwrap_err();
		assert!(error.to_string().starts_with("git clone"));
	}

	#[test]
	fn sync_clone_fetch_failure_reports_git_error() {
		let source = FixtureRepo::new();
		source.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let work = TempDir::new().unwrap();
		let path = sync_clone(&source_info(&source), work.path()).unwrap();
		drop(source);
		let error = sync_clone(&info("repo", "ignored"), work.path()).unwrap_err();
		assert!(error.to_string().starts_with("git fetch"));
		assert!(path.exists());
	}

	#[test]
	fn fetch_in_non_repository_never_touches_enclosing_repository() {
		let outer = FixtureRepo::new();
		outer.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		for branch in ["localwork", "feature"] {
			let status = git::command(outer.path())
				.args(["branch", branch])
				.status()
				.unwrap();
			assert!(status.success());
		}
		let status = git::command(outer.path())
			.args(["checkout", "-q", "--detach"])
			.status()
			.unwrap();
		assert!(status.success());
		let upstream = FixtureRepo::new();
		upstream.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let status = git::command(outer.path())
			.args(["remote", "add", "origin"])
			.arg(upstream.path())
			.status()
			.unwrap();
		assert!(status.success());
		std::fs::create_dir_all(outer.path().join("o/repo.git")).unwrap();

		let error = sync_clone(&info("repo", "irrelevant"), outer.path()).unwrap_err();

		assert!(error.to_string().starts_with("git fetch"));
		let output = git::command(outer.path())
			.args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
			.output()
			.unwrap();
		let mut branches: Vec<_> = String::from_utf8(output.stdout)
			.unwrap()
			.lines()
			.map(str::to_owned)
			.collect();
		branches.sort();
		assert_eq!(branches, ["feature", "localwork", "main"]);
	}

	#[test]
	fn clone_url_starting_with_dash_is_not_an_option() {
		let work = TempDir::new().unwrap();
		let error =
			sync_clone(&info("repo", "--upload-pack=touch pwned"), work.path()).unwrap_err();
		assert!(
			error
				.to_string()
				.contains("repository '--upload-pack=touch pwned'"),
			"{error}"
		);
		assert!(!work.path().join("pwned").exists());
	}

	#[test]
	fn unsafe_names_are_rejected() {
		let work = TempDir::new().unwrap();
		for name in [
			"",
			".",
			"..",
			"a/b",
			"../x",
			"a b",
			"-x",
			"r\u{e9}po",
			"a\\b",
		] {
			let error = sync_clone(&info(name, "irrelevant"), work.path()).unwrap_err();
			assert!(error.to_string().contains("unsafe repo name"), "{name:?}");
			let error =
				sync_clone(&repo_info(name, "repo", "irrelevant"), work.path()).unwrap_err();
			assert!(
				error.to_string().contains("unsafe repo name"),
				"owner {name:?}"
			);
		}
		assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
	}

	#[test]
	fn safe_names_are_accepted_by_validation() {
		for name in ["a", "my-repo", "my_repo.js", "A1.b-c_d"] {
			assert!(validate_name(name).is_ok(), "{name:?}");
		}
	}
}
