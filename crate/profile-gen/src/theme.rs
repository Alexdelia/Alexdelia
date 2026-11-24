#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
	Dark,
	Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
	pub text: &'static str,
	pub muted: &'static str,
	pub ground: &'static str,
	pub belt: &'static str,
	pub belt_accent: &'static str,
}

pub const FONT_STACK: &str =
	"-apple-system,BlinkMacSystemFont,'Segoe UI','Noto Sans',Helvetica,Arial,sans-serif";

const DARK: Palette = Palette {
	text: "#e6edf3",
	muted: "#9198a1",
	ground: "#3d444d",
	belt: "#6e7681",
	belt_accent: "#d29922",
};

const LIGHT: Palette = Palette {
	text: "#1f2328",
	muted: "#59636e",
	ground: "#d1d9e0",
	belt: "#818b98",
	belt_accent: "#bf8700",
};

#[must_use]
pub const fn palette(variant: Variant) -> Palette {
	match variant {
		Variant::Dark => DARK,
		Variant::Light => LIGHT,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::language::LANGUAGE_LIST;

	fn color_list(palette: Palette) -> [&'static str; 5] {
		[
			palette.text,
			palette.muted,
			palette.ground,
			palette.belt,
			palette.belt_accent,
		]
	}

	#[test]
	fn colors_are_hex() {
		for variant in [Variant::Dark, Variant::Light] {
			for color in color_list(palette(variant)) {
				assert_eq!(color.len(), 7, "{color}");
				assert!(color.starts_with('#'));
				assert!(color[1..].chars().all(|digit| digit.is_ascii_hexdigit()));
			}
		}
	}

	#[test]
	fn every_color_is_distinct_across_variants_and_languages() {
		let mut all: Vec<String> = [Variant::Dark, Variant::Light]
			.into_iter()
			.flat_map(|variant| color_list(palette(variant)))
			.map(str::to_ascii_lowercase)
			.collect();
		let palette_count = all.len();
		all.extend(
			LANGUAGE_LIST
				.iter()
				.map(|language| language.color.to_ascii_lowercase()),
		);
		let mut unique = all.clone();
		unique.sort();
		unique.dedup();
		assert_eq!(unique.len(), all.len());
		assert_eq!(palette_count, 10);
	}

	#[test]
	fn font_stack_is_system_only() {
		assert!(FONT_STACK.ends_with("sans-serif"));
		assert!(!FONT_STACK.contains("url"));
	}
}
