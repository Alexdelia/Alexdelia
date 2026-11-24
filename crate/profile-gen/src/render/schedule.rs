use std::iter::successors;

pub const FASTEST_MS: i64 = 30;
pub const CALM_MS: i64 = 300;
pub const EASE_MS: i64 = 600;
pub const AHEAD_MS: i64 = 1_000;
pub const HEAT_STEP_MS: i64 = 50;
pub const HEAT_MAX: i64 = 1_000;
pub const MAX_EARLY_MS: i64 = 1_000;

#[derive(Clone, Copy)]
pub struct Throw {
	pub lane: usize,
	pub deadline_ms: i64,
	pub flight_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Shot {
	pub lane: usize,
	pub enter_ms: i64,
	pub fire_ms: i64,
	pub land_ms: i64,
}

pub struct Schedule {
	pub shots: Vec<Shot>,
	pub heat: Vec<u16>,
}

impl Schedule {
	#[must_use]
	pub fn start_ms(&self) -> i64 {
		self.shots.first().map_or(0, |shot| shot.enter_ms)
	}
}

#[must_use]
pub fn schedule(throws: &[Throw], capacity: usize) -> Schedule {
	let mut queue = throws.to_vec();
	queue.sort_by_key(|throw| (throw.deadline_ms, throw.lane));
	let fire = fire_times(&queue);
	let enter = entry_times(&fire, capacity);
	let shots: Vec<Shot> = queue
		.iter()
		.zip(fire)
		.zip(enter)
		.map(|((throw, fire_ms), enter_ms)| Shot {
			lane: throw.lane,
			enter_ms,
			fire_ms,
			land_ms: fire_ms + throw.flight_ms,
		})
		.collect();
	let heat = heat_curve(&shots);
	Schedule { shots, heat }
}

fn fire_times(queue: &[Throw]) -> Vec<i64> {
	let mut fire: Vec<i64> = queue
		.iter()
		.map(|throw| throw.deadline_ms - throw.flight_ms)
		.collect();
	let floor = earliest_fire_times(&fire);
	slow_down_gently(&mut fire, &floor);
	while speed_up_gently(&mut fire, &floor) {}
	fire
}

fn earliest_fire_times(latest: &[i64]) -> Vec<i64> {
	let mut floor: Vec<i64> = latest.iter().map(|ms| ms - MAX_EARLY_MS).collect();
	for later in (1..floor.len()).rev() {
		floor[later - 1] = floor[later - 1].min(floor[later]);
	}
	floor
}

fn slow_down_gently(fire: &mut [i64], floor: &[i64]) {
	for later in (1..fire.len()).rev() {
		let next_gap = fire
			.get(later + 1)
			.map_or(FASTEST_MS, |next| next - fire[later]);
		let latest = fire[later] - shortest_gap_before(next_gap);
		fire[later - 1] = fire[later - 1].min(latest).max(floor[later - 1]);
	}
}

fn shortest_gap_before(next_gap: i64) -> i64 {
	ceil_div(next_gap.min(CALM_MS) * 3, 4).max(FASTEST_MS)
}

fn speed_up_gently(fire: &mut [i64], floor: &[i64]) -> bool {
	let mut moved = false;
	for middle in 1..fire.len().saturating_sub(1) {
		let (previous, next) = (fire[middle - 1], fire[middle + 1]);
		let previous_gap = fire[middle] - previous;
		let next_gap = next - fire[middle];
		let eased = (previous * 3 + next * 4).div_euclid(7).max(floor[middle]);
		if previous_gap < CALM_MS && next_gap * 4 < previous_gap * 3 && eased < fire[middle] {
			fire[middle] = eased;
			moved = true;
		}
	}
	moved
}

fn entry_times(fire: &[i64], capacity: usize) -> Vec<i64> {
	fire.iter()
		.enumerate()
		.map(|(index, fire_ms)| {
			let room_ms = index
				.checked_sub(capacity)
				.map_or(i64::MIN, |ahead| fire[ahead]);
			room_ms.max(fire_ms - AHEAD_MS)
		})
		.collect()
}

fn heat_curve(shots: &[Shot]) -> Vec<u16> {
	let Some(first) = shots.first() else {
		return Vec::new();
	};
	let fire: Vec<i64> = shots.iter().map(|shot| shot.fire_ms).collect();
	let last_landing = shots
		.iter()
		.map(|shot| shot.land_ms)
		.fold(first.enter_ms, i64::max);
	successors(Some(first.enter_ms), |time| {
		(*time < last_landing).then_some(time + HEAT_STEP_MS)
	})
	.scan(0, |heat, time| {
		*heat = approach(*heat, raw_heat(&fire, time));
		Some(*heat)
	})
	.map(|heat| u16::try_from(heat).unwrap_or_default())
	.collect()
}

fn raw_heat(fire: &[i64], time: i64) -> i64 {
	let fired = fire.partition_point(|fire_ms| *fire_ms <= time);
	let interval = fired
		.checked_sub(1)
		.and_then(|last| Some(fire.get(fired)? - fire[last]))
		.map_or(CALM_MS, |gap| gap.clamp(FASTEST_MS, CALM_MS));
	(CALM_MS - interval) * HEAT_MAX / (CALM_MS - FASTEST_MS)
}

const fn approach(heat: i64, target: i64) -> i64 {
	let pull = (target - heat) * HEAT_STEP_MS;
	heat + pull.signum() * ceil_div(pull.abs(), EASE_MS)
}

const fn ceil_div(numerator: i64, denominator: i64) -> i64 {
	-(-numerator).div_euclid(denominator)
}

#[cfg(test)]
mod tests {
	use super::*;

	const CAPACITY: usize = 11;

	fn throws(list: &[(usize, i64, i64)]) -> Vec<Throw> {
		list.iter()
			.map(|(lane, deadline_ms, flight_ms)| Throw {
				lane: *lane,
				deadline_ms: *deadline_ms,
				flight_ms: *flight_ms,
			})
			.collect()
	}

	fn clustered() -> Vec<Throw> {
		let mut list = Vec::new();
		let mut burst_start = 0;
		for burst in 0..20_i64 {
			burst_start += 150 + (burst * 977) % 2_400;
			for member in 0..10_i64 {
				let lane = usize::try_from((member * 3 + burst) % 6).unwrap();
				let deadline = burst_start + (member * (7 * burst + 13)) % 100;
				let flight = 300 + (member * burst * 17) % 300;
				list.push((lane, deadline, flight));
			}
		}
		throws(&list)
	}

	fn burst_between_calm(burst_size: usize, spacing_ms: i64) -> Vec<Throw> {
		let mut list = vec![(0, 0, 600)];
		list.extend(
			(0..burst_size)
				.zip(0_i64..)
				.map(|(member, step)| (member % 5, 10_000 + step * spacing_ms, 600)),
		);
		list.push((1, 30_000, 600));
		throws(&list)
	}

	fn alternating_dense_and_sparse() -> Vec<Throw> {
		let mut list = Vec::new();
		let mut cluster_start = 0;
		for cluster in 0..40_i64 {
			let (members, spacing) = if cluster % 2 == 0 {
				(10, 1)
			} else {
				(6, 100 + cluster * 3)
			};
			for member in 0..members {
				let lane = usize::try_from((member + cluster) % 16).unwrap();
				list.push((lane, cluster_start + member * spacing, 450));
			}
			cluster_start += members * spacing + 300 + (cluster * 131) % 400;
		}
		throws(&list)
	}

	fn very_dense() -> Vec<Throw> {
		throws(
			&(0..8_320_i64)
				.map(|index| (usize::try_from(index % 16).unwrap(), index * 6, 600))
				.collect::<Vec<_>>(),
		)
	}

	fn dense_middle_stretch() -> Vec<Throw> {
		let calm = |offset: i64| (0..10_i64).map(move |index| offset + index * 400);
		let dense = (0..400_i64).map(|index| 4_000 + index * 5);
		throws(
			&calm(0)
				.chain(dense)
				.chain(calm(6_400))
				.zip(0_usize..)
				.map(|(deadline, index)| (index % 16, deadline, 600))
				.collect::<Vec<_>>(),
		)
	}

	fn floor_ms(throw: &Throw) -> i64 {
		throw.deadline_ms - throw.flight_ms - MAX_EARLY_MS
	}

	fn in_deadline_order(list: &[Throw]) -> Vec<Throw> {
		let mut sorted = list.to_vec();
		sorted.sort_by_key(|throw| (throw.deadline_ms, throw.lane));
		sorted
	}

	fn fire_gaps(shots: &[Shot]) -> Vec<i64> {
		shots
			.windows(2)
			.map(|pair| pair[1].fire_ms - pair[0].fire_ms)
			.collect()
	}

	fn waiting_at(shots: &[Shot], time: i64) -> usize {
		shots.partition_point(|shot| shot.enter_ms <= time)
			- shots.partition_point(|shot| shot.fire_ms <= time)
	}

	fn tallest_tube(shots: &[Shot]) -> usize {
		shots
			.iter()
			.map(|shot| waiting_at(shots, shot.enter_ms))
			.max()
			.unwrap_or(0)
	}

	fn all_calm_fire_times(sorted: &[Throw]) -> Vec<i64> {
		let mut fire: Vec<i64> = sorted
			.iter()
			.map(|throw| throw.deadline_ms - throw.flight_ms)
			.collect();
		for later in (1..fire.len()).rev() {
			fire[later - 1] = fire[later - 1].min(fire[later] - CALM_MS);
		}
		fire
	}

	fn assert_lands_in_time(list: &[Throw], shots: &[Shot]) {
		assert_eq!(shots.len(), list.len());
		for (shot, throw) in shots.iter().zip(&in_deadline_order(list)) {
			assert_eq!(shot.land_ms, shot.fire_ms + throw.flight_ms);
			assert!(shot.land_ms <= throw.deadline_ms, "{shot:?}");
		}
	}

	fn assert_fastest_gap(shots: &[Shot]) {
		for gap in fire_gaps(shots) {
			assert!(gap >= FASTEST_MS, "{gap}");
		}
	}

	fn assert_deadline_order(list: &[Throw], shots: &[Shot]) {
		let fired: Vec<usize> = shots.iter().map(|shot| shot.lane).collect();
		let expected: Vec<usize> = in_deadline_order(list)
			.iter()
			.map(|throw| throw.lane)
			.collect();
		assert_eq!(fired, expected);
	}

	fn assert_entry_ahead_and_fifo(shots: &[Shot]) {
		for shot in shots {
			assert!(
				shot.fire_ms - AHEAD_MS <= shot.enter_ms && shot.enter_ms <= shot.fire_ms,
				"{shot:?}"
			);
		}
		for pair in shots.windows(2) {
			assert!(pair[0].enter_ms <= pair[1].enter_ms, "{pair:?}");
		}
	}

	fn assert_gaps_ease(shots: &[Shot]) {
		for pair in fire_gaps(shots).windows(2) {
			let (a, b) = (pair[0], pair[1]);
			if a < CALM_MS && b < CALM_MS {
				assert!(b * 4 >= a * 3 && a * 4 >= b * 3, "{a} {b}");
			}
		}
	}

	fn assert_valid(list: &[Throw], shots: &[Shot]) {
		assert_lands_in_time(list, shots);
		assert_fastest_gap(shots);
		assert_deadline_order(list, shots);
		assert_entry_ahead_and_fifo(shots);
		assert!(tallest_tube(shots) <= CAPACITY);
	}

	fn time_of_sample(schedule: &Schedule, index: usize) -> i64 {
		schedule.start_ms() + i64::try_from(index).unwrap() * HEAT_STEP_MS
	}

	#[test]
	fn every_shot_lands_by_its_deadline() {
		let list = clustered();
		assert_lands_in_time(&list, &schedule(&list, CAPACITY).shots);
	}

	#[test]
	fn shots_keep_the_fastest_gap() {
		assert_fastest_gap(&schedule(&clustered(), CAPACITY).shots);
	}

	#[test]
	fn shots_fire_in_deadline_order() {
		let list = clustered();
		assert_deadline_order(&list, &schedule(&list, CAPACITY).shots);
	}

	#[test]
	fn quiet_throws_fire_just_in_time() {
		let list = throws(&[(2, 4_000, 500), (0, 0, 600), (1, 2_000, 400)]);
		let landings: Vec<i64> = schedule(&list, CAPACITY)
			.shots
			.iter()
			.map(|shot| shot.land_ms)
			.collect();
		assert_eq!(landings, [0, 2_000, 4_000]);
	}

	#[test]
	fn gaps_ease_into_and_out_of_a_burst() {
		let shots = schedule(&burst_between_calm(12, 0), CAPACITY).shots;
		assert_gaps_ease(&shots);
		assert_eq!(fire_gaps(&shots).iter().min(), Some(&FASTEST_MS));
		assert_gaps_ease(&schedule(&clustered(), CAPACITY).shots);
	}

	#[test]
	fn tube_never_exceeds_capacity() {
		let filled = schedule(&very_dense(), CAPACITY).shots;
		assert_eq!(tallest_tube(&filled), CAPACITY);
		for list in [burst_between_calm(150, FASTEST_MS), clustered()] {
			assert!(tallest_tube(&schedule(&list, CAPACITY).shots) <= CAPACITY);
		}
	}

	#[test]
	fn entry_is_at_most_ahead_and_fifo() {
		for list in [burst_between_calm(150, FASTEST_MS), clustered()] {
			assert_entry_ahead_and_fifo(&schedule(&list, CAPACITY).shots);
		}
		let shots = schedule(&burst_between_calm(150, FASTEST_MS), CAPACITY).shots;
		let burst_first_fire_ms = shots[1].fire_ms;
		assert_eq!(waiting_at(&shots, burst_first_fire_ms - 1), CAPACITY);
	}

	#[test]
	fn tiny_tubes_hold_zero_or_one() {
		let list = burst_between_calm(150, FASTEST_MS);
		let straight_through = schedule(&list, 0).shots;
		assert!(
			straight_through
				.iter()
				.all(|shot| shot.enter_ms == shot.fire_ms)
		);
		assert_eq!(tallest_tube(&schedule(&list, 1).shots), 1);
	}

	#[test]
	fn alternating_dense_and_sparse_clusters_stay_valid() {
		let list = alternating_dense_and_sparse();
		let shots = schedule(&list, CAPACITY).shots;
		assert_valid(&list, &shots);
		assert_gaps_ease(&shots);
		for (shot, throw) in shots.iter().zip(&in_deadline_order(&list)) {
			assert!(shot.fire_ms > floor_ms(throw), "{shot:?}");
		}
	}

	#[test]
	fn easing_never_fires_before_an_all_calm_schedule() {
		for list in [
			burst_between_calm(12, 0),
			alternating_dense_and_sparse(),
			clustered(),
			very_dense(),
		] {
			let shots = schedule(&list, CAPACITY).shots;
			let floor = all_calm_fire_times(&in_deadline_order(&list));
			for (shot, floor_ms) in shots.iter().zip(floor) {
				assert!(shot.fire_ms >= floor_ms, "{shot:?} {floor_ms}");
			}
		}
	}

	#[test]
	fn very_dense_input_stays_valid() {
		let list = very_dense();
		let schedule = schedule(&list, CAPACITY);
		let shots = &schedule.shots;
		assert_lands_in_time(&list, shots);
		for (shot, throw) in shots.iter().zip(&in_deadline_order(&list)) {
			assert!(shot.land_ms >= throw.deadline_ms - MAX_EARLY_MS, "{shot:?}");
		}
		assert!(fire_gaps(shots).iter().all(|gap| *gap >= 0));
		assert_deadline_order(&list, shots);
		assert!(tallest_tube(shots) <= CAPACITY);
		assert!(
			schedule
				.heat
				.iter()
				.all(|heat| i64::from(*heat) <= HEAT_MAX)
		);
		assert!(schedule.start_ms() >= floor_ms(&list[0]) - AHEAD_MS);
		assert!(fire_gaps(shots).iter().any(|gap| *gap < FASTEST_MS));
	}

	#[test]
	fn gaps_stay_at_30_unless_the_floor_binds() {
		let list = dense_middle_stretch();
		let shots = schedule(&list, CAPACITY).shots;
		let sorted = in_deadline_order(&list);
		let mut compressed = 0;
		for (index, gap) in fire_gaps(&shots).iter().enumerate() {
			if *gap < FASTEST_MS {
				assert_eq!(shots[index].fire_ms, floor_ms(&sorted[index]), "{index}");
				compressed += 1;
			}
		}
		assert!(compressed > 0);
		assert_lands_in_time(&list, &shots);
	}

	#[test]
	fn heat_follows_the_fire_rate() {
		let burst_size = 150;
		let schedule = schedule(&burst_between_calm(burst_size, FASTEST_MS), CAPACITY);
		let burst = &schedule.shots[1..=burst_size];
		let hottest = (0..schedule.heat.len())
			.max_by_key(|index| schedule.heat[*index])
			.unwrap();
		assert_eq!(schedule.heat[hottest], 1_000);
		let hottest_ms = time_of_sample(&schedule, hottest);
		assert!(burst[0].fire_ms <= hottest_ms && hottest_ms <= burst[burst_size - 1].fire_ms);
		assert!(schedule.heat.iter().all(|heat| *heat <= 1_000));
		let calm = (0..schedule.heat.len())
			.find(|index| time_of_sample(&schedule, *index) >= 29_000)
			.unwrap();
		assert_eq!(schedule.heat[calm], 0);
		assert_eq!(schedule.heat[0], 0);
		let last_landing = schedule.shots[burst_size + 1].land_ms;
		let last = schedule.heat.len() - 1;
		assert!(time_of_sample(&schedule, last) >= last_landing);
		assert!(time_of_sample(&schedule, last - 1) < last_landing);
	}

	#[test]
	fn no_throws_gives_empty_schedule() {
		let schedule = schedule(&[], CAPACITY);
		assert!(schedule.shots.is_empty());
		assert_eq!(schedule.heat, Vec::<u16>::new());
		assert_eq!(schedule.start_ms(), 0);
	}
}
