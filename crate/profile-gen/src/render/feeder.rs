use std::collections::BTreeSet;
use std::f64::consts::TAU;

use super::format::{escape, key_time, num, seconds, whole_ms};
use super::heat::{EMBER_EXTENT, Heat};
use super::languages::{GROUND, TOKEN_RADIUS, WIDTH};
use super::tube::{tube, tube_defs};
use crate::theme::Palette;

pub(super) const SLINGSHOT: &str = "#e84e31";
pub(super) const FLIGHT_RADIUS: f64 = TOKEN_RADIUS / 2.0;
pub(super) const FLIGHT_SCALE: f64 = FLIGHT_RADIUS / TOKEN_RADIUS;

pub(super) const SLING_X: f64 = 36.0;
const PRONG: f64 = 11.0;
pub(super) const RING: f64 = 4.5;
const STROKE: f64 = 5.5;
const FOOT_Y: f64 = GROUND - 6.0;
const SLING_HEIGHT: f64 = 46.0;
pub(super) const PRONG_Y: f64 = FOOT_Y - SLING_HEIGHT;
const JOIN_Y: f64 = PRONG_Y + SLING_HEIGHT / 2.0;
pub(super) const LEFT_PRONG_X: f64 = SLING_X - PRONG;
pub(super) const RIGHT_PRONG_X: f64 = SLING_X + PRONG;

const BAND_END_Y: f64 = PRONG_Y + RING;
const LEFT_END: Point = (LEFT_PRONG_X, BAND_END_Y);
const RIGHT_END: Point = (RIGHT_PRONG_X, BAND_END_Y);
const REST_SAG: f64 = 3.0;
const QUADRATIC_TO_CUBIC: f64 = 2.0 / 3.0;
const REST_MIDDLE: Point = (SLING_X, BAND_END_Y + REST_SAG);
pub(super) const POUCH_X: f64 = SLING_X - 14.0;
pub(super) const POUCH_Y: f64 = PRONG_Y + 16.0;
const BAND_WIDTH: f64 = 1.5;
const WRAP: f64 = FLIGHT_RADIUS + BAND_WIDTH / 2.0;
const PULL_MS: u32 = 200;
const MIN_PULL_MS: u32 = 40;
const SNAP_FORWARD: f64 = 0.35;
const SNAP_SWING_MS: u32 = 30;
const SNAP_MS: u32 = 90;

pub(super) const GRAVITY: f64 = 0.0024;
const APEX_MARGIN: f64 = 10.0;
const APEX_PER_PX: f64 = 0.03;
const APEX_PER_LIFT: f64 = 0.55;
const RESTING_Y: f64 = GROUND - FLIGHT_RADIUS;
const HIDDEN_Y: f64 = GROUND + EMBER_EXTENT.max(FLIGHT_RADIUS);
pub(super) const FREE_FALL_SPLINE: &str = ".333 0 .667 .333";
const FIRE_KEY_STEPS: f64 = 10_000.0;
const VANISH_KEY_STEPS: f64 = 1_000.0;
const THROW_SLACK_MS: f64 = 0.5;
const THROW_SEARCH: u32 = 64;

type Point = (f64, f64);

#[derive(Clone, Copy)]
enum Pull {
	Rest,
	Loaded,
	Forward,
}

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

pub(super) fn feeder(
	flight_list: &[Flight],
	heat: &Heat,
	duration: f64,
	colors: Palette,
) -> String {
	let burn = heat.burn(duration);
	[
		defs(flight_list, &burn.defs(), duration),
		format!("<g data-feed=\"\">{}", burn.shake()),
		tube(flight_list, &burn.jiggle(), duration, colors),
		band(
			flight_list,
			&burn.band_stroke(colors.muted),
			duration,
			colors,
		),
		"</g>".to_owned(),
	]
	.concat()
}

pub(super) fn fork(heat: &Heat, duration: f64) -> String {
	let burn = heat.burn(duration);
	[
		format!("<g data-rig=\"\">{}", burn.shake()),
		burn.glow(),
		slingshot(),
		burn.flames(),
		burn.bolt(),
		burn.sparks(),
		"</g>".to_owned(),
	]
	.concat()
}

fn defs(flight_list: &[Flight], heat_defs: &str, duration: f64) -> String {
	let lane_set: BTreeSet<usize> = flight_list.iter().map(|flight| flight.lane).collect();
	let marker_list: Vec<String> = lane_set
		.iter()
		.map(|lane| {
			format!(
				concat!(
					"<g id=\"q{lane}\"><use href=\"#i{lane}\" transform=\"scale({})\"/></g>",
					"<g id=\"h{lane}\"><use href=\"#q{lane}\"/><use href=\"#e\"/></g>",
					"<marker id=\"m{lane}\" markerUnits=\"userSpaceOnUse\" overflow=\"visible\"><use href=\"#q{lane}\"/></marker>",
				),
				num(FLIGHT_SCALE, 2),
				lane = lane,
			)
		})
		.collect();
	format!(
		"<defs>{}{heat_defs}{}<clipPath id=\"kf\"><rect width=\"{}\" height=\"{}\"/></clipPath></defs>",
		marker_list.concat(),
		tube_defs(flight_list, duration),
		num(WIDTH, 1),
		num(GROUND, 1),
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
			"<g id=\"fk\"><path d=\"M{right} {below}V{above_foot}M{left} {below}Q{left} {join} {right} {join}\"/>",
			"{left_ring}{right_ring}{foot_ring}</g>",
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

fn toward(from: Point, to: Point, part: f64) -> Point {
	(
		(to.0 - from.0).mul_add(part, from.0),
		(to.1 - from.1).mul_add(part, from.1),
	)
}

fn touch_angle(end: Point, turn: f64) -> f64 {
	let (dx, dy) = (end.0 - POUCH_X, end.1 - POUCH_Y);
	turn.mul_add((WRAP / dx.hypot(dy)).acos(), dy.atan2(dx))
}

fn on_wrap(angle: f64, reach: f64) -> Point {
	(
		WRAP.mul_add(angle.cos(), -reach * angle.sin()) + POUCH_X,
		WRAP.mul_add(angle.sin(), reach * angle.cos()) + POUCH_Y,
	)
}

fn wrapped_strap() -> [Point; 4] {
	let from = touch_angle(LEFT_END, -1.0);
	let to = touch_angle(RIGHT_END, 1.0);
	let sweep = (to - from).rem_euclid(TAU) - TAU;
	let handle = WRAP * 4.0 / 3.0 * (sweep / 4.0).tan();
	[
		on_wrap(from, 0.0),
		on_wrap(from, handle),
		on_wrap(to, -handle),
		on_wrap(to, 0.0),
	]
}

fn strap(pull: Pull) -> String {
	let [left_touch, first, second, right_touch] = match pull {
		Pull::Rest => {
			let control = (SLING_X, REST_SAG.mul_add(2.0, BAND_END_Y));
			[
				LEFT_END,
				toward(LEFT_END, control, QUADRATIC_TO_CUBIC),
				toward(RIGHT_END, control, QUADRATIC_TO_CUBIC),
				RIGHT_END,
			]
		}
		Pull::Loaded => wrapped_strap(),
		Pull::Forward => [toward(REST_MIDDLE, (POUCH_X, POUCH_Y), -SNAP_FORWARD); 4],
	};
	let point = |at: Point| format!("{} {}", num(at.0, 1), num(at.1, 1));
	format!(
		"M{}L{}C{} {} {}L{}",
		point(LEFT_END),
		point(left_touch),
		point(first),
		point(second),
		point(right_touch),
		point(RIGHT_END),
	)
}

fn band(flight_list: &[Flight], heat_stroke: &str, duration: f64, colors: Palette) -> String {
	let [rest, loaded, forward] = [Pull::Rest, Pull::Loaded, Pull::Forward].map(strap);
	let mut fire_list: Vec<u32> = flight_list.iter().map(|flight| flight.fire_ms).collect();
	fire_list.dedup();
	let mut free_ms = 0;
	let mut held = false;
	let starts_loaded = fire_list.first().is_some_and(|first| *first < MIN_PULL_MS);
	let mut point_list: Vec<(u32, &str)> = vec![(0, if starts_loaded { &loaded } else { &rest })];
	for (index, fire) in fire_list.iter().copied().enumerate() {
		if !held {
			let pull_ms = free_ms.max(fire.saturating_sub(PULL_MS));
			point_list.extend([
				(pull_ms, point_list[point_list.len() - 1].1),
				(fire, &loaded),
			]);
		}
		held = fire_list
			.get(index + 1)
			.is_some_and(|next| next - fire < SNAP_MS + MIN_PULL_MS);
		if !held {
			free_ms = fire + SNAP_MS;
			point_list.extend([
				(fire, loaded.as_str()),
				(fire + SNAP_SWING_MS, &forward),
				(free_ms, &rest),
			]);
		}
	}
	point_list.dedup();
	let value_list: Vec<&str> = point_list
		.iter()
		.map(|(_, path)| *path)
		.chain([rest.as_str()])
		.collect();
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(ms, _)| key_time(f64::from(*ms).min(duration), duration))
		.chain([key_time(duration, duration)])
		.collect();
	format!(
		concat!(
			"<path data-band=\"\" d=\"{rest}\" fill=\"none\" stroke=\"{muted}\" stroke-width=\"{width}\" stroke-linejoin=\"round\">",
			"<animate attributeName=\"d\" values=\"{values}\" keyTimes=\"{keys}\" dur=\"{dur}ms\" fill=\"freeze\"/>{heat_stroke}",
			"</path>",
		),
		rest = rest,
		muted = colors.muted,
		width = num(BAND_WIDTH, 1),
		values = value_list.join(";"),
		keys = key_list.join(";"),
		dur = num(duration, 0),
		heat_stroke = heat_stroke,
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
		(flight.to_x - POUCH_X) * flown / (absorbed_ms(flight) - fire),
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
		x = num(POUCH_X, 1),
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
	use super::super::probe::{
		Pool as Probe, aimed_flight as aimed, command_list, off_line, path_samples, point_list,
		test_flight as flight, value_at, wrapped,
	};
	use super::*;
	use crate::theme::{Variant, palette};
	use roxmltree::{Document, Node};

	const DURATION: f64 = 10_000.0;

	fn measured(dx: f64, landing_y: f64) -> (f64, f64) {
		let fire_ms = 100;
		let flight = Flight {
			land_ms: fire_ms + u32::try_from(flight_ms(dx, landing_y)).unwrap(),
			to_x: POUCH_X + dx,
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
		assert!((absorbed.0 - POUCH_X - dx).abs() <= 1.5, "{absorbed:?}");
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
	fn farther_and_higher_targets_fly_higher_and_longer() {
		let (near_apex, near_ms) = measured(56.0, RESTING_Y);
		let (far_apex, far_ms) = measured(700.0, RESTING_Y);
		assert!(far_apex < near_apex - 10.0, "{near_apex} {far_apex}");
		assert!(far_ms > near_ms + 30.0, "{near_ms} {far_ms}");
		assert!(near_ms < 400.0, "{near_ms}");
		let mut previous = (f64::INFINITY, 0.0);
		for landing_y in [RESTING_Y, POUCH_Y, 260.0, 230.0, 180.0, 140.0, 94.0] {
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

	fn calm_and_rapid() -> Vec<Flight> {
		vec![
			flight(0, 0, 1_000),
			flight(1, 0, 2_000),
			flight(2, 0, 3_000),
			flight(0, 0, 3_040),
			flight(1, 0, 3_080),
			flight(2, 0, 4_000),
		]
	}

	fn emitted(flight_list: &[Flight]) -> String {
		let heat = Heat {
			start_ms: 0,
			sample_list: Vec::new(),
		};
		wrapped(&format!(
			"{}{}{}",
			feeder(flight_list, &heat, DURATION, palette(Variant::Dark)),
			flights(flight_list, DURATION),
			fork(&heat, DURATION)
		))
	}

	fn tagged<'a>(document: &'a Document, key: &str) -> Node<'a, 'a> {
		document
			.descendants()
			.find(|node| node.has_attribute(key))
			.unwrap()
	}

	fn number(node: Node, name: &str) -> f64 {
		node.attribute(name).unwrap().parse().unwrap()
	}

	fn distance(from: (f64, f64), to: (f64, f64)) -> f64 {
		(to.0 - from.0).hypot(to.1 - from.1)
	}

	fn band_shapes(band: Node) -> Vec<String> {
		let animation = band.first_element_child().unwrap();
		let mut shape_list = vec![band.attribute("d").unwrap().to_owned()];
		shape_list.extend(
			animation
				.attribute("values")
				.unwrap()
				.split(';')
				.map(str::to_owned),
		);
		shape_list
	}

	fn circumcenter(first: (f64, f64), second: (f64, f64), third: (f64, f64)) -> (f64, f64) {
		let square = |point: (f64, f64)| point.0.mul_add(point.0, point.1 * point.1);
		let twice_area = 2.0
			* first.0.mul_add(
				second.1 - third.1,
				second
					.0
					.mul_add(third.1 - first.1, third.0 * (first.1 - second.1)),
			);
		(
			square(first).mul_add(
				second.1 - third.1,
				square(second).mul_add(third.1 - first.1, square(third) * (first.1 - second.1)),
			) / twice_area,
			square(first).mul_add(
				third.0 - second.0,
				square(second).mul_add(first.0 - third.0, square(third) * (second.0 - first.0)),
			) / twice_area,
		)
	}

	fn ends(shape: &str) -> [(f64, f64); 2] {
		let point_list: Vec<(f64, f64)> = command_list(shape)
			.iter()
			.flat_map(|(_, number_list)| point_list(number_list))
			.collect();
		[point_list[0], point_list[point_list.len() - 1]]
	}

	#[test]
	fn band_sits_behind_the_fork_and_ends_under_the_rings() {
		let svg = emitted(&calm_and_rapid());
		let document = Document::parse(&svg).unwrap();
		let order = |key: &str| {
			document
				.descendants()
				.position(|node| node.has_attribute(key))
				.unwrap()
		};
		assert!(order("data-band") < order("data-slingshot"));
		let fork = tagged(&document, "data-slingshot");
		let stroke = number(fork, "stroke-width");
		let ring_list: Vec<((f64, f64), f64)> = fork
			.descendants()
			.filter(|node| node.has_tag_name("circle"))
			.map(|ring| ((number(ring, "cx"), number(ring, "cy")), number(ring, "r")))
			.collect();
		let shape_list = band_shapes(tagged(&document, "data-band"));
		assert!(shape_list.len() > 10, "{shape_list:?}");
		for shape in shape_list {
			for end in ends(&shape) {
				let gap = ring_list
					.iter()
					.map(|(center, radius)| (distance(end, *center) - radius).abs())
					.fold(f64::INFINITY, f64::min);
				assert!(gap <= stroke / 2.0, "{shape} {end:?} {gap}");
			}
		}
	}

	#[test]
	fn band_arcs_at_rest_and_bends_into_a_v_when_pulled() {
		let flight_list = calm_and_rapid();
		let svg = emitted(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let band = tagged(&document, "data-band");
		let animation = band.first_element_child().unwrap();
		let rest = band.attribute("d").unwrap();
		let rest_samples = path_samples(rest, 20);
		let [first, last] = ends(rest);
		assert!((first.1 - last.1).abs() < 1e-9, "{rest}");
		let lowest = rest_samples
			.iter()
			.copied()
			.max_by(|left, right| left.1.total_cmp(&right.1))
			.unwrap();
		assert!(lowest.1 > first.1 + 2.0, "{rest}");
		assert!(rest_samples.iter().all(|point| point.1 >= first.1 - 1e-9));
		let left_half: Vec<(f64, f64)> = rest_samples
			.iter()
			.copied()
			.filter(|point| point.0 <= lowest.0)
			.collect();
		assert!(off_line(&left_half) > 0.3, "{rest}");
		for flight in &flight_list {
			let pulled = value_at(animation, f64::from(flight.fire_ms));
			let command_list: Vec<(char, Vec<(f64, f64)>)> = command_list(&pulled)
				.into_iter()
				.map(|(letter, number_list)| (letter, point_list(&number_list)))
				.collect();
			let letter_list: String = command_list.iter().map(|(letter, _)| *letter).collect();
			assert_eq!(letter_list, "MLCL", "{pulled}");
			let left_end = command_list[0].1[0];
			let left_touch = command_list[1].1[0];
			let right_touch = command_list[2].1[2];
			let right_end = command_list[3].1[0];
			assert!(distance(left_end, left_touch) > 5.0, "{pulled}");
			assert!(distance(right_end, right_touch) > 5.0, "{pulled}");
			assert!(left_touch.1 > left_end.1 + 5.0, "{pulled}");
			let wrap = path_samples(&pulled, 20)
				.into_iter()
				.skip(21)
				.take(20)
				.collect::<Vec<(f64, f64)>>();
			let pouch = circumcenter(left_touch, wrap[9], right_touch);
			let toward = |end: (f64, f64)| (end.1 - pouch.1).atan2(end.0 - pouch.0);
			let opening = (toward(right_end) - toward(left_end)).abs().to_degrees();
			assert!(opening > 30.0 && opening < 150.0, "{pulled} {opening}");
			let spread = wrap
				.iter()
				.flat_map(|from| wrap.iter().map(|to| distance(*from, *to)))
				.fold(0.0, f64::max);
			assert!(spread < 4.0 * FLIGHT_RADIUS, "{pulled} {spread}");
		}
		let value_list: Vec<&str> = animation.attribute("values").unwrap().split(';').collect();
		assert_eq!(value_list.last(), Some(&rest));
		assert_eq!(animation.attribute("fill"), Some("freeze"));
	}

	#[test]
	fn loaded_circle_sits_in_the_band() {
		let svg = emitted(&calm_and_rapid());
		let document = Document::parse(&svg).unwrap();
		let band = tagged(&document, "data-band");
		let animation = band.first_element_child().unwrap();
		let touch = FLIGHT_RADIUS + number(band, "stroke-width") / 2.0;
		let mut checked = 0;
		for node in document
			.descendants()
			.filter(|node| node.has_attribute("data-flight"))
		{
			let probe = Probe::of(node);
			for begin in &probe.begin_list {
				let center = probe.at(*begin, *begin);
				let shape = value_at(animation, *begin);
				let [first, last] = ends(&shape);
				let middle = (
					f64::midpoint(first.0, last.0),
					f64::midpoint(first.1, last.1),
				);
				let closest = path_samples(&shape, 200)
					.into_iter()
					.min_by(|left, right| {
						distance(*left, center).total_cmp(&distance(*right, center))
					})
					.unwrap();
				let gap = distance(closest, center);
				assert!(
					gap >= touch - 0.1 && gap <= touch + 0.2,
					"{begin} {gap} {shape}"
				);
				let facing = (closest.0 - center.0).mul_add(
					middle.0 - center.0,
					(closest.1 - center.1) * (middle.1 - center.1),
				);
				assert!(facing < 0.0, "{begin} {closest:?} {shape}");
				checked += 1;
			}
		}
		assert_eq!(checked, 6);
	}

	#[test]
	fn every_pull_lasts_long_enough_to_be_seen() {
		let flight_list = [
			flight(0, 0, 10),
			flight(1, 0, 105),
			flight(2, 0, 234),
			flight(0, 0, 364),
			flight(1, 0, 600),
			flight(2, 0, 1_000),
		];
		let svg = emitted(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let band = tagged(&document, "data-band");
		let animation = band.first_element_child().unwrap();
		let rest = band.attribute("d").unwrap();
		let loaded = value_at(animation, 10.0);
		assert_ne!(loaded, rest);
		let value_list: Vec<&str> = animation.attribute("values").unwrap().split(';').collect();
		let key_list: Vec<f64> = animation
			.attribute("keyTimes")
			.unwrap()
			.split(';')
			.map(|key| key.parse::<f64>().unwrap() * DURATION)
			.collect();
		let mut pulls = 0;
		for index in 1..value_list.len() {
			if value_list[index - 1] == rest && value_list[index] == loaded {
				let span = key_list[index] - key_list[index - 1];
				assert!(span >= 40.0 - 0.5, "{index} {span}");
				pulls += 1;
			}
		}
		assert_eq!(pulls, 3);
		for flight in &flight_list {
			assert_eq!(value_at(animation, f64::from(flight.fire_ms)), loaded);
		}
	}

	#[test]
	#[should_panic(expected = "unsupported path command Q")]
	fn sampling_rejects_unknown_path_commands() {
		path_samples("M0 0Q1 1 2 2", 2);
	}

	#[test]
	fn rapid_shots_hold_the_band_pulled_and_calm_shots_snap_forward() {
		let flight_list = [
			flight(0, 0, 100),
			flight(1, 0, 110),
			flight(2, 0, 110),
			flight(3, 0, 600),
		];
		let band = band(&flight_list, "", DURATION, palette(Variant::Light));
		assert_eq!(band.matches(&strap(Pull::Loaded)).count(), 3, "{band}");
		assert_eq!(band.matches(&strap(Pull::Forward)).count(), 2, "{band}");
		assert!(
			band.contains("keyTimes=\"0;.01;.011;.014;.02;.04;.06;.063;.069;1\""),
			"{band}"
		);
	}
}
