use roxmltree::Node;

use super::feeder::Flight;
use super::format::float;

pub(super) struct Pool {
	thrust_key_list: Vec<f64>,
	thrust_list: Vec<(f64, f64)>,
	pub(super) resting: (f64, f64),
	pub(super) begin_list: Vec<f64>,
	vanish_list: Vec<f64>,
	hide_ms: f64,
	drop_from: (f64, f64),
	drop_ms: f64,
	spline: [f64; 4],
}

fn clock(text: &str) -> f64 {
	text.strip_suffix("ms").map_or_else(
		|| text.parse::<f64>().unwrap() * 1_000.0,
		|ms| ms.parse().unwrap(),
	)
}

fn pair(text: &str) -> (f64, f64) {
	let mut number_list = text.split(' ').map(|part| part.parse::<f64>().unwrap());
	let x = number_list.next().unwrap();
	(x, number_list.next().unwrap_or(0.0))
}

fn clock_list(node: Node, name: &str) -> Vec<f64> {
	node.attribute(name)
		.unwrap()
		.split(';')
		.map(clock)
		.collect()
}

fn number(node: Node, name: &str) -> f64 {
	node.attribute(name).unwrap().parse().unwrap()
}

fn child<'a>(node: Node<'a, 'a>, tag: &str) -> Node<'a, 'a> {
	node.children()
		.find(|child| child.has_tag_name(tag))
		.unwrap()
}

fn bezier(first: f64, second: f64, part: f64) -> f64 {
	let rest = 1.0 - part;
	3.0f64.mul_add(
		rest * part * rest.mul_add(first, part * second),
		part * part * part,
	)
}

impl Pool {
	pub(super) fn of(flight: Node) -> Self {
		let outer = flight.parent().unwrap();
		let thrust = outer.first_element_child().unwrap();
		let fall = child(flight, "animateMotion");
		let fade = child(flight, "animate");
		assert_eq!(thrust.attribute("type"), Some("translate"));
		assert_eq!(fall.attribute("calcMode"), Some("spline"));
		assert_eq!(fade.attribute("attributeName"), Some("opacity"));
		assert_eq!(fade.attribute("calcMode"), Some("discrete"));
		assert_eq!(fade.attribute("values"), Some("0;1"));
		let duration = clock(thrust.attribute("dur").unwrap());
		let value_list: Vec<(f64, f64)> = fall
			.attribute("values")
			.unwrap()
			.split(';')
			.map(pair)
			.collect();
		assert_eq!(value_list.len(), 2);
		assert_eq!(value_list[1], (0.0, 0.0));
		let resting = (number(flight, "x"), number(flight, "y"));
		let spline: Vec<f64> = fall
			.attribute("keySplines")
			.unwrap()
			.split(' ')
			.map(|control| control.parse().unwrap())
			.collect();
		Self {
			thrust_key_list: thrust
				.attribute("keyTimes")
				.unwrap()
				.split(';')
				.map(|key| key.parse::<f64>().unwrap() * duration)
				.collect(),
			thrust_list: thrust
				.attribute("values")
				.unwrap()
				.split(';')
				.map(pair)
				.collect(),
			resting,
			begin_list: clock_list(fall, "begin"),
			vanish_list: clock_list(fade, "begin"),
			hide_ms: clock(fade.attribute("dur").unwrap()) / 2.0,
			drop_from: (resting.0 + value_list[0].0, resting.1 + value_list[0].1),
			drop_ms: clock(fall.attribute("dur").unwrap()),
			spline: [spline[0], spline[1], spline[2], spline[3]],
		}
	}

	fn thrust(&self, at: f64) -> (f64, f64) {
		let index = self
			.thrust_key_list
			.iter()
			.rposition(|key| *key <= at)
			.unwrap();
		let Some(next) = self.thrust_key_list.get(index + 1) else {
			return self.thrust_list[index];
		};
		let part = (at - self.thrust_key_list[index]) / (next - self.thrust_key_list[index]);
		let (from, to) = (self.thrust_list[index], self.thrust_list[index + 1]);
		(
			(to.0 - from.0).mul_add(part, from.0),
			(to.1 - from.1).mul_add(part, from.1),
		)
	}

	fn progress(&self, part: f64) -> f64 {
		let [x1, y1, x2, y2] = self.spline;
		let (mut low, mut high) = (0.0, 1.0);
		for _ in 0..60 {
			let middle = f64::midpoint(low, high);
			if bezier(x1, x2, middle) < part {
				low = middle;
			} else {
				high = middle;
			}
		}
		bezier(y1, y2, f64::midpoint(low, high))
	}

	pub(super) fn drop_at(&self, flown: f64) -> (f64, f64) {
		let part = self.progress((flown / self.drop_ms).clamp(0.0, 1.0));
		(
			(self.resting.0 - self.drop_from.0).mul_add(part, self.drop_from.0),
			(self.resting.1 - self.drop_from.1).mul_add(part, self.drop_from.1),
		)
	}

	pub(super) fn gravity(&self) -> f64 {
		2.0 * (self.resting.1 - self.drop_from.1) / (self.drop_ms * self.drop_ms)
	}

	pub(super) fn drop_ms(&self) -> f64 {
		self.drop_ms
	}

	pub(super) fn at(&self, ms: f64, begin: f64) -> (f64, f64) {
		let thrust = self.thrust(ms);
		let fall = self.drop_at(ms - begin);
		(thrust.0 + fall.0, thrust.1 + fall.1)
	}

	pub(super) fn static_at(&self) -> (f64, f64) {
		let thrust = self.thrust_list[self.thrust_list.len() - 1];
		(thrust.0 + self.resting.0, thrust.1 + self.resting.1)
	}

	pub(super) fn shown(&self, ms: f64) -> bool {
		self.vanish_list
			.iter()
			.all(|vanish| ms < *vanish || ms >= vanish + self.hide_ms)
	}

	pub(super) fn reuse_gaps(&self) -> Vec<f64> {
		self.vanish_list
			.iter()
			.zip(self.begin_list.iter().skip(1))
			.map(|(vanish, next)| next - vanish - self.hide_ms)
			.collect()
	}

	pub(super) fn vanish(&self, begin: f64) -> f64 {
		self.vanish_list
			.iter()
			.copied()
			.find(|vanish| *vanish > begin)
			.unwrap()
	}

	fn thrown(&self, begin: f64) -> f64 {
		self.thrust_key_list
			.iter()
			.zip(&self.thrust_list)
			.filter(|(key, value)| **key <= begin + 1.0 && **value == (0.0, 0.0))
			.map(|(key, _)| key.max(begin))
			.fold(begin, f64::max)
	}

	pub(super) fn samples(&self, begin: f64, count: u32) -> Vec<(f64, (f64, f64))> {
		let thrown = self.thrown(begin);
		let span = self.vanish(begin) - thrown;
		(0..=count)
			.map(|step| {
				let ms = span.mul_add(float(step) / float(count), thrown);
				(ms, self.at(ms, begin))
			})
			.collect()
	}

	pub(super) fn reaching(&self, begin: f64, y: f64) -> f64 {
		let apex = self
			.samples(begin, 400)
			.into_iter()
			.min_by(|left, right| (left.1).1.total_cmp(&(right.1).1))
			.unwrap()
			.0;
		let (mut low, mut high) = (apex, self.vanish(begin));
		for _ in 0..60 {
			let middle = f64::midpoint(low, high);
			if self.at(middle, begin).1 < y {
				low = middle;
			} else {
				high = middle;
			}
		}
		high
	}
}

pub(super) fn off_line(point_list: &[(f64, f64)]) -> f64 {
	let (first, last) = (point_list[0], point_list[point_list.len() - 1]);
	let slope = (last.1 - first.1) / (last.0 - first.0);
	point_list
		.iter()
		.map(|(at, value)| (slope.mul_add(at - first.0, first.1) - value).abs())
		.fold(0.0, f64::max)
}

pub(super) fn aimed_flight(lane: usize, enter_ms: u32, fire_ms: u32, landing_y: f64) -> Flight {
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

pub(super) fn test_flight(lane: usize, enter_ms: u32, fire_ms: u32) -> Flight {
	aimed_flight(lane, enter_ms, fire_ms, 250.0)
}

pub(super) fn wrapped(svg: &str) -> String {
	format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{svg}</svg>")
}

pub(super) fn value_at(animation: Node, ms: f64) -> String {
	let duration = clock(animation.attribute("dur").unwrap());
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

fn animation_keys(animation: Node) -> Vec<f64> {
	let duration = clock(animation.attribute("dur").unwrap());
	animation
		.attribute("keyTimes")
		.unwrap()
		.split(';')
		.map(|key| key.parse::<f64>().unwrap() * duration)
		.collect()
}

pub(super) fn linear_at(animation: Node, ms: f64) -> f64 {
	let key_list = animation_keys(animation);
	let value_list: Vec<f64> = animation
		.attribute("values")
		.unwrap()
		.split(';')
		.map(|value| value.parse().unwrap())
		.collect();
	let index = key_list.iter().rposition(|key| *key <= ms).unwrap();
	let Some(next) = key_list.get(index + 1) else {
		return value_list[index];
	};
	let part = (ms - key_list[index]) / (next - key_list[index]);
	(value_list[index + 1] - value_list[index]).mul_add(part, value_list[index])
}

pub(super) fn uniform_at(animation: Node, ms: f64) -> String {
	let begin = clock(animation.attribute("begin").unwrap());
	let duration = clock(animation.attribute("dur").unwrap());
	let value_list: Vec<&str> = animation.attribute("values").unwrap().split(';').collect();
	let slot = duration / float(value_list.len());
	let index = (0..value_list.len())
		.take_while(|position| slot.mul_add(float(*position), begin) <= ms)
		.last()
		.unwrap_or(0);
	value_list[index].to_owned()
}

pub(super) fn vertex_list(path: &str) -> Vec<(f64, f64)> {
	let spaced: String = path
		.chars()
		.flat_map(|letter| match letter {
			'M' | 'v' | 'l' | 'h' => vec![' ', letter, ' '],
			'-' => vec![' ', '-'],
			_ => vec![letter],
		})
		.collect();
	let mut part_list = spaced.split_whitespace();
	let mut vertex_list = Vec::new();
	let mut at = (0.0, 0.0);
	let mut command = "M";
	while let Some(part) = part_list.next() {
		let number = |text: &str| text.parse::<f64>().unwrap();
		match part {
			"M" | "v" | "l" | "h" => command = part,
			_ => {
				let first = number(part);
				at = match command {
					"M" => (first, number(part_list.next().unwrap())),
					"l" => (at.0 + first, at.1 + number(part_list.next().unwrap())),
					"v" => (at.0, at.1 + first),
					_ => (at.0 + first, at.1),
				};
				vertex_list.push(at);
			}
		}
	}
	vertex_list
}

pub(super) fn drop_at(node: Node, begin: f64, ms: f64) -> f64 {
	let motion = node.first_element_child().unwrap();
	let duration = clock(motion.attribute("dur").unwrap());
	let lift = pair(
		motion
			.attribute("values")
			.unwrap()
			.split(';')
			.next()
			.unwrap(),
	)
	.1;
	let deep = number(node, "y");
	let part = ((ms - begin) / duration).clamp(0.0, 1.0);
	(-lift).mul_add(part * part, deep + lift)
}

pub(super) fn drop_begins(node: Node) -> Vec<f64> {
	clock_list(node.first_element_child().unwrap(), "begin")
}

pub(super) fn command_list(path: &str) -> Vec<(char, Vec<f64>)> {
	let spaced: String = path
		.chars()
		.flat_map(|letter| {
			if letter.is_ascii_alphabetic() {
				vec![' ', letter, ' ']
			} else {
				vec![letter]
			}
		})
		.collect();
	let mut command_list: Vec<(char, Vec<f64>)> = Vec::new();
	for part in spaced.split_whitespace() {
		match part.parse::<f64>() {
			Ok(value) => command_list.last_mut().unwrap().1.push(value),
			Err(_) => command_list.push((part.chars().next().unwrap(), Vec::new())),
		}
	}
	command_list
}

pub(super) fn point_list(number_list: &[f64]) -> Vec<(f64, f64)> {
	number_list
		.chunks(2)
		.map(|pair| (pair[0], pair[1]))
		.collect()
}

fn cubic(from: f64, first: f64, second: f64, to: f64, part: f64) -> f64 {
	let rest = 1.0 - part;
	(rest * rest * rest).mul_add(
		from,
		3.0f64.mul_add(
			rest * part * rest.mul_add(first, part * second),
			part * part * part * to,
		),
	)
}

pub(super) fn path_samples(path: &str, steps: u32) -> Vec<(f64, f64)> {
	let mut at = (0.0, 0.0);
	let mut sample_list = Vec::new();
	for (letter, number_list) in command_list(path) {
		let point_list = point_list(&number_list);
		let part_list = (1..=steps).map(|step| float(step) / float(steps));
		match letter {
			'M' => {
				at = point_list[0];
				sample_list.push(at);
			}
			'L' => {
				for to in point_list {
					let from = at;
					sample_list.extend(part_list.clone().map(|part| {
						(
							(to.0 - from.0).mul_add(part, from.0),
							(to.1 - from.1).mul_add(part, from.1),
						)
					}));
					at = to;
				}
			}
			'C' => {
				let [first, second, to] = [point_list[0], point_list[1], point_list[2]];
				let from = at;
				sample_list.extend(part_list.map(|part| {
					(
						cubic(from.0, first.0, second.0, to.0, part),
						cubic(from.1, first.1, second.1, to.1, part),
					)
				}));
				at = to;
			}
			other => panic!("unsupported path command {other}"),
		}
	}
	sample_list
}
