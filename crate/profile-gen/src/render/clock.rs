use super::format::float;

const LEAD_MS: u32 = 1_600;
const RACE_MS: u32 = 13_000;
const MAX_TICK_MS: u32 = 500;
const TAIL_MS: u32 = 700;
const LABEL_STATES: usize = 60;

pub(super) struct Clock {
	pub(super) lead: usize,
	pub(super) tick_ms: u32,
	frames: usize,
	tail: usize,
}

impl Clock {
	pub(super) fn new(frames: usize) -> Self {
		let intervals = u32::try_from(frames.saturating_sub(1).max(1)).unwrap_or(u32::MAX);
		let tick_ms = (RACE_MS / intervals).clamp(1, MAX_TICK_MS);
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
		let count = LABEL_STATES.min(last + 1);
		(0..count).map(|state| state * last / (count - 1)).collect()
	}
}

fn ticks_within(ms: u32, tick_ms: u32) -> usize {
	usize::try_from(ms.div_ceil(tick_ms)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn race_lasts_about_thirteen_seconds_after_lead() {
		let clock = Clock::new(150);
		assert_eq!(clock.tick_ms, 87);
		assert!(clock.ms(clock.lead) >= f64::from(LEAD_MS));
		assert!(clock.duration() < 17_000.0);
	}

	#[test]
	fn short_timelines_use_bounded_ticks() {
		for frames in [0, 1, 2] {
			let clock = Clock::new(frames);
			assert_eq!(clock.tick_ms, MAX_TICK_MS);
			assert!(clock.duration() <= 4_000.0, "{frames}");
			assert_eq!(clock.state_ticks().last(), Some(&(clock.ticks() - 1)));
		}
	}
}
