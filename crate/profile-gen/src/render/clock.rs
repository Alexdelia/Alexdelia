use std::time::Duration;

use super::format::float;
use crate::timeline::Timeline;

const LEAD_MS: u32 = 1_600;
const MIN_TICK_MS: u32 = 40;
const TAIL_MS: u32 = 700;
const LABEL_STATE_MS: u32 = 400;
const DAYS_PER_YEAR: f64 = 365.2425;

pub(super) fn race_ms(timeline: &Timeline, seconds_per_year: f64) -> u32 {
	let days = timeline
		.frames
		.first()
		.zip(timeline.frames.last())
		.and_then(|(first, last)| first.week.until(last.week).ok())
		.map_or(0, |span| span.get_days());
	let race = Duration::try_from_secs_f64(seconds_per_year * f64::from(days) / DAYS_PER_YEAR)
		.unwrap_or(Duration::MAX);
	u32::try_from(race.as_millis()).unwrap_or(u32::MAX)
}

pub(super) struct Clock {
	pub(super) lead: usize,
	pub(super) tick_ms: u32,
	frames: usize,
	tail: usize,
}

impl Clock {
	pub(super) fn new(frames: usize, race_ms: u32) -> Self {
		let intervals = u32::try_from(frames.saturating_sub(1).max(1)).unwrap_or(u32::MAX);
		let tick_ms = (race_ms.saturating_add(intervals / 2) / intervals).max(MIN_TICK_MS);
		Self {
			lead: ticks_within(LEAD_MS, tick_ms),
			tick_ms,
			frames,
			tail: ticks_within(TAIL_MS, tick_ms),
		}
	}

	pub(super) fn ticks_in(&self, ms: u32) -> usize {
		ticks_within(ms, self.tick_ms)
	}

	pub(super) const fn ticks(&self) -> usize {
		self.lead + self.frames + self.tail
	}

	pub(super) const fn is_lead(&self, tick: usize) -> bool {
		tick < self.lead
	}

	pub(super) fn frame_of(&self, tick: usize) -> usize {
		tick.saturating_sub(self.lead)
			.min(self.frames.saturating_sub(1))
	}

	pub(super) fn ms(&self, tick: usize) -> f64 {
		float(tick) * f64::from(self.tick_ms)
	}

	pub(super) fn duration(&self) -> f64 {
		self.ms(self.ticks() - 1)
	}

	pub(super) fn state_ticks(&self) -> Vec<usize> {
		let last = self.ticks() - 1;
		(0..last)
			.step_by(self.ticks_in(LABEL_STATE_MS))
			.chain([last])
			.collect()
	}
}

fn ticks_within(ms: u32, tick_ms: u32) -> usize {
	usize::try_from(ms.div_ceil(tick_ms)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::MIN_SECONDS_PER_YEAR;
	use crate::timeline::Frame;
	use jiff::Span;
	use jiff::civil::Date;

	fn spanning(week_list: &[i64]) -> Timeline {
		let start = Date::constant(2020, 1, 6);
		Timeline {
			frames: week_list
				.iter()
				.map(|index| Frame {
					week: start.saturating_add(Span::new().weeks(*index)),
					entries: Vec::new(),
				})
				.collect(),
			..Timeline::default()
		}
	}

	#[test]
	fn race_lasts_seconds_per_year_times_history_span() {
		let six_years = spanning(&[0, 313]);
		assert_eq!(race_ms(&six_years, 5.0), 29_993);
		assert_eq!(race_ms(&six_years, 1.0), 5_998);
		assert_eq!(race_ms(&spanning(&[0, 104]), 5.0), 9_965);
	}

	#[test]
	fn race_follows_span_not_frame_count() {
		let sparse = spanning(&[0, 100, 200, 313]);
		let dense: Vec<i64> = (0..=313).collect();
		assert_eq!(race_ms(&sparse, 5.0), race_ms(&spanning(&dense), 5.0));
		assert_eq!(race_ms(&spanning(&[3]), 5.0), 0);
		assert_eq!(race_ms(&Timeline::default(), 5.0), 0);
		assert_eq!(race_ms(&sparse, f64::MAX), u32::MAX);
	}

	#[test]
	fn slowest_allowed_pace_keeps_weekly_ticks_above_the_floor() {
		let one_year = spanning(&[0, 52]);
		let race = race_ms(&one_year, MIN_SECONDS_PER_YEAR);
		assert!(race / 52 >= MIN_TICK_MS, "{race}");
	}

	#[test]
	fn weekly_ticks_keep_the_race_length() {
		let clock = Clock::new(314, 29_993);
		assert_eq!(clock.tick_ms, 96);
		assert!(clock.ms(clock.lead) >= f64::from(LEAD_MS));
		let race = clock.ms(clock.lead + 313) - clock.ms(clock.lead);
		assert!((race - 29_993.0).abs() <= 313.0 / 2.0, "{race}");
		let tail = clock.duration() - clock.ms(clock.lead + 313);
		assert!(tail >= f64::from(TAIL_MS) && tail < f64::from(TAIL_MS) + 96.0);
	}

	#[test]
	fn short_or_empty_races_keep_a_bounded_tick() {
		for (frames, race) in [(0, 0), (1, 0), (2, 0), (2, 96)] {
			let clock = Clock::new(frames, race);
			assert!(clock.tick_ms >= MIN_TICK_MS, "{frames}");
			assert!(clock.ticks() <= 200, "{frames}");
			assert!(clock.duration() <= 3_000.0, "{frames}");
			assert_eq!(clock.state_ticks().last(), Some(&(clock.ticks() - 1)));
		}
	}

	#[test]
	fn label_states_follow_time_not_frames() {
		let short = Clock::new(100, 10_000).state_ticks();
		let long = Clock::new(314, 29_993).state_ticks();
		assert!(
			long.len() > short.len() * 2,
			"{} {}",
			long.len(),
			short.len()
		);
		let clock = Clock::new(314, 29_993);
		let state_list = clock.state_ticks();
		assert_eq!(state_list.first(), Some(&0));
		assert_eq!(state_list.last(), Some(&(clock.ticks() - 1)));
		for pair in state_list.windows(2) {
			let gap = clock.ms(pair[1]) - clock.ms(pair[0]);
			assert!(
				gap > 0.0 && gap <= f64::from(LABEL_STATE_MS) + 96.0,
				"{gap}"
			);
		}
	}
}
