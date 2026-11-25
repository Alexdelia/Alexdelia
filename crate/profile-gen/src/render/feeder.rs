use std::collections::BTreeSet;

use super::format::{discrete, escape, key_time, num, whole_ms};
use super::languages::{GROUND, TOKEN_RADIUS, WIDTH};
use crate::theme::Palette;

pub(super) const SLINGSHOT: &str = "#e84e31";
pub(super) const FLIGHT_RADIUS: f64 = TOKEN_RADIUS / 2.0;
const FLIGHT_SCALE: f64 = FLIGHT_RADIUS / TOKEN_RADIUS;
const MAX_CAPACITY: u32 = 64;

pub(super) const SLING_X: f64 = 36.0;
const PRONG: f64 = 13.0;
const RING: f64 = 4.5;
const STROKE: f64 = 4.5;
const PRONG_Y: f64 = 200.0;
const FOOT_Y: f64 = 294.0;
const JOIN_Y: f64 = PRONG_Y + 44.0;
const LEFT_PRONG_X: f64 = SLING_X - PRONG;
const RIGHT_PRONG_X: f64 = SLING_X + PRONG;

pub(super) const POUCH_Y: f64 = PRONG_Y + 6.0;
const PULL_X: f64 = -14.0;
const PULL_Y: f64 = 10.0;
const PULL_MS: u32 = 200;
const QUICK_PULL_MS: u32 = 40;
const BAND_WIDTH: f64 = 1.5;

const TUBE_TOP: f64 = 16.0;
const TUBE_BOTTOM: f64 = PRONG_Y - 6.0;
const TUBE_RADIUS: f64 = FLIGHT_RADIUS;
const TUBE_STEP: f64 = 2.0 * TUBE_RADIUS;
const TUBE_WALL: f64 = TUBE_RADIUS + 3.0;
const TUBE_REST_Y: f64 = TUBE_BOTTOM - TUBE_RADIUS;
const WALL_OVERHANG: f64 = 8.0;
const WINDOW_HEIGHT: f64 = 400.0;

const GRAVITY: f64 = 0.0024;
const APEX_MARGIN: f64 = 10.0;
const APEX_PER_PX: f64 = 0.03;
const APEX_PER_LIFT: f64 = 0.55;
const RESTING_Y: f64 = GROUND - FLIGHT_RADIUS;
const HIDDEN_Y: f64 = GROUND + FLIGHT_RADIUS;
const FREE_FALL_SPLINE: &str = ".333 0 .667 .333";
const FIRE_KEY_STEPS: f64 = 10_000.0;
const VANISH_KEY_STEPS: f64 = 1_000.0;
const THROW_SLACK_MS: f64 = 0.5;
const THROW_SEARCH: u32 = 64;

pub(super) struct Flight {
	pub(super) lane: usize,
	pub(super) name: &'static str,
	pub(super) frame: usize,
	pub(super) enter_ms: u32,
	pub(super) fire_ms: u32,
	pub(super) land_ms: u32,
	pub(super) to_x: f64,
	pub(super) landing_y: f64,
}

pub(super) fn flight_ms(dx: f64, landing_y: f64) -> i64 {
	let apex_y = apex_y(dx, landing_y);
	whole_ms(fall_ms(POUCH_Y - apex_y) + fall_ms(landing_y - apex_y))
}

fn apex_y(dx: f64, landing_y: f64) -> f64 {
	let lift = APEX_PER_LIFT * (RESTING_Y - landing_y.max(POUCH_Y)).max(0.0);
	POUCH_Y.min(landing_y) - APEX_PER_PX.mul_add(dx.abs(), APEX_MARGIN) - lift
}

fn fall_ms(height: f64) -> f64 {
	(2.0 * height.max(0.0) / GRAVITY).sqrt()
}

pub(super) fn tube_capacity() -> usize {
	(1..=MAX_CAPACITY)
		.take_while(|count| TUBE_STEP.mul_add(f64::from(*count), TUBE_TOP) <= TUBE_BOTTOM)
		.count()
}

pub(super) fn feeder(flight_list: &[Flight], duration: f64, colors: Palette) -> String {
	[
		defs(flight_list, duration),
		tube(flight_list, duration, colors),
		slingshot(),
		band(flight_list, duration, colors),
	]
	.concat()
}

fn defs(flight_list: &[Flight], duration: f64) -> String {
	let lane_set: BTreeSet<usize> = flight_list.iter().map(|flight| flight.lane).collect();
	let marker_list: Vec<String> = lane_set
		.iter()
		.map(|lane| {
			format!(
				"<g id=\"h{lane}\"><use href=\"#i{lane}\" transform=\"scale({})\"/></g><marker id=\"m{lane}\" markerUnits=\"userSpaceOnUse\" overflow=\"visible\"><use href=\"#h{lane}\"/></marker>",
				num(FLIGHT_SCALE, 2)
			)
		})
		.collect();
	format!(
		concat!(
			"<defs>{markers}",
			"<clipPath id=\"tb\"><rect x=\"{tube_x}\" y=\"{tube_y}\" width=\"{tube_w}\" height=\"{tube_h}\"/></clipPath>",
			"<clipPath id=\"tw\"><rect y=\"{bottom}\" width=\"{w}\" height=\"{window_h}\">{window}</rect></clipPath>",
			"<clipPath id=\"kf\"><rect width=\"{w}\" height=\"{g}\"/></clipPath>",
			"</defs>",
		),
		markers = marker_list.concat(),
		tube_x = num(SLING_X - TUBE_WALL, 1),
		tube_y = num(TUBE_TOP, 1),
		tube_w = num(2.0 * TUBE_WALL, 1),
		tube_h = num(TUBE_BOTTOM - TUBE_TOP, 1),
		bottom = num(TUBE_BOTTOM, 1),
		w = num(WIDTH, 1),
		window_h = num(WINDOW_HEIGHT, 1),
		window = window(flight_list, duration),
		g = num(GROUND, 1),
	)
}

fn window(flight_list: &[Flight], duration: f64) -> String {
	let mut event_list: Vec<(u32, i32)> = flight_list
		.iter()
		.flat_map(|flight| [(flight.enter_ms, 1), (flight.fire_ms, -1)])
		.collect();
	event_list.sort_unstable();
	let mut waiting = 0;
	let point_list: Vec<(u32, String)> = event_list
		.iter()
		.map(|(ms, change)| {
			waiting += change;
			(
				*ms,
				num(TUBE_STEP.mul_add(-f64::from(waiting), TUBE_BOTTOM), 1),
			)
		})
		.collect();
	discrete(
		"<animate attributeName=\"y\"",
		&num(TUBE_BOTTOM, 1),
		point_list,
		duration,
	)
}

fn tube_offset(fired: f64) -> String {
	format!(
		"{} {}",
		num(SLING_X, 1),
		num(TUBE_STEP.mul_add(fired, TUBE_REST_Y), 1)
	)
}

fn lane_tape(lane: usize, place_list: &[u32]) -> String {
	let mut previous = 0.0;
	let step_list: Vec<String> = place_list
		.iter()
		.map(|place| {
			let y = -TUBE_STEP * f64::from(*place);
			let step = num(y - previous, 1);
			previous = y;
			step
		})
		.collect();
	format!(
		"<path d=\"M0 {}h0\" marker-start=\"url(#m{lane})\" marker-mid=\"url(#m{lane})\"/>",
		step_list.join("v"),
	)
}

fn tube(flight_list: &[Flight], duration: f64, colors: Palette) -> String {
	let wall = |x: f64| {
		format!(
			"M{} {}V{}",
			num(x, 1),
			num(TUBE_TOP - WALL_OVERHANG, 1),
			num(TUBE_BOTTOM + 2.0, 1)
		)
	};
	let lane_set: BTreeSet<usize> = flight_list.iter().map(|flight| flight.lane).collect();
	let tape: Vec<String> = lane_set
		.iter()
		.map(|lane| {
			let place_list: Vec<u32> = flight_list
				.iter()
				.zip(0_u32..)
				.filter(|(flight, _)| flight.lane == *lane)
				.map(|(_, place)| place)
				.collect();
			lane_tape(*lane, &place_list)
		})
		.collect();
	let mut fired = 0.0;
	let step_list: Vec<(u32, String)> = flight_list
		.iter()
		.map(|flight| {
			fired += 1.0;
			(flight.fire_ms, tube_offset(fired))
		})
		.collect();
	format!(
		concat!(
			"<g data-tube=\"\">",
			"<path d=\"{left}{right}\" fill=\"none\" stroke=\"{ground}\" stroke-width=\"1.5\"/>",
			"<g clip-path=\"url(#tb)\"><g clip-path=\"url(#tw)\">",
			"<g transform=\"translate({end})\">{step}{tape}</g>",
			"</g></g></g>",
		),
		left = wall(SLING_X - TUBE_WALL),
		right = wall(SLING_X + TUBE_WALL),
		ground = colors.ground,
		end = tube_offset(fired),
		step = discrete(
			"<animateTransform attributeName=\"transform\" type=\"translate\"",
			&tube_offset(0.0),
			step_list,
			duration
		),
		tape = tape.concat(),
	)
}

fn slingshot() -> String {
	let ring = |x: f64, y: f64| {
		format!(
			"<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
			num(x, 1),
			num(y, 1),
			num(RING, 1)
		)
	};
	format!(
		concat!(
			"<g data-slingshot=\"fork\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{stroke}\" stroke-linecap=\"round\">",
			"<path d=\"M{right} {below}V{above_foot}M{left} {below}Q{left} {join} {right} {join}\"/>",
			"{left_ring}{right_ring}{foot_ring}",
			"</g>",
		),
		color = SLINGSHOT,
		stroke = num(STROKE, 1),
		right = num(RIGHT_PRONG_X, 1),
		left = num(LEFT_PRONG_X, 1),
		below = num(PRONG_Y + RING, 1),
		above_foot = num(FOOT_Y - RING, 1),
		join = num(JOIN_Y, 1),
		left_ring = ring(LEFT_PRONG_X, PRONG_Y),
		right_ring = ring(RIGHT_PRONG_X, PRONG_Y),
		foot_ring = ring(RIGHT_PRONG_X, FOOT_Y),
	)
}

fn band_path(pull: f64) -> String {
	format!(
		"M{} {}L{} {} {} {}",
		num(LEFT_PRONG_X, 1),
		num(PRONG_Y, 1),
		num(PULL_X.mul_add(pull, SLING_X), 1),
		num(PULL_Y.mul_add(pull, POUCH_Y), 1),
		num(RIGHT_PRONG_X, 1),
		num(PRONG_Y, 1),
	)
}

fn band(flight_list: &[Flight], duration: f64, colors: Palette) -> String {
	let rest = band_path(0.0);
	let mut fire_list: Vec<u32> = flight_list.iter().map(|flight| flight.fire_ms).collect();
	fire_list.dedup();
	let mut previous = 0;
	let mut point_list: Vec<(f64, String)> = vec![(0.0, rest.clone())];
	for fire in fire_list {
		let span = fire.saturating_sub(previous).min(PULL_MS);
		previous = fire;
		if span < QUICK_PULL_MS {
			continue;
		}
		let pulled = band_path(f64::from(span) / f64::from(PULL_MS));
		point_list.extend([
			(f64::from(fire - span), rest.clone()),
			(f64::from(fire), pulled),
			(f64::from(fire), rest.clone()),
		]);
	}
	point_list.push((duration, rest.clone()));
	let value_list: Vec<&str> = point_list.iter().map(|(_, path)| path.as_str()).collect();
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(ms, _)| key_time(*ms, duration))
		.collect();
	format!(
		concat!(
			"<path data-band=\"\" d=\"{rest}\" fill=\"none\" stroke=\"{muted}\" stroke-width=\"{width}\" stroke-linejoin=\"round\">",
			"<animate attributeName=\"d\" values=\"{values}\" keyTimes=\"{keys}\" dur=\"{dur}ms\" fill=\"freeze\"/>",
			"</path>",
		),
		rest = rest,
		muted = colors.muted,
		width = num(BAND_WIDTH, 1),
		values = value_list.join(";"),
		keys = key_list.join(";"),
		dur = num(duration, 0),
	)
}

struct Launch {
	fire_ms: u32,
	vanish_ms: u32,
	fire_key: f64,
	vanish_key: f64,
	thrust: (f64, f64),
	hidden_ms: u32,
}

fn whole(ms: f64) -> u32 {
	u32::try_from(whole_ms(ms)).unwrap_or(u32::MAX)
}

fn sink_ms(climb: f64, depth: f64) -> f64 {
	(GRAVITY.mul_add(2.0 * depth, climb * climb).sqrt() - climb) / GRAVITY
}

fn climb(flight: &Flight) -> f64 {
	let flown = f64::from(flight.land_ms - flight.fire_ms);
	(GRAVITY * flown / 2.0).mul_add(-flown, flight.landing_y - POUCH_Y) / flown
}

fn sunk_ms(flight: &Flight) -> f64 {
	f64::from(flight.fire_ms) + sink_ms(climb(flight), flight.landing_y + FLIGHT_RADIUS - POUCH_Y)
}

pub(super) fn throw_ms(fire_ms: u32, duration: f64) -> u32 {
	let step = duration / FIRE_KEY_STEPS;
	let latest = (f64::from(fire_ms) / step).floor();
	let index = (0..THROW_SEARCH)
		.map(|back| latest - f64::from(back))
		.take_while(|index| *index >= 0.0)
		.find(|index| {
			let ms = index * step;
			ms.ceil() - ms <= THROW_SLACK_MS
		})
		.unwrap_or(latest);
	whole((index * step).ceil()).min(fire_ms)
}

pub(super) fn absorbed_ms(flight: &Flight) -> f64 {
	f64::midpoint(f64::from(flight.land_ms), sunk_ms(flight))
}

fn launch(flight: &Flight, duration: f64) -> Launch {
	let fire = f64::from(flight.fire_ms);
	let land = f64::from(flight.land_ms);
	let flown = land - fire;
	let sunk = sunk_ms(flight);
	let reach = (
		(flight.to_x - SLING_X) * flown / (absorbed_ms(flight) - fire),
		climb(flight) * flown,
	);
	let vanish_ms = whole(sunk.ceil());
	let fire_key =
		((fire - THROW_SLACK_MS / 2.0) / duration * FIRE_KEY_STEPS).round() / FIRE_KEY_STEPS;
	let vanish_key = (f64::from(vanish_ms) / duration * VANISH_KEY_STEPS).ceil() / VANISH_KEY_STEPS;
	let from = fire_key * duration;
	let stretch = vanish_key.mul_add(duration, -from) / (land - from);
	let thrust = ((reach.0 * stretch).round(), (reach.1 * stretch).round());
	Launch {
		fire_ms: flight.fire_ms,
		vanish_ms,
		fire_key,
		vanish_key,
		thrust,
		hidden_ms: whole(fall_ms(HIDDEN_Y + 1.0 - POUCH_Y - thrust.1).ceil()),
	}
}

struct Pool {
	lane: usize,
	name: &'static str,
	launch_list: Vec<Launch>,
	drop_ms: u32,
}

impl Pool {
	fn takes(&self, lane: usize, launch: &Launch, hide_ms: u32) -> bool {
		self.lane == lane
			&& self.launch_list.last().is_some_and(|last| {
				last.vanish_key < launch.fire_key && last.vanish_ms + hide_ms < launch.fire_ms
			})
	}

	fn push(&mut self, launch: Launch) {
		self.drop_ms = self.drop_ms.max(launch.hidden_ms);
		self.launch_list.push(launch);
	}
}

fn hide_ms(launch_list: &[Launch]) -> u32 {
	launch_list
		.iter()
		.map(|launch| (launch.fire_ms + launch.hidden_ms).saturating_sub(launch.vanish_ms))
		.max()
		.unwrap_or(0)
		+ 1
}

fn pool_list(flight_list: &[Flight], duration: f64, hide_ms: u32) -> Vec<Pool> {
	let mut pool_list: Vec<Pool> = Vec::new();
	for flight in flight_list {
		let launch = launch(flight, duration);
		match pool_list
			.iter_mut()
			.find(|pool| pool.takes(flight.lane, &launch, hide_ms))
		{
			Some(pool) => pool.push(launch),
			None => pool_list.push(Pool {
				lane: flight.lane,
				name: flight.name,
				drop_ms: launch.hidden_ms,
				launch_list: vec![launch],
			}),
		}
	}
	pool_list
}

fn thrust_text(thrust: (f64, f64)) -> String {
	format!("{} {}", num(thrust.0, 0), num(thrust.1, 0))
}

fn seconds(ms: u32) -> String {
	num(f64::from(ms) / 1_000.0, 3)
}

fn thrust(pool: &Pool, duration: f64) -> String {
	let thrust_list: Vec<String> = pool
		.launch_list
		.iter()
		.map(|launch| thrust_text(launch.thrust))
		.collect();
	let mut point_list: Vec<(f64, &str)> = vec![(0.0, "0")];
	for (launch, thrust) in pool.launch_list.iter().zip(&thrust_list) {
		point_list.extend([(launch.fire_key, "0"), (launch.vanish_key, thrust.as_str())]);
	}
	let last = thrust_list.last().map_or("0", String::as_str);
	point_list.push((1.0, last));
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(key, _)| key_time(key * duration, duration))
		.collect();
	let value_list: Vec<&str> = point_list.iter().map(|(_, value)| *value).collect();
	format!(
		concat!(
			"<g transform=\"translate({last})\">",
			"<animateTransform attributeName=\"transform\" type=\"translate\" values=\"{values}\" keyTimes=\"{keys}\" dur=\"{dur}ms\" fill=\"freeze\"/>",
		),
		last = last,
		values = value_list.join(";"),
		keys = key_list.join(";"),
		dur = num(duration, 0),
	)
}

fn circle(pool: &Pool, hide_ms: u32) -> String {
	let drop_ms = f64::from(pool.drop_ms);
	let deep = (GRAVITY * drop_ms / 2.0).mul_add(drop_ms, POUCH_Y);
	let begins = |at: fn(&Launch) -> u32| {
		let list: Vec<String> = pool
			.launch_list
			.iter()
			.map(|launch| seconds(at(launch)))
			.collect();
		list.join(";")
	};
	format!(
		concat!(
			"<use href=\"#h{lane}\" x=\"{x}\" y=\"{deep}\" data-flight=\"{name}\">",
			"<animateMotion calcMode=\"spline\" keySplines=\"{spline}\" values=\"0 {lift};0 0\" dur=\"{drop}ms\" begin=\"{fires}\" fill=\"freeze\"/>",
			"<animate attributeName=\"opacity\" calcMode=\"discrete\" values=\"0;1\" dur=\"{hide}ms\" begin=\"{vanishes}\" fill=\"freeze\"/>",
			"</use>",
		),
		lane = pool.lane,
		x = num(SLING_X, 1),
		deep = num(deep, 1),
		name = escape(pool.name),
		spline = FREE_FALL_SPLINE,
		lift = num(POUCH_Y - deep, 1),
		drop = pool.drop_ms,
		fires = begins(|launch| launch.fire_ms),
		hide = 2 * hide_ms,
		vanishes = begins(|launch| launch.vanish_ms),
	)
}

pub(super) fn flights(flight_list: &[Flight], duration: f64) -> String {
	let launch_list: Vec<Launch> = flight_list
		.iter()
		.map(|flight| launch(flight, duration))
		.collect();
	let hide_ms = hide_ms(&launch_list);
	let pool_text: Vec<String> = pool_list(flight_list, duration, hide_ms)
		.iter()
		.map(|pool| format!("{}{}</g>", thrust(pool, duration), circle(pool, hide_ms)))
		.collect();
	format!("<g clip-path=\"url(#kf)\">{}</g>", pool_text.concat())
}

#[cfg(test)]
mod tests {
	use super::super::probe::Pool as Probe;
	use super::*;
	use crate::theme::{Variant, palette};
	use roxmltree::{Document, Node};

	const DURATION: f64 = 10_000.0;

	fn flight(lane: usize, enter_ms: u32, fire_ms: u32) -> Flight {
		aimed(lane, enter_ms, fire_ms, 250.0)
	}

	fn aimed(lane: usize, enter_ms: u32, fire_ms: u32, landing_y: f64) -> Flight {
		Flight {
			lane,
			name: "Rust",
			frame: 0,
			enter_ms,
			fire_ms,
			land_ms: fire_ms + 500,
			to_x: 300.0,
			landing_y,
		}
	}

	fn wrapped(svg: &str) -> String {
		format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{svg}</svg>")
	}

	fn value_at(animation: Node, ms: f64) -> String {
		let duration: f64 = animation
			.attribute("dur")
			.unwrap()
			.trim_end_matches("ms")
			.parse()
			.unwrap();
		let key_list: Vec<f64> = animation
			.attribute("keyTimes")
			.unwrap()
			.split(';')
			.map(|key| key.parse::<f64>().unwrap() * duration)
			.collect();
		let value_list: Vec<&str> = animation.attribute("values").unwrap().split(';').collect();
		let index = key_list.iter().rposition(|key| *key <= ms + 0.01).unwrap();
		value_list[index].to_owned()
	}

	fn waiting_at(flight_list: &[Flight], ms: u32) -> usize {
		flight_list
			.iter()
			.filter(|flight| flight.enter_ms <= ms && ms < flight.fire_ms)
			.count()
	}

	fn measured(dx: f64, landing_y: f64) -> (f64, f64) {
		let fire_ms = 100;
		let flight = Flight {
			land_ms: fire_ms + u32::try_from(flight_ms(dx, landing_y)).unwrap(),
			to_x: SLING_X + dx,
			..aimed(0, 0, fire_ms, landing_y)
		};
		let svg = wrapped(&flights(&[flight], DURATION));
		let document = Document::parse(&svg).unwrap();
		let node = document
			.descendants()
			.find(|node| node.has_attribute("data-flight"))
			.unwrap();
		let probe = Probe::of(node);
		let begin = probe.begin_list[0];
		let landing = probe.reaching(begin, landing_y);
		let absorbed = probe.at(f64::midpoint(landing, probe.vanish(begin)), begin);
		assert!((absorbed.0 - SLING_X - dx).abs() <= 1.5, "{absorbed:?}");
		assert!(
			(landing - begin - f64::from(u32::try_from(flight_ms(dx, landing_y)).unwrap())).abs()
				<= 1.5
		);
		let apex = probe
			.samples(begin, 400)
			.iter()
			.map(|(_, point)| point.1)
			.fold(f64::INFINITY, f64::min);
		(apex, landing - begin)
	}

	#[test]
	fn tube_capacity_is_what_fits() {
		let places = (TUBE_BOTTOM - TUBE_TOP) / TUBE_STEP;
		let capacity = f64::from(u32::try_from(tube_capacity()).unwrap());
		assert!(capacity <= places && capacity + 1.0 > places, "{capacity}");
		assert_eq!(tube_capacity(), 17);
		assert!(FLIGHT_SCALE.mul_add(-TOKEN_RADIUS, TUBE_RADIUS).abs() < 1e-9);
	}

	#[test]
	fn farther_and_higher_targets_fly_higher_and_longer() {
		let (near_apex, near_ms) = measured(56.0, RESTING_Y);
		let (far_apex, far_ms) = measured(700.0, RESTING_Y);
		assert!(far_apex < near_apex - 10.0, "{near_apex} {far_apex}");
		assert!(far_ms > near_ms + 30.0, "{near_ms} {far_ms}");
		assert!(near_ms < 400.0, "{near_ms}");
		let mut previous = (f64::INFINITY, 0.0);
		for landing_y in [RESTING_Y, 260.0, 230.0, POUCH_Y, 180.0, 140.0, 94.0] {
			let (apex, ms) = measured(300.0, landing_y);
			assert!(
				apex < previous.0 && ms > previous.1,
				"{landing_y} {apex} {ms}"
			);
			assert!(
				apex < landing_y.min(POUCH_Y) - APEX_MARGIN,
				"{landing_y} {apex}"
			);
			previous = (apex, ms);
		}
		let (highest, longest) = measured(812.0, 94.0);
		assert!(highest >= FLIGHT_RADIUS, "{highest}");
		assert!(longest < 700.0, "{longest}");
	}

	#[test]
	fn tube_shows_each_circle_from_entry_to_fire() {
		let flight_list = [
			flight(0, 0, 400),
			flight(1, 100, 400),
			flight(0, 300, 900),
			flight(2, 900, 900),
			flight(1, 950, 1_200),
		];
		let svg = wrapped(&feeder(&flight_list, DURATION, palette(Variant::Dark)));
		let document = Document::parse(&svg).unwrap();
		let window = document
			.descendants()
			.find(|node| node.attribute("attributeName") == Some("y"))
			.unwrap();
		let tube = document
			.descendants()
			.find(|node| node.attribute("data-tube").is_some())
			.unwrap();
		let step = tube
			.descendants()
			.find(|node| node.attribute("calcMode") == Some("discrete"))
			.unwrap();
		for ms in [
			0, 50, 100, 350, 400, 450, 899, 900, 949, 950, 1_199, 1_200, 2_000,
		] {
			let edge: f64 = value_at(window, f64::from(ms)).parse().unwrap();
			let shown = (TUBE_BOTTOM - edge) / TUBE_STEP;
			assert!(
				(shown - f64::from(u32::try_from(waiting_at(&flight_list, ms)).unwrap())).abs()
					< 1e-9,
				"{ms}"
			);
			let fired = flight_list
				.iter()
				.filter(|flight| flight.fire_ms <= ms)
				.count();
			let offset = value_at(step, f64::from(ms));
			assert_eq!(
				offset,
				tube_offset(f64::from(u32::try_from(fired).unwrap())),
				"{ms}"
			);
		}
		let tape_list: Vec<Node> = tube
			.descendants()
			.filter(|node| node.has_attribute("marker-mid"))
			.collect();
		assert_eq!(tape_list.len(), 3);
		assert_eq!(tape_list[0].attribute("d"), Some("M0 0v-20h0"));
		assert_eq!(tape_list[1].attribute("d"), Some("M0 -10v-30h0"));
		assert_eq!(tape_list[2].attribute("d"), Some("M0 -30h0"));
		assert_eq!(tape_list[2].attribute("marker-start"), Some("url(#m2)"));
	}

	#[test]
	fn pools_reuse_circles_once_hidden() {
		let flight_list = [
			flight(0, 0, 0),
			flight(0, 0, 100),
			flight(0, 0, 2_000),
			flight(0, 0, 2_100),
			flight(1, 0, 2_000),
			aimed(2, 0, 3_000, 94.0),
			flight(2, 0, 3_600),
			flight(2, 0, 3_700),
			flight(2, 0, 4_300),
		];
		let launch_list: Vec<Launch> = flight_list
			.iter()
			.map(|flight| launch(flight, DURATION))
			.collect();
		let hide_ms = hide_ms(&launch_list);
		let fire_list: Vec<(usize, Vec<u32>)> = pool_list(&flight_list, DURATION, hide_ms)
			.iter()
			.map(|pool| {
				let fire_list = pool.launch_list.iter().map(|launch| launch.fire_ms);
				(pool.lane, fire_list.collect())
			})
			.collect();
		assert_eq!(
			fire_list,
			[
				(0, vec![0, 2_000]),
				(0, vec![100, 2_100]),
				(1, vec![2_000]),
				(2, vec![3_000, 3_700]),
				(2, vec![3_600, 4_300]),
			]
		);
		let svg = wrapped(&flights(&flight_list, DURATION));
		let document = Document::parse(&svg).unwrap();
		let node_list: Vec<Node> = document
			.descendants()
			.filter(|node| node.has_attribute("data-flight"))
			.collect();
		assert_eq!(node_list.len(), 5);
		assert!(Probe::of(node_list[3]).drop_ms() > 650.0);
		let (first, second) = (&launch_list[0], &launch_list[2]);
		let thrust = thrust_text(first.thrust);
		let outer = node_list[0].parent().unwrap();
		let track = outer.first_element_child().unwrap();
		assert_eq!(
			track.attribute("values"),
			Some(format!("0;0;{thrust};0;{thrust};{thrust}").as_str())
		);
		let key = |key: f64| key_time(key * DURATION, DURATION);
		assert_eq!(
			track.attribute("keyTimes"),
			Some(
				format!(
					"0;0;{};{};{};1",
					key(first.vanish_key),
					key(second.fire_key),
					key(second.vanish_key)
				)
				.as_str()
			)
		);
		assert!(first.vanish_ms > 500 && first.vanish_key * DURATION >= f64::from(first.vanish_ms));
		assert_eq!(
			outer.attribute("transform"),
			Some(format!("translate({thrust})").as_str())
		);
		for node in node_list {
			let probe = Probe::of(node);
			assert!(probe.static_at().1 >= HIDDEN_Y);
			for begin in &probe.begin_list {
				let vanish = probe.vanish(*begin);
				assert!(probe.shown(*begin) && probe.shown(vanish - 1.0), "{begin}");
				assert!(!probe.shown(vanish) && !probe.shown(vanish + f64::from(hide_ms) - 1.0));
				let hidden = vanish + f64::from(hide_ms);
				assert!(
					probe.shown(hidden) && probe.at(hidden, *begin).1 >= HIDDEN_Y,
					"{begin}"
				);
				assert!(probe.at(DURATION, *begin).1 >= HIDDEN_Y, "{begin}");
			}
		}
	}

	#[test]
	fn quick_shots_skip_the_band_pull() {
		let flight_list = [
			flight(0, 0, 100),
			flight(1, 0, 110),
			flight(2, 0, 110),
			flight(3, 0, 600),
		];
		let band = band(&flight_list, DURATION, palette(Variant::Light));
		let pulled = band_path(0.5);
		let full = band_path(1.0);
		assert_eq!(band.matches(&pulled).count(), 1, "{band}");
		assert_eq!(band.matches(&full).count(), 1, "{band}");
		assert!(
			band.contains("keyTimes=\"0;0;.01;.01;.04;.06;.06;1\""),
			"{band}"
		);
	}
}
