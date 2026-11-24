use crate::history::Week;
use crate::timeline::Direction;

pub(super) fn num(value: f64, digits: i32) -> String {
	let scale = 10f64.powi(digits);
	format!("{}", (value * scale).round() / scale + 0.0)
}

pub(super) fn float(value: impl TryInto<u32>) -> f64 {
	f64::from(value.try_into().unwrap_or(u32::MAX))
}

pub(super) fn escape(text: &str) -> String {
	text.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
		.replace('"', "&quot;")
		.replace('\'', "&#39;")
}

pub(super) fn percent(share_bp: u32) -> String {
	let tenths = (share_bp + 5) / 10;
	format!("{}.{}%", tenths / 10, tenths % 10)
}

pub(super) fn compact(lines: u64) -> String {
	match lines {
		0..1_000 => lines.to_string(),
		1_000..10_000 => format!("{}.{}k", lines / 1_000, lines / 100 % 10),
		10_000..1_000_000 => format!("{}k", lines / 1_000),
		1_000_000..10_000_000 => format!("{}.{}M", lines / 1_000_000, lines / 100_000 % 10),
		_ => format!("{}M", lines / 1_000_000),
	}
}

pub(super) const fn arrow(direction: Direction) -> &'static str {
	match direction {
		Direction::Up => "&#x2191;",
		Direction::Down => "&#x2193;",
		Direction::Flat => "&#x2192;",
	}
}

pub(super) const fn trend_word(direction: Direction) -> &'static str {
	match direction {
		Direction::Up => "up",
		Direction::Down => "down",
		Direction::Flat => "flat",
	}
}

pub(super) fn month(week: Week) -> String {
	format!("{}-{:02}", week.year(), week.month())
}

pub(super) fn animate(attribute: &str, values: &str, duration: f64) -> String {
	format!(
		"<animate attributeName=\"{attribute}\" values=\"{values}\" dur=\"{}ms\" fill=\"freeze\"/>",
		num(duration, 0)
	)
}

pub(super) fn animate_transform(kind: &str, values: &str, duration: f64) -> String {
	format!(
		"<animateTransform attributeName=\"transform\" type=\"{kind}\" values=\"{values}\" dur=\"{}ms\" fill=\"freeze\"/>",
		num(duration, 0)
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compact_line_counts() {
		let cases = [
			(0, "0"),
			(999, "999"),
			(1_000, "1.0k"),
			(9_999, "9.9k"),
			(10_000, "10k"),
			(999_999, "999k"),
			(1_000_000, "1.0M"),
			(9_999_999, "9.9M"),
			(12_345_678, "12M"),
		];
		for (lines, expected) in cases {
			assert_eq!(compact(lines), expected);
		}
	}

	#[test]
	fn numbers_are_short_and_never_negative_zero() {
		assert_eq!(num(-0.04, 1), "0");
		assert_eq!(num(12.0, 1), "12");
		assert_eq!(num(6.11372, 3), "6.114");
		assert_eq!(float(u64::MAX), f64::from(u32::MAX));
	}

	#[test]
	fn trend_words_name_each_direction() {
		let word_list = [Direction::Up, Direction::Down, Direction::Flat].map(trend_word);
		assert_eq!(word_list, ["up", "down", "flat"]);
	}

	#[test]
	fn escape_covers_xml_specials() {
		assert_eq!(escape("a&<>\"'b"), "a&amp;&lt;&gt;&quot;&#39;b");
	}
}
