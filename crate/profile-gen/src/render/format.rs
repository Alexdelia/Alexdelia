use std::time::Duration;

use crate::history::Week;
use crate::timeline::Direction;

pub(super) fn num(value: f64, digits: i32) -> String {
	let scale = 10f64.powi(digits);
	format!("{}", (value * scale).round() / scale + 0.0)
}

pub(super) fn float(value: impl TryInto<u32>) -> f64 {
	f64::from(value.try_into().unwrap_or(u32::MAX))
}

pub(super) fn key_time(ms: f64, duration: f64) -> String {
	let key = num(ms / duration, 5);
	key.strip_prefix('0')
		.filter(|fraction| fraction.starts_with('.'))
		.map_or_else(|| key.clone(), str::to_owned)
}

pub(super) fn whole_ms(ms: f64) -> i64 {
	Duration::try_from_secs_f64((ms + 0.5) / 1_000.0).map_or(0, |span| {
		i64::try_from(span.as_millis()).unwrap_or(i64::MAX)
	})
}

pub(super) fn discrete(
	head: &str,
	start: &str,
	point_list: Vec<(u32, String)>,
	duration: f64,
) -> String {
	let mut list: Vec<(u32, String)> = Vec::new();
	for (ms, value) in point_list {
		if list.last().is_some_and(|last| last.0 == ms) {
			list.pop();
		}
		list.push((ms, value));
	}
	let mut value_list = vec![start.to_owned()];
	let mut key_list = vec!["0".to_owned()];
	for (ms, value) in list {
		if value_list.last() != Some(&value) {
			value_list.push(value);
			key_list.push(key_time(f64::from(ms), duration));
		}
	}
	format!(
		"{head} calcMode=\"discrete\" values=\"{}\" keyTimes=\"{}\" dur=\"{}ms\" fill=\"freeze\"/>",
		value_list.join(";"),
		key_list.join(";"),
		num(duration, 0),
	)
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

pub(super) fn totals_label(language_count: u32, total_lines: u64) -> String {
	let languages = if language_count == 1 {
		"language"
	} else {
		"languages"
	};
	let lines = if total_lines == 1 { "line" } else { "lines" };
	format!(
		"{language_count} {languages} - {} {lines}",
		compact(total_lines)
	)
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
	fn key_times_drop_the_leading_zero() {
		assert_eq!(key_time(0.0, 1_000.0), "0");
		assert_eq!(key_time(1_000.0, 1_000.0), "1");
		assert_eq!(key_time(250.0, 1_000.0), ".25");
		assert_eq!(key_time(1.0, 3_000.0), ".00033");
	}

	#[test]
	fn whole_ms_rounds_and_floors_at_zero() {
		assert_eq!(whole_ms(339.4), 339);
		assert_eq!(whole_ms(339.5), 340);
		assert_eq!(whole_ms(-5.0), 0);
		assert_eq!(whole_ms(f64::NAN), 0);
	}

	#[test]
	fn trend_words_name_each_direction() {
		let word_list = [Direction::Up, Direction::Down, Direction::Flat].map(trend_word);
		assert_eq!(word_list, ["up", "down", "flat"]);
	}

	#[test]
	fn totals_read_as_languages_and_lines() {
		assert_eq!(totals_label(21, 412_345), "21 languages - 412k lines");
		assert_eq!(totals_label(1, 1), "1 language - 1 line");
		assert_eq!(totals_label(0, 0), "0 languages - 0 lines");
	}

	#[test]
	fn escape_covers_xml_specials() {
		assert_eq!(escape("a&<>\"'b"), "a&amp;&lt;&gt;&quot;&#39;b");
	}
}
