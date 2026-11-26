pub(super) struct Track<'a> {
	pub(super) x: &'a [f64],
	pub(super) shown: &'a [bool],
	pub(super) ranked: &'a [bool],
}

impl Track<'_> {
	fn near(&self, rival: &Self, tick: usize, reach: f64) -> bool {
		let gap = |at: usize| self.x[at] - rival.x[at];
		let from = gap(tick);
		let to = gap((tick + 1).min(self.x.len() - 1));
		let passing = from.signum() != to.signum();
		self.shown[tick] && rival.shown[tick] && (passing || from.abs().min(to.abs()) < reach)
	}

	fn yields_to(&self, rival: &Self, tick: usize, reach: f64) -> bool {
		let outranks = self.ranked[tick] && !rival.ranked[tick];
		!outranks && self.near(rival, tick, reach)
	}
}

pub(super) fn close_runs(
	track_list: &[Track],
	reach: f64,
	bridge: usize,
) -> Vec<Vec<(usize, usize)>> {
	track_list
		.iter()
		.enumerate()
		.map(|(lane, track)| {
			let mut run_list: Vec<(usize, usize)> = Vec::new();
			let close_ticks = (0..track.x.len()).filter(|tick| {
				track_list
					.iter()
					.enumerate()
					.any(|(other, rival)| other != lane && track.yields_to(rival, *tick, reach))
			});
			for tick in close_ticks {
				match run_list.last_mut() {
					Some(run) if tick <= run.1 + bridge => run.1 = tick,
					_ => run_list.push((tick, tick)),
				}
			}
			run_list
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	const REACH: f64 = 50.0;

	fn track<'a>(x: &'a [f64], shown: &'a [bool]) -> Track<'a> {
		Track {
			x,
			shown,
			ranked: shown,
		}
	}

	#[test]
	fn lanes_within_reach_cross_while_both_shown() {
		let left = [0.0, 0.0, 20.0, 40.0, 50.0, 50.0];
		let right = [50.0, 50.0, 30.0, 10.0, 0.0, 0.0];
		let far = [100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
		let shown = [true; 6];
		let run_list = close_runs(
			&[
				track(&left, &shown),
				track(&right, &shown),
				track(&far, &shown),
			],
			REACH,
			1,
		);
		assert_eq!(run_list, [vec![(1, 3)], vec![(1, 3)], vec![]]);
	}

	#[test]
	fn hidden_lanes_never_cross() {
		let parked = [50.0; 4];
		let last = [50.0; 4];
		let run_list = close_runs(
			&[
				track(&parked, &[false, false, true, true]),
				track(&last, &[true; 4]),
			],
			REACH,
			1,
		);
		assert_eq!(run_list, [vec![(2, 3)], vec![(2, 3)]]);
	}

	#[test]
	fn runs_within_the_bridge_join() {
		let mover = [50.0, 10.0, 50.0, 50.0, 10.0, 50.0, 50.0, 50.0, 50.0, 10.0];
		let still = [0.0; 10];
		let shown = [true; 10];
		let run_list = close_runs(&[track(&mover, &shown), track(&still, &shown)], REACH, 3);
		assert_eq!(run_list[0], [(0, 4), (8, 9)]);
	}

	#[test]
	fn ranked_lane_keeps_its_label_over_an_unranked_one() {
		let spot = [50.0; 4];
		let shown = [true; 4];
		let newcomer = Track {
			x: &spot,
			shown: &shown,
			ranked: &[false, false, true, true],
		};
		let run_list = close_runs(&[newcomer, track(&spot, &shown)], REACH, 1);
		assert_eq!(run_list, [vec![(0, 3)], vec![(2, 3)]]);
	}

	#[test]
	fn lanes_passing_within_one_tick_cross() {
		let jumper = [0.0, 100.0, 100.0];
		let still = [50.0; 3];
		let shown = [true; 3];
		let run_list = close_runs(&[track(&jumper, &shown), track(&still, &shown)], REACH, 1);
		assert_eq!(run_list[0], [(0, 0)]);
	}
}
