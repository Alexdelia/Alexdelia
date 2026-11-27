use std::collections::BTreeSet;

use super::feeder::{
	ARC_HEADROOM, FLIGHT_RADIUS, FREE_FALL_SPLINE, Flight, GRAVITY, PRONG_Y, SLING_X,
};
use super::format::{discrete, float, num, seconds, whole_ms};
use super::heat::{JITTER_SALT, hash};
use super::languages::WIDTH;
use crate::theme::Palette;

const MAX_CAPACITY: u32 = 64;
pub(super) const TUBE_TOP: f64 = ARC_HEADROOM;
const TUBE_BOTTOM: f64 = PRONG_Y - 6.0;
const TUBE_RADIUS: f64 = FLIGHT_RADIUS;
const TUBE_STEP: f64 = 2.0 * TUBE_RADIUS;
const TUBE_WALL: f64 = TUBE_RADIUS + 3.0;
const TUBE_REST_Y: f64 = TUBE_BOTTOM - TUBE_RADIUS;
const WINDOW_HEIGHT: f64 = 400.0;
const BOUNCE_MS: u32 = 150;
const BOUNCE_OVERSHOOT: f64 = 1.5;
const BOUNCE_SPLINE: &str = ".5 0 1 1;0 0 .5 1";
const DROP_START_Y: f64 = TUBE_TOP - TUBE_RADIUS;
const DROP_SLACK: f64 = 0.5;

pub(super) fn tube_capacity() -> usize {
	(1..=MAX_CAPACITY)
		.take_while(|count| TUBE_STEP.mul_add(f64::from(*count), TUBE_TOP) <= TUBE_BOTTOM)
		.count()
}

pub(super) fn tube_defs(flight_list: &[Flight], duration: f64) -> String {
	let cut_top = TUBE_STEP.mul_add(-2.0, TUBE_TOP);
	format!(
		concat!(
			"<clipPath id=\"tb\"><rect x=\"{tube_x}\" y=\"{tube_y}\" width=\"{tube_w}\" height=\"{tube_h}\"/></clipPath>",
			"<clipPath id=\"tc\"><rect x=\"{tube_x}\" y=\"{cut_y}\" width=\"{tube_w}\" height=\"{cut_h}\"/></clipPath>",
			"<clipPath id=\"tw\"><rect y=\"{bottom}\" width=\"{w}\" height=\"{window_h}\">{window}</rect></clipPath>",
		),
		tube_x = num(SLING_X - TUBE_WALL, 1),
		tube_y = num(TUBE_TOP, 1),
		tube_w = num(2.0 * TUBE_WALL, 1),
		tube_h = num(TUBE_BOTTOM - TUBE_TOP, 1),
		cut_y = num(cut_top, 1),
		cut_h = num(TUBE_BOTTOM - cut_top, 1),
		bottom = num(TUBE_BOTTOM, 1),
		w = num(WIDTH, 1),
		window_h = num(WINDOW_HEIGHT, 1),
		window = window(flight_list, duration),
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

fn jitter(place: usize) -> i64 {
	i64::try_from(hash(place, JITTER_SALT) % 3).unwrap_or(1) - 1
}

fn lane_tape(lane: usize, place_list: &[usize]) -> String {
	let mut previous: Option<(i64, f64)> = None;
	let step_list: Vec<String> = place_list
		.iter()
		.map(|place| {
			let at = (jitter(*place), -TUBE_STEP * float(*place));
			let step = match previous {
				None => format!("M{} {}", at.0, num(at.1, 1)),
				Some(before) if before.0 == at.0 => format!("v{}", num(at.1 - before.1, 1)),
				Some(before) => format!("l{} {}", at.0 - before.0, num(at.1 - before.1, 1)),
			};
			previous = Some(at);
			step
		})
		.collect();
	format!(
		"<path d=\"{}h0\" marker-start=\"url(#m{lane})\" marker-mid=\"url(#m{lane})\"/>",
		step_list.concat(),
	)
}

fn distinct_fires(flight_list: &[Flight]) -> Vec<u32> {
	let mut fire_list: Vec<u32> = flight_list.iter().map(|flight| flight.fire_ms).collect();
	fire_list.dedup();
	fire_list
}

fn bounce(flight_list: &[Flight]) -> String {
	let fire_list = distinct_fires(flight_list);
	let calm_list: Vec<String> = fire_list
		.iter()
		.enumerate()
		.filter(|(index, fire)| {
			fire_list
				.get(index + 1)
				.is_none_or(|next| next - **fire >= BOUNCE_MS)
		})
		.map(|(_, fire)| seconds(*fire))
		.collect();
	if calm_list.is_empty() {
		return String::new();
	}
	format!(
		concat!(
			"<animateTransform attributeName=\"transform\" type=\"translate\" calcMode=\"spline\" keySplines=\"{spline}\"",
			" keyTimes=\"0;.6;1\" values=\"0 {from};0 {over};0 0\" dur=\"{dur}ms\" begin=\"{begins}\" fill=\"freeze\"/>",
		),
		spline = BOUNCE_SPLINE,
		from = num(-TUBE_STEP, 1),
		over = num(BOUNCE_OVERSHOOT, 1),
		dur = BOUNCE_MS,
		begins = calm_list.join(";"),
	)
}

struct Queue {
	entries: Vec<u32>,
	fires: Vec<u32>,
	events: Vec<u32>,
}

impl Queue {
	fn new(flight_list: &[Flight]) -> Self {
		let mut entries: Vec<u32> = flight_list.iter().map(|flight| flight.enter_ms).collect();
		entries.sort_unstable();
		let fires: Vec<u32> = flight_list.iter().map(|flight| flight.fire_ms).collect();
		let mut events: Vec<u32> = entries.iter().chain(&fires).copied().collect();
		events.sort_unstable();
		events.dedup();
		Self {
			entries,
			fires,
			events,
		}
	}

	fn fired(&self, ms: u32) -> usize {
		self.fires.partition_point(|fire| *fire <= ms)
	}

	fn edge_before(&self, ms: u32) -> f64 {
		let entered = self.entries.partition_point(|enter| *enter < ms);
		let fired = self.fires.partition_point(|fire| *fire < ms);
		TUBE_STEP.mul_add(-(float(entered) - float(fired)), TUBE_BOTTOM)
	}

	fn edge_after(&self, ms: u32) -> f64 {
		let entered = self.entries.partition_point(|enter| *enter <= ms);
		TUBE_STEP.mul_add(-(float(entered) - float(self.fired(ms))), TUBE_BOTTOM)
	}

	fn events(&self, after: u32, until: u32) -> &[u32] {
		let from = self.events.partition_point(|event| *event <= after);
		let to = self.events.partition_point(|event| *event <= until);
		&self.events[from..to]
	}
}

fn fall_ms(depth: f64) -> i64 {
	whole_ms((2.0 * depth / GRAVITY).sqrt())
}

fn drop_ms() -> u32 {
	u32::try_from(fall_ms(TUBE_BOTTOM + TUBE_STEP - DROP_START_Y)).unwrap_or(u32::MAX)
}

fn drop_y(release_ms: u32, ms: u32) -> f64 {
	let fallen = f64::from(ms - release_ms);
	(GRAVITY / 2.0 * fallen).mul_add(fallen, DROP_START_Y)
}

fn release_ms(flight: &Flight, place: usize, queue: &Queue) -> Option<u32> {
	let enter = flight.enter_ms;
	if enter >= flight.fire_ms {
		return None;
	}
	let slot = TUBE_STEP.mul_add(float(queue.fired(enter)) - float(place), TUBE_REST_Y);
	let fall = fall_ms(slot - DROP_START_Y);
	let release = enter.checked_sub(u32::try_from(fall).unwrap_or(u32::MAX))?;
	let clear_above = queue.events(release, enter).iter().all(|event| {
		drop_y(release, *event) + TUBE_RADIUS <= queue.edge_before(*event) + DROP_SLACK
	});
	let hidden_below = queue
		.events(enter - 1, release + drop_ms())
		.iter()
		.all(|event| {
			drop_y(release, *event) - TUBE_RADIUS >= queue.edge_after(*event) - DROP_SLACK
		});
	(clear_above && hidden_below).then_some(release)
}

fn drops(flight_list: &[Flight]) -> String {
	let queue = Queue::new(flight_list);
	let mut pool_list: Vec<(usize, Vec<(u32, u32)>)> = Vec::new();
	for (place, flight) in flight_list.iter().enumerate() {
		let Some(release) = release_ms(flight, place, &queue) else {
			continue;
		};
		let fall = (release, flight.enter_ms);
		match pool_list.iter_mut().find(|(lane, fall_list)| {
			*lane == flight.lane && fall_list.last().is_some_and(|last| last.1 < release)
		}) {
			Some((_, fall_list)) => fall_list.push(fall),
			None => pool_list.push((flight.lane, vec![fall])),
		}
	}
	if pool_list.is_empty() {
		return String::new();
	}
	let deep = drop_y(0, drop_ms());
	let use_list: Vec<String> = pool_list
		.iter()
		.map(|(lane, fall_list)| {
			let begin_list: Vec<String> = fall_list.iter().map(|fall| seconds(fall.0)).collect();
			format!(
				concat!(
					"<use href=\"#q{lane}\" x=\"{x}\" y=\"{deep}\" data-entry=\"\">",
					"<animateMotion calcMode=\"spline\" keySplines=\"{spline}\" values=\"0 {lift};0 0\" dur=\"{dur}ms\" begin=\"{begins}\" fill=\"freeze\"/>",
					"</use>",
				),
				lane = lane,
				x = num(SLING_X, 1),
				deep = num(deep, 1),
				spline = FREE_FALL_SPLINE,
				lift = num(DROP_START_Y - deep, 1),
				dur = drop_ms(),
				begins = begin_list.join(";"),
			)
		})
		.collect();
	format!(
		"<g transform=\"translate(0 {})\" clip-path=\"url(#tw)\"><g transform=\"translate(0 {})\">{}</g></g>",
		num(-WINDOW_HEIGHT, 1),
		num(WINDOW_HEIGHT, 1),
		use_list.concat(),
	)
}

pub(super) fn tube(flight_list: &[Flight], jiggle: &str, duration: f64, colors: Palette) -> String {
	let wall = |x: f64| {
		format!(
			"M{} {}V{}",
			num(x, 1),
			num(TUBE_TOP, 1),
			num(TUBE_BOTTOM + 2.0, 1)
		)
	};
	let lane_set: BTreeSet<usize> = flight_list.iter().map(|flight| flight.lane).collect();
	let tape: Vec<String> = lane_set
		.iter()
		.map(|lane| {
			let place_list: Vec<usize> = flight_list
				.iter()
				.enumerate()
				.filter(|(_, flight)| flight.lane == *lane)
				.map(|(place, _)| place)
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
			"<g clip-path=\"url(#tb)\"><g>{jiggle}<g>{bounce}<g clip-path=\"url(#tc)\">",
			"<g clip-path=\"url(#tw)\"><g data-step=\"\" fill=\"none\" transform=\"translate({end})\">{step}{tape}</g></g>",
			"{drops}</g></g></g></g></g>",
		),
		left = wall(SLING_X - TUBE_WALL),
		right = wall(SLING_X + TUBE_WALL),
		jiggle = jiggle,
		bounce = bounce(flight_list),
		ground = colors.ground,
		end = tube_offset(fired),
		step = discrete(
			"<animateTransform attributeName=\"transform\" type=\"translate\"",
			&tube_offset(0.0),
			step_list,
			duration
		),
		tape = tape.concat(),
		drops = drops(flight_list),
	)
}

#[cfg(test)]
mod tests {
	use super::super::feeder::FLIGHT_SCALE;
	use super::super::feeder::GRAVITY;
	use super::super::format::seconds;
	use super::super::languages::TOKEN_RADIUS;
	use super::super::probe::{
		command_list, drop_at, drop_begins, test_flight as flight, value_at, vertex_list, wrapped,
	};
	use super::*;
	use crate::theme::{Variant, palette};
	use roxmltree::{Document, Node};

	const DURATION: f64 = 10_000.0;

	fn waiting_at(flight_list: &[Flight], ms: u32) -> usize {
		flight_list
			.iter()
			.filter(|flight| flight.enter_ms <= ms && ms < flight.fire_ms)
			.count()
	}

	fn rendered(flight_list: &[Flight]) -> String {
		wrapped(&format!(
			"<defs>{}</defs>{}",
			tube_defs(flight_list, DURATION),
			tube(flight_list, "", DURATION, palette(Variant::Dark))
		))
	}

	fn edge_at(flight_list: &[Flight], ms: f64) -> f64 {
		let waiting = flight_list
			.iter()
			.filter(|flight| f64::from(flight.enter_ms) <= ms && ms < f64::from(flight.fire_ms))
			.count();
		TUBE_STEP.mul_add(-f64::from(u32::try_from(waiting).unwrap()), TUBE_BOTTOM)
	}

	#[test]
	fn tube_capacity_is_what_fits() {
		let places = (TUBE_BOTTOM - TUBE_TOP) / TUBE_STEP;
		let capacity = f64::from(u32::try_from(tube_capacity()).unwrap());
		assert!(capacity <= places && capacity + 1.0 > places, "{capacity}");
		assert_eq!(tube_capacity(), 21);
		assert!(FLIGHT_SCALE.mul_add(-TOKEN_RADIUS, TUBE_RADIUS).abs() < 1e-9);
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
		let svg = rendered(&flight_list);
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
			.find(|node| node.has_attribute("data-step"))
			.unwrap()
			.first_element_child()
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
		let y_list: Vec<Vec<f64>> = tape_list
			.iter()
			.map(|tape| {
				let vertex_list = vertex_list(tape.attribute("d").unwrap());
				vertex_list.iter().map(|vertex| vertex.1).collect()
			})
			.collect();
		assert_eq!(
			y_list,
			[
				vec![0.0, -20.0, -20.0],
				vec![-10.0, -40.0, -40.0],
				vec![-30.0, -30.0]
			]
		);
		assert_eq!(tape_list[2].attribute("marker-start"), Some("url(#m2)"));
	}

	#[test]
	fn tube_steps_down_with_bounce_after_each_shot() {
		let flight_list = [
			flight(0, 0, 400),
			flight(1, 100, 900),
			flight(0, 300, 930),
			flight(2, 400, 1_200),
		];
		let svg = rendered(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let step = document
			.descendants()
			.find(|node| node.has_attribute("data-step"))
			.unwrap()
			.first_element_child()
			.unwrap();
		for (fired, flight) in (1_u32..).zip(&flight_list) {
			let at = f64::from(flight.fire_ms);
			assert_eq!(value_at(step, at), tube_offset(f64::from(fired)));
			assert_eq!(value_at(step, at - 1.0), tube_offset(f64::from(fired - 1)));
		}
		let bounce = document
			.descendants()
			.find(|node| node.attribute("calcMode") == Some("spline"))
			.unwrap();
		assert!(
			bounce
				.ancestors()
				.any(|node| node.has_attribute("data-tube"))
		);
		assert_eq!(
			bounce.attribute("begin"),
			Some([400, 930, 1_200].map(seconds).join(";").as_str())
		);
		let y_list: Vec<f64> = bounce
			.attribute("values")
			.unwrap()
			.split(';')
			.map(|value| value.split(' ').nth(1).unwrap().parse().unwrap())
			.collect();
		assert!((y_list[0] + TUBE_STEP).abs() < 1e-9, "{y_list:?}");
		assert!(
			y_list[1..y_list.len() - 1].iter().any(|y| *y > 0.5),
			"{y_list:?}"
		);
		assert!(y_list[y_list.len() - 1].abs() < 1e-9, "{y_list:?}");
		let duration: f64 = bounce
			.attribute("dur")
			.unwrap()
			.trim_end_matches("ms")
			.parse()
			.unwrap();
		assert!(duration < 270.0 && duration > 60.0, "{duration}");
	}

	#[test]
	fn circles_drop_into_the_tube() {
		let flight_list = [
			flight(0, 0, 300),
			flight(1, 300, 600),
			flight(0, 1_000, 2_000),
			flight(1, 1_300, 2_100),
			flight(0, 1_500, 2_200),
			flight(2, 2_300, 2_300),
		];
		let svg = rendered(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let drop_list: Vec<Node> = document
			.descendants()
			.filter(|node| node.has_attribute("data-entry"))
			.collect();
		let begin_count: usize = drop_list.iter().map(|node| drop_begins(*node).len()).sum();
		assert_eq!(begin_count, 3);
		for (index, flight) in flight_list.iter().enumerate().skip(2).take(3) {
			let enter = f64::from(flight.enter_ms);
			let fired = flight_list
				.iter()
				.filter(|other| other.fire_ms <= flight.enter_ms)
				.count();
			let place = f64::from(u32::try_from(index - fired).unwrap());
			let slot = TUBE_STEP.mul_add(-place, TUBE_REST_Y);
			let (node, begin) = drop_list
				.iter()
				.filter(|node| {
					node.attribute("href") == Some(format!("#q{}", flight.lane).as_str())
				})
				.flat_map(|node| {
					drop_begins(*node)
						.into_iter()
						.map(move |begin| (*node, begin))
				})
				.find(|(node, begin)| (drop_at(*node, *begin, enter) - slot).abs() <= 1.0)
				.unwrap();
			assert!(drop_at(node, begin, begin) <= TUBE_TOP - TUBE_RADIUS + 1e-9);
			let free_fall = drop_at(node, begin, begin + 100.0) - drop_at(node, begin, begin);
			assert!(
				(GRAVITY / 2.0 * 100.0).mul_add(-100.0, free_fall).abs() < 0.5,
				"{free_fall}"
			);
			for ms in (0..=800).map(|step| begin + f64::from(step)) {
				let y = drop_at(node, begin, ms);
				let edge = edge_at(&flight_list, ms);
				if ms < enter {
					assert!(y + TUBE_RADIUS <= edge + 0.5, "{index} {ms}");
				} else {
					assert!(y - TUBE_RADIUS >= edge - 0.5, "{index} {ms}");
				}
			}
		}
		for node in drop_list {
			let deep: f64 = node.attribute("y").unwrap().parse().unwrap();
			assert!(deep - TUBE_RADIUS > TUBE_BOTTOM, "{deep}");
		}
	}

	#[test]
	fn tapes_paint_only_their_markers() {
		let flight_list: Vec<Flight> = (0..24_u32)
			.map(|index| {
				flight(
					usize::try_from(index % 3).unwrap(),
					index * 50,
					index * 50 + 900,
				)
			})
			.collect();
		let svg = rendered(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let tape_list: Vec<Node> = document
			.descendants()
			.filter(|node| node.has_attribute("marker-mid"))
			.collect();
		assert_eq!(tape_list.len(), 3);
		for tape in tape_list {
			let painted = |name: &str| {
				tape.ancestors()
					.find_map(|node| node.attribute(name))
					.unwrap_or(if name == "fill" { "black" } else { "none" })
					.to_owned()
			};
			assert_eq!(painted("fill"), "none", "{tape:?}");
			assert_eq!(painted("stroke"), "none", "{tape:?}");
		}
	}

	#[test]
	fn tube_circles_are_jittered_deterministically() {
		let flight_list: Vec<Flight> = (0..24_u32)
			.map(|index| {
				flight(
					usize::try_from(index % 3).unwrap(),
					index * 50,
					index * 50 + 900,
				)
			})
			.collect();
		let svg = rendered(&flight_list);
		assert_eq!(svg, rendered(&flight_list));
		let document = Document::parse(&svg).unwrap();
		let x_list: Vec<f64> = document
			.descendants()
			.filter(|node| node.has_attribute("marker-mid"))
			.flat_map(|tape| vertex_list(tape.attribute("d").unwrap()))
			.map(|vertex| vertex.0)
			.collect();
		assert!(x_list.iter().all(|x| x.abs() <= 1.0), "{x_list:?}");
		assert!(x_list.iter().any(|x| *x > 0.5) && x_list.iter().any(|x| *x < -0.5));
		assert!(x_list.iter().any(|x| x.abs() < 0.5));
	}

	fn clip_box(document: &Document, id: &str) -> (f64, f64) {
		let rect = document
			.descendants()
			.find(|node| node.attribute("id") == Some(id))
			.unwrap()
			.first_element_child()
			.unwrap();
		let top: f64 = rect.attribute("y").unwrap().parse().unwrap();
		let height: f64 = rect.attribute("height").unwrap().parse().unwrap();
		(top, top + height)
	}

	fn second_number(text: &str) -> f64 {
		text.split(' ').nth(1).unwrap().parse().unwrap()
	}

	#[test]
	fn tube_circles_are_whole_below_the_tube_top() {
		let capacity = u32::try_from(tube_capacity()).unwrap();
		let fire_ms = |index: u32| 1_000 + 200 * index;
		let flight_list: Vec<Flight> = (0..capacity + 8)
			.map(|index| {
				let enter = index.checked_sub(capacity).map_or(0, fire_ms);
				flight(usize::try_from(index % 3).unwrap(), enter, fire_ms(index))
			})
			.collect();
		let svg = rendered(&flight_list);
		let document = Document::parse(&svg).unwrap();
		let tube = document
			.descendants()
			.find(|node| node.has_attribute("data-tube"))
			.unwrap();
		let tube_top = command_list(tube.first_element_child().unwrap().attribute("d").unwrap())
			.into_iter()
			.filter(|(letter, _)| *letter == 'M')
			.map(|(_, number_list)| number_list[1])
			.fold(f64::INFINITY, f64::min);
		let (outer_top, outer_bottom) = clip_box(&document, "tb");
		let (inner_top, inner_bottom) = clip_box(&document, "tc");
		let window = document
			.descendants()
			.find(|node| node.attribute("attributeName") == Some("y"))
			.unwrap();
		let step_node = tube
			.descendants()
			.find(|node| node.has_attribute("data-step"))
			.unwrap();
		let step = step_node.first_element_child().unwrap();
		let bounce = tube
			.descendants()
			.find(|node| node.attribute("calcMode") == Some("spline"))
			.unwrap();
		let bounce_list: Vec<f64> = bounce
			.attribute("begin")
			.unwrap()
			.split(';')
			.map(|begin| begin.trim_end_matches('s').parse::<f64>().unwrap() * 1_000.0)
			.collect();
		let lift = second_number(
			bounce
				.attribute("values")
				.unwrap()
				.split(';')
				.next()
				.unwrap(),
		);
		let vertex_y: Vec<f64> = step_node
			.descendants()
			.filter(|node| node.has_attribute("marker-mid"))
			.flat_map(|tape| {
				let mut tape_vertices = vertex_list(tape.attribute("d").unwrap());
				tape_vertices.pop();
				tape_vertices
			})
			.map(|vertex| vertex.1)
			.collect();
		let mut instant_list: Vec<f64> = flight_list
			.iter()
			.flat_map(|flight| [flight.enter_ms, flight.fire_ms])
			.map(f64::from)
			.collect();
		instant_list.sort_by(f64::total_cmp);
		instant_list.dedup();
		let mut lifted = 0;
		for at in instant_list {
			let bouncing = bounce_list.iter().any(|begin| (begin - at).abs() < 0.5);
			let shift = if bouncing { lift } else { 0.0 };
			let edge: f64 = value_at(window, at).parse().unwrap();
			let offset = second_number(&value_at(step, at));
			for local in vertex_y.iter().map(|y| y + offset) {
				let waiting = local - TUBE_RADIUS >= edge - 0.01;
				let fired = local - TUBE_RADIUS >= inner_bottom - 0.01;
				let shown = local + shift;
				if !waiting || fired || shown < tube_top + TUBE_RADIUS - 1e-9 {
					continue;
				}
				assert!(
					shown - TUBE_RADIUS >= outer_top - 1e-9
						&& shown + TUBE_RADIUS <= outer_bottom + 1e-9,
					"{at} {shown}"
				);
				assert!(
					local - TUBE_RADIUS >= inner_top - 1e-9
						&& local + TUBE_RADIUS <= inner_bottom + 1e-9,
					"{at} {local}"
				);
				if bouncing {
					lifted += 1;
				}
			}
		}
		assert!(lifted > 5, "{lifted}");
	}
}
