const ICON_LIST: [(&str, &str); 20] = [
	("Rust", include_str!("../asset/icon/rust.svg")),
	("C", include_str!("../asset/icon/c.svg")),
	("C++", include_str!("../asset/icon/cplusplus.svg")),
	("Nix", include_str!("../asset/icon/nixos.svg")),
	("Python", include_str!("../asset/icon/python.svg")),
	("TypeScript", include_str!("../asset/icon/typescript.svg")),
	("JavaScript", include_str!("../asset/icon/javascript.svg")),
	("Shell", include_str!("../asset/icon/gnubash.svg")),
	("Dockerfile", include_str!("../asset/icon/docker.svg")),
	("Go", include_str!("../asset/icon/go.svg")),
	("Java", include_str!("../asset/icon/openjdk.svg")),
	("Lua", include_str!("../asset/icon/lua.svg")),
	("HTML", include_str!("../asset/icon/html5.svg")),
	("CSS", include_str!("../asset/icon/css.svg")),
	("SCSS", include_str!("../asset/icon/sass.svg")),
	("OpenSCAD", include_str!("../asset/icon/openscad.svg")),
	("GLSL", include_str!("../asset/icon/opengl.svg")),
	("Haskell", include_str!("../asset/icon/haskell.svg")),
	("CMake", include_str!("../asset/icon/cmake.svg")),
	("Vim script", include_str!("../asset/icon/vim.svg")),
];

#[must_use]
pub fn icon_path(language: &str) -> Option<&'static str> {
	ICON_LIST
		.iter()
		.find(|(name, _)| *name == language)
		.and_then(|(_, svg)| path_data(svg))
}

fn path_data(svg: &'static str) -> Option<&'static str> {
	let (_, rest) = svg.split_once(" d=\"")?;
	rest.split_once('"').map(|(data, _)| data)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::language::language_by_name;

	#[test]
	fn every_icon_belongs_to_a_known_language() {
		for (name, _) in ICON_LIST {
			assert_eq!(
				language_by_name(name).map(|language| language.name),
				Some(name)
			);
		}
	}

	#[test]
	fn icon_paths_are_single_simple_icons_paths() {
		for (name, svg) in ICON_LIST {
			assert!(svg.contains("viewBox=\"0 0 24 24\""), "{name}");
			assert_eq!(svg.matches("<path").count(), 1, "{name}");
			let path = icon_path(name).unwrap();
			assert!(path.starts_with('M'), "{name}");
			assert!(!path.contains('"'), "{name}");
		}
	}

	#[test]
	fn languages_without_icon_fall_back() {
		for name in [
			"Makefile",
			"Boo",
			"Assembly",
			"Perl",
			"Linker Script",
			"Unknown",
		] {
			assert_eq!(icon_path(name), None, "{name}");
		}
	}

	#[test]
	fn path_data_requires_a_d_attribute() {
		assert_eq!(path_data("<svg><path/></svg>"), None);
		assert_eq!(path_data("<path d=\"M1 2z\"/>"), Some("M1 2z"));
	}
}
