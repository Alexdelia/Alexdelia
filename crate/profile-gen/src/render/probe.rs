use roxmltree::Node;

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
