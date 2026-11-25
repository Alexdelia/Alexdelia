use std::collections::BTreeSet;

use super::format::{discrete, escape, float, key_time, num, whole_ms};
use super::languages::{GROUND, WIDTH};
use crate::theme::Palette;

pub(super) const SLINGSHOT: &str = "#e84e31";
pub(super) const TUBE_CAPACITY: usize = 11;

const SLING_X: f64 = 36.0;
const PRONG: f64 = 13.0;
const RING: f64 = 4.5;
const STROKE: f64 = 4.5;
const PRONG_Y: f64 = 200.0;
const FOOT_Y: f64 = 294.0;
const JOIN_Y: f64 = PRONG_Y + 44.0;
const LEFT_PRONG_X: f64 = SLING_X - PRONG;
const RIGHT_PRONG_X: f64 = SLING_X + PRONG;

const POUCH_Y: f64 = PRONG_Y + 6.0;
const PULL_X: f64 = -14.0;
const PULL_Y: f64 = 10.0;
const PULL_MS: u32 = 200;
const QUICK_PULL_MS: u32 = 40;
const BAND_WIDTH: f64 = 1.5;

const TUBE_TOP: f64 = 16.0;
const TUBE_BOTTOM: f64 = PRONG_Y - 6.0;
const TUBE_SCALE: f64 = 0.8;
const TUBE_RADIUS: f64 = 8.0;
const TUBE_STEP: f64 = 2.0 * TUBE_RADIUS;
const TUBE_WALL: f64 = TUBE_RADIUS + 3.0;
const TUBE_REST_Y: f64 = TUBE_BOTTOM - TUBE_RADIUS;
const WALL_OVERHANG: f64 = 8.0;
const WINDOW_HEIGHT: f64 = 400.0;

const APEX_Y: f64 = 64.0;
const SINK_Y: f64 = GROUND + 12.0;
const CLEAR_Y: f64 = 90.0;
const RISE_MS: u32 = 340;
const FALL_MS: u32 = 460;
const FLIGHT_MS: u32 = RISE_MS + FALL_MS;
const SAMPLE_COUNT: u32 = 20;
const MAX_STRETCH: f64 = 1.28;
const MAX_SKEW: f64 = 0.015;
const MAX_PIECES: u32 = 64;
const UPRIGHT_UNIT: f64 = 1_000.0;

pub(super) struct Flight {
	pub(super) lane: usize,
	pub(super) name: &'static str,
	pub(super) frame: usize,
	pub(super) enter_ms: u32,
	pub(super) fire_ms: u32,
	pub(super) land_ms: u32,
}

pub(super) struct Track<'a> {
	pub(super) lane: usize,
	pub(super) x_list: &'a [f64],
	pub(super) tick_ms: u32,
}

pub(super) fn flight_ms(landing_y: f64) -> i64 {
	whole_ms(reach_ms(landing_y))
}

fn reach_ms(y: f64) -> f64 {
	let fallen = ((y - APEX_Y) / (SINK_Y - APEX_Y)).clamp(0.0, 1.0);
	f64::from(FALL_MS).mul_add(fallen.sqrt(), f64::from(RISE_MS))
}

fn height_at(ms: f64) -> f64 {
	let rise = f64::from(RISE_MS);
	if ms < rise {
		let left = 1.0 - ms / rise;
		(POUCH_Y - APEX_Y).mul_add(left * left, APEX_Y)
	} else {
		let fallen = (ms - rise) / f64::from(FALL_MS);
		(SINK_Y - APEX_Y).mul_add(fallen * fallen, APEX_Y)
	}
}

fn reach_at(ms: f64) -> f64 {
	let left = (1.0 - ms / reach_ms(CLEAR_Y)).max(0.0);
	left.mul_add(-left, 1.0)
}

pub(super) fn feeder(
	flight_list: &[Flight],
	track_list: &[Track],
	duration: f64,
	colors: Palette,
) -> String {
	let volley_list = volley_list(flight_list, track_list);
	[
		defs(flight_list, &volley_list, duration),
		tube(flight_list, duration, colors),
		slingshot(),
		band(flight_list, duration, colors),
		flights(&volley_list),
	]
	.concat()
}

fn defs(flight_list: &[Flight], volley_list: &[Volley], duration: f64) -> String {
	let lane_set: BTreeSet<usize> = flight_list.iter().map(|flight| flight.lane).collect();
	let marker_list: Vec<String> = lane_set
		.iter()
		.map(|lane| {
			format!(
				"<marker id=\"m{lane}\" markerUnits=\"userSpaceOnUse\" overflow=\"visible\"><use href=\"#i{lane}\" transform=\"scale({})\"/></marker>",
				num(TUBE_SCALE, 2)
			)
		})
		.collect();
	format!(
		concat!(
			"<defs>{markers}{logos}",
			"<clipPath id=\"tb\"><rect x=\"{tube_x}\" y=\"{tube_y}\" width=\"{tube_w}\" height=\"{tube_h}\"/></clipPath>",
			"<clipPath id=\"tw\"><rect y=\"{bottom}\" width=\"{w}\" height=\"{window_h}\">{window}</rect></clipPath>",
			"<clipPath id=\"kf\"><rect width=\"{w}\" height=\"{g}\"/></clipPath>",
			"</defs>",
		),
		markers = marker_list.concat(),
		logos = upright_logos(volley_list),
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

struct Volley<'a> {
	lane: usize,
	name: &'static str,
	x_list: &'a [f64],
	tick_ms: u32,
	pool_list: Vec<Vec<u32>>,
}

fn volley_list<'a>(flight_list: &[Flight], track_list: &[Track<'a>]) -> Vec<Volley<'a>> {
	track_list
		.iter()
		.map(|track| {
			let own: Vec<&Flight> = flight_list
				.iter()
				.filter(|flight| flight.lane == track.lane)
				.collect();
			let mut pool_list: Vec<Vec<u32>> = Vec::new();
			for flight in &own {
				let free = pool_list.iter().position(|pool| {
					pool.last()
						.is_some_and(|last| last + FLIGHT_MS <= flight.fire_ms)
				});
				match free {
					Some(index) => pool_list[index].push(flight.fire_ms),
					None => pool_list.push(vec![flight.fire_ms]),
				}
			}
			Volley {
				lane: track.lane,
				name: own.first().map_or("", |flight| flight.name),
				x_list: track.x_list,
				tick_ms: track.tick_ms,
				pool_list,
			}
		})
		.filter(|volley| !volley.pool_list.is_empty())
		.collect()
}

fn begins(list: &[u32]) -> String {
	let text_list: Vec<String> = list.iter().map(|ms| format!("{ms}ms")).collect();
	text_list.join(";")
}

fn trajectory() -> String {
	let sample_list: Vec<String> = (0..=SAMPLE_COUNT)
		.map(|sample| {
			let ms = f64::from(FLIGHT_MS * sample) / f64::from(SAMPLE_COUNT);
			format!("{} {}", num(reach_at(ms), 3), num(height_at(ms), 1))
		})
		.collect();
	sample_list.join(";")
}

fn reach_list(volley: &Volley) -> Vec<f64> {
	volley.x_list.iter().map(|x| x - SLING_X).collect()
}

fn kept_ticks(reach_list: &[f64]) -> Vec<usize> {
	let last = reach_list.len().saturating_sub(1);
	(0..=last)
		.filter(|index| *index == 0 || *index == last || bends(reach_list, *index))
		.collect()
}

fn bends(reach_list: &[f64], index: usize) -> bool {
	let before = reach_list[index] - reach_list[index - 1];
	let after = reach_list[index + 1] - reach_list[index];
	before.total_cmp(&after).is_ne()
}

fn stretch(from: f64, to: f64) -> f64 {
	from.max(to) / from.min(to)
}

fn pieces(from: f64, to: f64) -> u32 {
	let ratio = stretch(from, to);
	(1..=MAX_PIECES)
		.find(|count| ratio.powf(1.0 / f64::from(*count)) <= MAX_STRETCH)
		.unwrap_or(MAX_PIECES)
}

fn scale_animation(value_list: &[String], key_list: Option<&[String]>, duration: f64) -> String {
	let keys = key_list.map_or_else(String::new, |list| {
		format!(" keyTimes=\"{}\"", list.join(";"))
	});
	format!(
		concat!(
			"transform=\"scale({last})\">",
			"<animateTransform attributeName=\"transform\" type=\"scale\" calcMode=\"linear\" values=\"{values}\"{keys} dur=\"{dur}ms\" fill=\"freeze\"/>",
		),
		last = value_list.last().cloned().unwrap_or_default(),
		values = value_list.join(";"),
		keys = keys,
		dur = num(duration, 0),
	)
}

fn track_duration(volley: &Volley) -> f64 {
	float(volley.x_list.len().saturating_sub(1).max(1)) * f64::from(volley.tick_ms)
}

fn follow(volley: &Volley) -> String {
	let reach_list = reach_list(volley);
	let scale = |reach: f64| format!("{} 1", num(reach, 1));
	let duration = track_duration(volley);
	let last = float(reach_list.len().saturating_sub(1).max(1));
	let every: Vec<String> = reach_list.iter().map(|reach| scale(*reach)).collect();
	let kept = kept_ticks(&reach_list);
	let keyed_values: Vec<String> = kept.iter().map(|index| scale(reach_list[*index])).collect();
	let keyed_keys: Vec<String> = kept
		.iter()
		.map(|index| key_time(float(*index), last))
		.collect();
	[
		scale_animation(&every, None, duration),
		scale_animation(&keyed_values, Some(&keyed_keys), duration),
	]
	.into_iter()
	.min_by_key(String::len)
	.unwrap_or_default()
}

fn reach_between(reach_list: &[f64], quarter: usize) -> f64 {
	let tick = quarter / 4;
	let from = reach_list[tick];
	let to = reach_list.get(tick + 1).copied().unwrap_or(from);
	(to - from).mul_add(float(quarter % 4) / 4.0, from)
}

fn fits(reach_list: &[f64], from: usize, to: usize) -> bool {
	let (start, end) = (1.0 / reach_list[from], 1.0 / reach_list[to]);
	let span = float(to - from) * 4.0;
	(from * 4..=to * 4).all(|quarter| {
		let part = float(quarter - from * 4) / span;
		let inverse = (end - start).mul_add(part, start);
		reach_between(reach_list, quarter)
			.mul_add(inverse, -1.0)
			.abs()
			<= MAX_SKEW
	})
}

fn upright_points(reach_list: &[f64]) -> Vec<(f64, f64)> {
	let last = reach_list.len().saturating_sub(1);
	let mut point_list: Vec<(f64, f64)> = reach_list
		.first()
		.map(|reach| (0.0, *reach))
		.into_iter()
		.collect();
	let mut from = 0;
	while from < last {
		let mut to = from + 1;
		if fits(reach_list, from, to) {
			while to < last && fits(reach_list, from, to + 1) {
				to += 1;
			}
		} else {
			let (start, end) = (reach_list[from], reach_list[to]);
			let count = pieces(start, end);
			let step = (end / start).powf(1.0 / f64::from(count));
			point_list.extend((1..count).map(|piece| {
				let reach = start * step.powi(i32::try_from(piece).unwrap_or(i32::MAX));
				(float(from) + (reach - start) / (end - start), reach)
			}));
		}
		point_list.push((float(to), reach_list[to]));
		from = to;
	}
	point_list
}

fn unfollow(volley: &Volley) -> String {
	let reach_list = reach_list(volley);
	let scale = |reach: f64| format!("{} 1", num(UPRIGHT_UNIT / reach, 3));
	let duration = track_duration(volley);
	let last = float(reach_list.len().saturating_sub(1).max(1));
	let point_list = upright_points(&reach_list);
	let value_list: Vec<String> = point_list.iter().map(|(_, reach)| scale(*reach)).collect();
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(tick, _)| key_time(*tick, last))
		.collect();
	scale_animation(&value_list, Some(&key_list), duration)
}

fn flights(volley_list: &[Volley]) -> String {
	let path = trajectory();
	let volley_text: Vec<String> = volley_list
		.iter()
		.map(|volley| {
			let pool_text: Vec<String> = volley
				.pool_list
				.iter()
				.map(|begin_list| {
					format!(
						concat!(
							"<g transform=\"translate(1 {sink})\">",
							"<animateTransform attributeName=\"transform\" type=\"translate\" calcMode=\"linear\" values=\"{path}\" dur=\"{flight}ms\" begin=\"{begin}\" fill=\"freeze\"/>",
							"<use href=\"#j{lane}\" data-flight=\"{name}\"/>",
							"</g>",
						),
						sink = num(SINK_Y, 1),
						path = path,
						flight = FLIGHT_MS,
						begin = begins(begin_list),
						lane = volley.lane,
						name = escape(volley.name),
					)
				})
				.collect();
			format!(
				"<g transform=\"translate({x})\"><g {scale}{pools}</g></g>",
				x = num(SLING_X, 1),
				scale = follow(volley),
				pools = pool_text.concat(),
			)
		})
		.collect();
	format!("<g clip-path=\"url(#kf)\">{}</g>", volley_text.concat())
}

fn upright_logos(volley_list: &[Volley]) -> String {
	let logo_list: Vec<String> = volley_list
		.iter()
		.map(|volley| {
			format!(
				"<g id=\"j{lane}\" {scale}<use href=\"#i{lane}\" transform=\"scale({unit} 1)\"/></g>",
				lane = volley.lane,
				scale = unfollow(volley),
				unit = num(1.0 / UPRIGHT_UNIT, 3),
			)
		})
		.collect();
	logo_list.concat()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::{Variant, palette};
	use roxmltree::{Document, Node};

	const DURATION: f64 = 10_000.0;

	fn flight(lane: usize, enter_ms: u32, fire_ms: u32) -> Flight {
		Flight {
			lane,
			name: "Rust",
			frame: 0,
			enter_ms,
			fire_ms,
			land_ms: fire_ms + 500,
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

	#[test]
	fn tube_capacity_is_what_fits() {
		let places = (TUBE_BOTTOM - TUBE_TOP) / TUBE_STEP;
		assert!(f64::from(u32::try_from(TUBE_CAPACITY).unwrap()) <= places);
		assert!(f64::from(u32::try_from(TUBE_CAPACITY + 1).unwrap()) > places);
		assert!(TUBE_SCALE.mul_add(-10.0, TUBE_RADIUS).abs() < 1e-9);
	}

	#[test]
	fn taller_bars_are_hit_sooner_and_after_the_aim() {
		let tallest = flight_ms(GROUND - 196.0 - 10.0);
		let shortest = flight_ms(GROUND - 10.0);
		assert!(tallest < shortest);
		assert!(reach_ms(CLEAR_Y) <= reach_ms(GROUND - 196.0 - 10.0));
		assert!(shortest < i64::from(FLIGHT_MS));
		assert!((height_at(reach_ms(150.0)) - 150.0).abs() < 1e-9);
		assert!((height_at(0.0) - POUCH_Y).abs() < 1e-9);
		assert!((height_at(f64::from(FLIGHT_MS)) - SINK_Y).abs() < 1e-9);
		assert!((reach_at(f64::from(FLIGHT_MS)) - 1.0).abs() < 1e-9);
		assert!(reach_at(0.0).abs() < 1e-9);
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
		let x_list = [100.0, 100.0];
		let track_list: Vec<Track> = (0..3)
			.map(|lane| Track {
				lane,
				x_list: &x_list,
				tick_ms: 100,
			})
			.collect();
		let svg = wrapped(&feeder(
			&flight_list,
			&track_list,
			DURATION,
			palette(Variant::Dark),
		));
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
		assert_eq!(tape_list[0].attribute("d"), Some("M0 0v-32h0"));
		assert_eq!(tape_list[1].attribute("d"), Some("M0 -16v-48h0"));
		assert_eq!(tape_list[2].attribute("d"), Some("M0 -48h0"));
		assert_eq!(tape_list[2].attribute("marker-start"), Some("url(#m2)"));
	}

	#[test]
	fn volleys_reuse_free_circles_and_follow_their_column() {
		let flight_list = [
			flight(0, 0, 0),
			flight(0, 0, 100),
			flight(0, 0, 900),
			flight(0, 0, 1_000),
			flight(1, 0, 1_000),
		];
		let sliding = [100.0, 100.0, 120.0, 140.0, 140.0];
		let resting = [300.0, 300.0, 300.0, 300.0, 300.0];
		let track_list = [
			Track {
				lane: 0,
				x_list: &sliding,
				tick_ms: 100,
			},
			Track {
				lane: 1,
				x_list: &resting,
				tick_ms: 100,
			},
			Track {
				lane: 2,
				x_list: &resting,
				tick_ms: 100,
			},
		];
		let volley_list = volley_list(&flight_list, &track_list);
		assert_eq!(volley_list.len(), 2);
		assert_eq!(volley_list[0].pool_list, [vec![0, 900], vec![100, 1_000]]);
		assert_eq!(volley_list[1].pool_list, [vec![1_000]]);
		let svg = wrapped(&format!(
			"<defs>{}</defs>{}",
			upright_logos(&volley_list),
			flights(&volley_list)
		));
		let document = Document::parse(&svg).unwrap();
		let scale_list: Vec<Node> = document
			.descendants()
			.filter(|node| node.attribute("type") == Some("scale"))
			.collect();
		assert_eq!(scale_list.len(), 4);
		assert_eq!(
			scale_list[0].attribute("values"),
			Some("15.625 1;15.625 1;13.639 1;11.905 1;9.615 1;9.615 1")
		);
		assert_eq!(
			scale_list[0].attribute("keyTimes"),
			Some("0;.25;.36652;.5;.75;1")
		);
		assert_eq!(scale_list[0].attribute("dur"), Some("400ms"));
		assert_eq!(scale_list[1].attribute("values"), Some("3.788 1;3.788 1"));
		assert_eq!(scale_list[1].attribute("keyTimes"), Some("0;1"));
		assert!(!scale_list[2].has_attribute("keyTimes"));
		assert_eq!(
			scale_list[2].attribute("values"),
			Some("64 1;64 1;84 1;104 1;104 1")
		);
		assert_eq!(scale_list[3].attribute("values"), Some("264 1;264 1"));
		let upright = document
			.descendants()
			.find(|node| node.attribute("id") == Some("j1"))
			.unwrap();
		assert_eq!(upright.attribute("transform"), Some("scale(3.788 1)"));
		assert_eq!(
			upright.last_element_child().unwrap().attribute("transform"),
			Some("scale(0.001 1)")
		);
		assert_eq!(
			upright.last_element_child().unwrap().attribute("href"),
			Some("#i1")
		);
		let use_list: Vec<Node> = document
			.descendants()
			.filter(|node| node.has_attribute("data-flight"))
			.collect();
		assert_eq!(use_list.len(), 3);
		assert_eq!(use_list[2].attribute("href"), Some("#j1"));
		assert_eq!(
			use_list[0]
				.parent()
				.unwrap()
				.parent()
				.unwrap()
				.attribute("transform"),
			Some("scale(104 1)")
		);
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
