#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Language {
	pub name: &'static str,
	pub color: &'static str,
}

const RUST: Language = Language {
	name: "Rust",
	color: "#dea584",
};
const C: Language = Language {
	name: "C",
	color: "#555555",
};
const CPP: Language = Language {
	name: "C++",
	color: "#f34b7d",
};
const NIX: Language = Language {
	name: "Nix",
	color: "#7e7eff",
};
const PYTHON: Language = Language {
	name: "Python",
	color: "#3572A5",
};
const TYPESCRIPT: Language = Language {
	name: "TypeScript",
	color: "#3178c6",
};
const JAVASCRIPT: Language = Language {
	name: "JavaScript",
	color: "#f1e05a",
};
const SHELL: Language = Language {
	name: "Shell",
	color: "#89e051",
};
const MAKEFILE: Language = Language {
	name: "Makefile",
	color: "#427819",
};
const DOCKERFILE: Language = Language {
	name: "Dockerfile",
	color: "#384d54",
};
const GO: Language = Language {
	name: "Go",
	color: "#00ADD8",
};
const JAVA: Language = Language {
	name: "Java",
	color: "#b07219",
};
const LUA: Language = Language {
	name: "Lua",
	color: "#000080",
};
const HTML: Language = Language {
	name: "HTML",
	color: "#e34c26",
};
const CSS: Language = Language {
	name: "CSS",
	color: "#663399",
};
const SCSS: Language = Language {
	name: "SCSS",
	color: "#c6538c",
};
const OPENSCAD: Language = Language {
	name: "OpenSCAD",
	color: "#e5cd45",
};
const GLSL: Language = Language {
	name: "GLSL",
	color: "#5686a5",
};
const BOO: Language = Language {
	name: "Boo",
	color: "#d4bec1",
};
const HASKELL: Language = Language {
	name: "Haskell",
	color: "#5e5086",
};
const ASSEMBLY: Language = Language {
	name: "Assembly",
	color: "#6E4C13",
};
const CMAKE: Language = Language {
	name: "CMake",
	color: "#DA3434",
};
const VIM_SCRIPT: Language = Language {
	name: "Vim script",
	color: "#199f4b",
};

pub(crate) const LANGUAGE_LIST: [Language; 23] = [
	RUST, C, CPP, NIX, PYTHON, TYPESCRIPT, JAVASCRIPT, SHELL, MAKEFILE, DOCKERFILE, GO, JAVA, LUA,
	HTML, CSS, SCSS, OPENSCAD, GLSL, BOO, HASKELL, ASSEMBLY, CMAKE, VIM_SCRIPT,
];

const EXTENSION_LIST: &[(&str, Language)] = &[
	("rs", RUST),
	("c", C),
	("h", C),
	("cpp", CPP),
	("cc", CPP),
	("cxx", CPP),
	("hpp", CPP),
	("hh", CPP),
	("hxx", CPP),
	("nix", NIX),
	("py", PYTHON),
	("pyi", PYTHON),
	("ts", TYPESCRIPT),
	("mts", TYPESCRIPT),
	("cts", TYPESCRIPT),
	("js", JAVASCRIPT),
	("mjs", JAVASCRIPT),
	("cjs", JAVASCRIPT),
	("jsx", JAVASCRIPT),
	("sh", SHELL),
	("bash", SHELL),
	("zsh", SHELL),
	("mk", MAKEFILE),
	("make", MAKEFILE),
	("dockerfile", DOCKERFILE),
	("go", GO),
	("java", JAVA),
	("lua", LUA),
	("html", HTML),
	("htm", HTML),
	("css", CSS),
	("scss", SCSS),
	("scad", OPENSCAD),
	("glsl", GLSL),
	("vert", GLSL),
	("frag", GLSL),
	("boo", BOO),
	("hs", HASKELL),
	("asm", ASSEMBLY),
	("nasm", ASSEMBLY),
	("s", ASSEMBLY),
	("cmake", CMAKE),
	("vim", VIM_SCRIPT),
];

const FILE_NAME_LIST: &[(&str, Language)] = &[
	("Makefile", MAKEFILE),
	("makefile", MAKEFILE),
	("GNUmakefile", MAKEFILE),
	("Dockerfile", DOCKERFILE),
	("Containerfile", DOCKERFILE),
	("CMakeLists.txt", CMAKE),
];

#[must_use]
pub fn language_for_path(path: &str) -> Option<Language> {
	let file_name = path.rsplit_once('/').map_or(path, |(_, name)| name);
	if let Some((_, found)) = FILE_NAME_LIST.iter().find(|(name, _)| *name == file_name) {
		return Some(*found);
	}
	let (_, extension) = file_name.rsplit_once('.')?;
	EXTENSION_LIST
		.iter()
		.find(|(known, _)| known.eq_ignore_ascii_case(extension))
		.map(|(_, found)| *found)
}

#[must_use]
pub fn language_by_name(name: &str) -> Option<Language> {
	LANGUAGE_LIST
		.iter()
		.find(|candidate| candidate.name.eq_ignore_ascii_case(name))
		.copied()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn maps_paths_to_linguist_languages() {
		let cases = [
			("src/main.rs", Some(("Rust", "#dea584"))),
			("a/b.c", Some(("C", "#555555"))),
			("inc/x.h", Some(("C", "#555555"))),
			("x.hpp", Some(("C++", "#f34b7d"))),
			("x.cpp", Some(("C++", "#f34b7d"))),
			("flake.nix", Some(("Nix", "#7e7eff"))),
			("s.py", Some(("Python", "#3572A5"))),
			("app.ts", Some(("TypeScript", "#3178c6"))),
			("run.sh", Some(("Shell", "#89e051"))),
			("Makefile", Some(("Makefile", "#427819"))),
			("Dockerfile", Some(("Dockerfile", "#384d54"))),
			("X.RS", Some(("Rust", "#dea584"))),
			("a/CMakeLists.txt", Some(("CMake", "#DA3434"))),
			("Cargo.lock", None),
			("flake.lock", None),
			("package-lock.json", None),
			("img.png", None),
			("README.md", None),
			("LICENSE", None),
			("dir/", None),
		];
		for (path, expected) in cases {
			let found = language_for_path(path).map(|language| (language.name, language.color));
			assert_eq!(found, expected, "{path}");
		}
	}

	#[test]
	fn finds_language_by_name_ignoring_case() {
		assert_eq!(language_by_name("rust"), Some(RUST));
		assert_eq!(language_by_name("C++"), Some(CPP));
		assert_eq!(language_by_name("nope"), None);
	}

	#[test]
	fn every_extension_and_file_name_targets_a_listed_language() {
		for (_, found) in EXTENSION_LIST.iter().chain(FILE_NAME_LIST) {
			assert!(LANGUAGE_LIST.contains(found));
		}
		for found in LANGUAGE_LIST {
			assert_eq!(language_by_name(found.name), Some(found));
		}
	}
}
