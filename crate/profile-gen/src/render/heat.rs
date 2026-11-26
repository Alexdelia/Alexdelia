use super::feeder::{LEFT_PRONG_X, POUCH_X, POUCH_Y, PRONG_Y, RIGHT_PRONG_X, RING, SLING_X};
use super::format::{float, key_time, num, whole_ms};
use super::schedule::{HEAT_MAX, HEAT_STEP_MS};

const GLOW_FROM: u16 = 200;
const SHAKE_FROM: u16 = 350;
const SPARK_FROM: u16 = 500;
const FLAME_FROM: u16 = 750;
const BOLT_FROM: u16 = 900;
const COOL_PER_STEP: u16 = 84;
const FADE_TOLERANCE: f64 = 0.03;
const SHAKE_PX: f64 = 2.6;
const JIGGLE_PX: f64 = 1.0;
const SPARK_TURN: f64 = 80.0;
const FLAME_SWAY: f64 = 24.0;
const BOLT_TURN_LIST: [&str; 6] = ["0", "180", "8", "-8", "172", "188"];
const EMBER_PEAK: f64 = 0.85;
const EMBER_HALO_RADIUS: f64 = 9.0;
const EMBER_HALO_WIDTH: f64 = 5.0;
const EMBER_RING_RADIUS: f64 = 6.3;
const EMBER_RING_WIDTH: f64 = 2.6;
pub(super) const EMBER_EXTENT: f64 = EMBER_HALO_RADIUS + EMBER_HALO_WIDTH / 2.0;
const GLOW: &str = "#ff7b00";
const WARM: (u8, u8, u8) = (255, 214, 150);
const HOT: (u8, u8, u8) = (255, 96, 32);
const SPARK_COUNT: usize = 9;
const TRACER_COUNT: usize = 3;
const CALM: &str = "0";

pub(super) struct Heat {
	pub(super) start_ms: u32,
	pub(super) sample_list: Vec<u16>,
}

pub(super) struct Burn {
	start_ms: u32,
	sample_list: Vec<u16>,
	duration: f64,
}

pub(super) fn hash(index: usize, salt: u64) -> u64 {
	let mut mixed = u64::try_from(index)
		.unwrap_or(u64::MAX)
		.wrapping_add(salt.wrapping_mul(0x9e37_79b9_7f4a_7c15));
	mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	mixed ^ (mixed >> 31)
}

fn seeded(index: usize, salt: u64) -> f64 {
	f64::from(u32::try_from(hash(index, salt) >> 32).unwrap_or(0)) / 4_294_967_296.0
}

fn level(heat: u16, from: u16) -> f64 {
	(f64::from(heat.saturating_sub(from)) / (float(HEAT_MAX) - f64::from(from))).min(1.0)
}

fn channel(from: u8, to: u8, part: f64) -> u8 {
	let value = (f64::from(to) - f64::from(from)).mul_add(part, f64::from(from));
	u8::try_from(whole_ms(value)).unwrap_or(u8::MAX)
}

fn shade(part: f64) -> String {
	format!(
		"#{:02x}{:02x}{:02x}",
		channel(WARM.0, HOT.0, part),
		channel(WARM.1, HOT.1, part),
		channel(WARM.2, HOT.2, part)
	)
}

fn is_edge(level_list: &[f64], index: usize) -> bool {
	let lit = |at: Option<usize>| {
		at.and_then(|at| level_list.get(at))
			.is_some_and(|level| *level > 0.0)
	};
	level_list[index] <= 0.0 && (lit(index.checked_sub(1)) || lit(Some(index + 1)))
}

fn fits_line(level_list: &[f64], from: usize, to: usize) -> bool {
	let span = float(to - from);
	(from + 1..to).all(|inner| {
		let part = float(inner - from) / span;
		let line = (level_list[to] - level_list[from]).mul_add(part, level_list[from]);
		(line - level_list[inner]).abs() <= FADE_TOLERANCE
	})
}

fn simplified(level_list: &[f64]) -> Vec<usize> {
	let last = level_list.len() - 1;
	let pinned: Vec<usize> = (0..=last)
		.filter(|index| *index == 0 || *index == last || is_edge(level_list, *index))
		.collect();
	let mut kept = vec![0];
	for pair in pinned.windows(2) {
		let mut anchor = pair[0];
		for candidate in anchor + 2..=pair[1] {
			if !fits_line(level_list, anchor, candidate) {
				anchor = candidate - 1;
				kept.push(anchor);
			}
		}
		kept.push(pair[1]);
	}
	kept
}

fn tongue(x: f64, half_width: f64, height: f64) -> String {
	format!(
		"<path d=\"M{} 0Q{} {} {} 0Z\"/>",
		num(x - half_width, 1),
		num(x, 1),
		num(-2.0 * height, 1),
		num(x + half_width, 1)
	)
}

fn spark_at(index: usize, salt: u64) -> (f64, f64) {
	let angle = seeded(index, salt).mul_add(130.0, -100.0).to_radians();
	let distance = seeded(index, salt + 1).mul_add(22.0, 8.0);
	(angle.cos() * distance, angle.sin() * distance)
}

impl Heat {
	pub(super) fn burn(&self, duration: f64) -> Burn {
		let fits = |count: usize| {
			float(count).mul_add(float(HEAT_STEP_MS), f64::from(self.start_ms)) < duration
		};
		let mut sample_list: Vec<u16> = self
			.sample_list
			.iter()
			.enumerate()
			.take_while(|(index, _)| fits(*index))
			.map(|(_, heat)| *heat)
			.collect();
		while let Some(last) = sample_list.last().copied().filter(|last| *last > 0)
			&& fits(sample_list.len())
		{
			sample_list.push(last.saturating_sub(COOL_PER_STEP));
		}
		if let Some(last) = sample_list.last_mut() {
			*last = 0;
		}
		Burn {
			start_ms: self.start_ms,
			sample_list,
			duration,
		}
	}
}

impl Burn {
	fn ms(&self, index: usize) -> f64 {
		float(index).mul_add(float(HEAT_STEP_MS), f64::from(self.start_ms))
	}

	fn level_list(&self, from: u16) -> Vec<f64> {
		self.sample_list
			.iter()
			.map(|heat| level(*heat, from))
			.collect()
	}

	fn track(&self, attribute: &str, from: u16, value: impl Fn(f64) -> String) -> String {
		let level_list = self.level_list(from);
		if level_list.iter().all(|level| *level <= 0.0) {
			return String::new();
		}
		let mut point_list = vec![(0.0, value(0.0))];
		point_list.extend(
			simplified(&level_list)
				.into_iter()
				.map(|index| (self.ms(index), value(level_list[index]))),
		);
		point_list.push((self.duration, value(0.0)));
		let value_list: Vec<&str> = point_list.iter().map(|(_, value)| value.as_str()).collect();
		let key_list: Vec<String> = point_list
			.iter()
			.map(|(ms, _)| key_time(*ms, self.duration))
			.collect();
		format!(
			"<animate attributeName=\"{attribute}\" values=\"{}\" keyTimes=\"{}\" dur=\"{}ms\" fill=\"freeze\"/>",
			value_list.join(";"),
			key_list.join(";"),
			num(self.duration, 0),
		)
	}

	fn fade(&self, from: u16, peak: f64) -> String {
		self.track("opacity", from, |level| num(level * peak, 2))
	}

	fn flicker(&self, kind: &str, from: u16, value: impl Fn(usize, f64) -> String) -> String {
		let value_list: Vec<String> = self
			.level_list(from)
			.into_iter()
			.enumerate()
			.map(|(index, level)| {
				if level > 0.0 {
					value(index, level)
				} else {
					CALM.to_owned()
				}
			})
			.collect();
		if value_list.iter().all(|value| value == CALM) {
			return String::new();
		}
		format!(
			"<animateTransform attributeName=\"transform\" type=\"{kind}\" calcMode=\"discrete\" values=\"{}\" begin=\"{}ms\" dur=\"{}ms\" fill=\"freeze\"/>",
			value_list.join(";"),
			self.start_ms,
			num(float(value_list.len()) * float(HEAT_STEP_MS), 0),
		)
	}

	fn sway(&self, from: u16, salt: u64, turn: f64) -> String {
		self.flicker("rotate", from, |index, _| {
			num((seeded(index, salt) - 0.5) * turn, 0)
		})
	}

	pub(super) fn defs(&self) -> String {
		format!(
			concat!(
				"<g id=\"e\" opacity=\"0\" fill=\"none\" stroke=\"{glow}\">{fade}",
				"<circle r=\"{halo_r}\" stroke-width=\"{halo_w}\" stroke-opacity=\".3\"/>",
				"<circle r=\"{ring_r}\" stroke-width=\"{ring_w}\" stroke=\"#ffb347\"/>",
				"</g>",
			),
			glow = GLOW,
			halo_r = num(EMBER_HALO_RADIUS, 1),
			halo_w = num(EMBER_HALO_WIDTH, 1),
			ring_r = num(EMBER_RING_RADIUS, 1),
			ring_w = num(EMBER_RING_WIDTH, 1),
			fade = self.fade(SPARK_FROM, EMBER_PEAK),
		)
	}

	pub(super) fn shake(&self) -> String {
		self.flicker("translate", SHAKE_FROM, |index, level| {
			let amplitude = SHAKE_PX * level;
			format!(
				"{} {}",
				num((seeded(index, 1) - 0.5) * amplitude, 1),
				num((seeded(index, 2) - 0.5) * amplitude, 1)
			)
		})
	}

	pub(super) fn jiggle(&self) -> String {
		self.flicker("translate", 0, |index, level| {
			let side = if index % 2 == 0 { 1.0 } else { -1.0 };
			num(
				side * JIGGLE_PX * level * seeded(index, 3).mul_add(0.5, 0.5),
				1,
			)
		})
	}

	pub(super) fn glow(&self) -> String {
		format!(
			concat!(
				"<g data-glow=\"\" opacity=\"0\" fill=\"none\" stroke=\"{glow}\" stroke-linecap=\"round\">{fade}",
				"<use href=\"#fk\" stroke-width=\"13\" stroke-opacity=\".18\"/>",
				"<use href=\"#fk\" stroke-width=\"9\" stroke-opacity=\".35\"/>",
				"</g>",
			),
			glow = GLOW,
			fade = self.fade(GLOW_FROM, 1.0),
		)
	}

	pub(super) fn band_stroke(&self, muted: &str) -> String {
		self.track("stroke", SPARK_FROM, |level| {
			if level > 0.0 {
				shade(level)
			} else {
				muted.to_owned()
			}
		})
	}

	pub(super) fn flames(&self) -> String {
		let prong = |x: f64, salt: u64| {
			format!(
				concat!(
					"<g transform=\"translate({} {})\"><g>{}",
					"<g fill=\"#ff6a00\" fill-opacity=\".75\">{}{}{}</g>",
					"<g fill=\"#ffd23f\">{}{}</g>",
					"</g></g>",
				),
				num(x, 1),
				num(PRONG_Y - RING, 1),
				self.sway(FLAME_FROM, salt, FLAME_SWAY),
				tongue(-3.0, 3.0, 9.0),
				tongue(1.0, 3.5, 13.0),
				tongue(4.0, 2.5, 8.0),
				tongue(-2.5, 1.5, 4.5),
				tongue(1.0, 2.0, 7.0),
			)
		};
		format!(
			"<g data-flames=\"\" opacity=\"0\">{}{}{}</g>",
			self.fade(FLAME_FROM, 1.0),
			prong(LEFT_PRONG_X, 5),
			prong(RIGHT_PRONG_X, 6),
		)
	}

	pub(super) fn bolt(&self) -> String {
		let reach = RIGHT_PRONG_X - SLING_X;
		let path = format!(
			"M{} 0L{} -5 {} 3 0 -4 {} 4 {} -3 {} 0",
			num(-reach, 1),
			num(-reach * 0.7, 1),
			num(-reach * 0.3, 1),
			num(reach * 0.35, 1),
			num(reach * 0.7, 1),
			num(reach, 1),
		);
		let turn = self.flicker("rotate", BOLT_FROM, |index, _| {
			let choice = usize::try_from(hash(index, 7) % 6).unwrap_or(0);
			BOLT_TURN_LIST[choice].to_owned()
		});
		format!(
			concat!(
				"<g data-bolt=\"\" opacity=\"0\" transform=\"translate({x} {y})\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{fade}",
				"<g>{turn}<path d=\"{path}\" stroke=\"#58a6ff\" stroke-width=\"3\" stroke-opacity=\".45\"/>",
				"<path d=\"{path}\" stroke=\"#a5d6ff\" stroke-width=\"1.2\"/></g>",
				"</g>",
			),
			x = num(SLING_X, 1),
			y = num(PRONG_Y, 1),
			fade = self.fade(BOLT_FROM, 1.0),
			turn = turn,
			path = path,
		)
	}

	pub(super) fn sparks(&self) -> String {
		let dot_list: Vec<String> = (0..SPARK_COUNT)
			.map(|index| {
				let (x, y) = spark_at(index, 10);
				let color = if index % 2 == 0 { "#ffd700" } else { GLOW };
				format!(
					"<rect x=\"{}\" y=\"{}\" width=\"1.6\" height=\"1.6\" fill=\"{color}\"/>",
					num(x, 1),
					num(y, 1)
				)
			})
			.collect();
		let tracer_list: Vec<String> = (0..TRACER_COUNT)
			.map(|index| {
				let (x, y) = spark_at(index, 20);
				format!(
					"<path d=\"M{} {}l{} {}\"/>",
					num(x, 1),
					num(y, 1),
					num(x * 0.4, 1),
					num(y * 0.4, 1)
				)
			})
			.collect();
		format!(
			concat!(
				"<g data-sparks=\"\" opacity=\"0\" transform=\"translate({x} {y})\">{fade}",
				"<g>{turn}{dots}<g stroke=\"#ffb347\" stroke-width=\"1.2\" stroke-linecap=\"round\">{tracers}</g></g>",
				"</g>",
			),
			x = num(POUCH_X, 1),
			y = num(POUCH_Y, 1),
			fade = self.fade(SPARK_FROM, 1.0),
			turn = self.sway(SPARK_FROM, 4, SPARK_TURN),
			dots = dot_list.concat(),
			tracers = tracer_list.concat(),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::super::probe::{linear_at, uniform_at, wrapped};
	use super::*;
	use roxmltree::{Document, Node};

	const DURATION: f64 = 10_000.0;
	const START_MS: u32 = 1_000;
	const MUTED: &str = "#123456";

	fn ramp() -> Vec<u16> {
		let up = (0..=20).map(|step| step * 50);
		let down = (0..=20).rev().map(|step| step * 50);
		up.chain([1_000; 40]).chain(down).chain([0; 10]).collect()
	}

	fn sample_ms(index: usize) -> f64 {
		50.0f64.mul_add(
			f64::from(u32::try_from(index).unwrap()),
			f64::from(START_MS),
		)
	}

	fn burn(sample_list: Vec<u16>) -> Burn {
		Heat {
			start_ms: START_MS,
			sample_list,
		}
		.burn(DURATION)
	}

	fn effects(burn: &Burn) -> String {
		wrapped(
			&[
				format!("<defs>{}</defs>", burn.defs()),
				format!("<g data-rig=\"\">{}</g>", burn.shake()),
				format!("<g data-jiggle=\"\">{}</g>", burn.jiggle()),
				burn.glow(),
				format!("<path data-band=\"\">{}</path>", burn.band_stroke(MUTED)),
				burn.flames(),
				burn.bolt(),
				burn.sparks(),
			]
			.concat(),
		)
	}

	fn effect<'a>(document: &'a Document, key: &str) -> Node<'a, 'a> {
		document
			.descendants()
			.find(|node| node.has_attribute(key) || node.attribute("id") == Some(key))
			.unwrap()
	}

	fn fade_of<'a>(node: Node<'a, 'a>) -> Node<'a, 'a> {
		node.children()
			.find(|child| child.attribute("attributeName") == Some("opacity"))
			.unwrap()
	}

	fn flicker_of<'a>(node: Node<'a, 'a>) -> Node<'a, 'a> {
		node.descendants()
			.find(|child| child.has_tag_name("animateTransform"))
			.unwrap()
	}

	fn stroke_brackets(band: Node, ms: f64) -> (String, String) {
		let animation = band.first_element_child().unwrap();
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
		let on_key = (key_list[index] - ms).abs() <= 0.01;
		let next = if on_key {
			index
		} else {
			(index + 1).min(value_list.len() - 1)
		};
		(value_list[index].to_owned(), value_list[next].to_owned())
	}

	#[test]
	fn heat_drives_effects() {
		let svg = effects(&burn(ramp()));
		let document = Document::parse(&svg).unwrap();
		let threshold_list = [
			("data-glow", 200),
			("data-sparks", 500),
			("e", 500),
			("data-flames", 750),
			("data-bolt", 900),
		];
		let flicker_list = [
			("data-rig", 350),
			("data-jiggle", 0),
			("data-sparks", 500),
			("data-flames", 750),
			("data-bolt", 900),
		];
		let mut moved = vec![0; flicker_list.len()];
		for (index, heat) in ramp().into_iter().enumerate() {
			let ms = sample_ms(index);
			for (key, from) in threshold_list {
				let opacity = linear_at(fade_of(effect(&document, key)), ms);
				if heat <= from {
					assert!(opacity.abs() < 1e-9, "{key} {heat} {opacity}");
				}
				if heat == 1_000 {
					assert!(opacity > 0.3, "{key} {heat} {opacity}");
				}
			}
			for ((key, from), count) in flicker_list.iter().zip(&mut moved) {
				let value = uniform_at(flicker_of(effect(&document, key)), ms + 25.0);
				if heat <= *from {
					assert_eq!(value, "0", "{key} {heat}");
				}
				if heat == 1_000 && value != "0" {
					*count += 1;
				}
			}
			let (before, after) = stroke_brackets(effect(&document, "data-band"), ms);
			if heat <= 500 {
				assert_eq!((before.as_str(), after.as_str()), (MUTED, MUTED), "{heat}");
			}
			if heat == 1_000 {
				assert!([before, after].iter().any(|value| value != MUTED), "{heat}");
			}
		}
		assert!(moved.iter().all(|count| *count > 20), "{moved:?}");
	}

	#[test]
	fn effects_end_calm() {
		let mut hot_to_the_end = ramp();
		hot_to_the_end.truncate(50);
		for sample_list in [ramp(), hot_to_the_end, vec![1_000; 400]] {
			let burn = burn(sample_list);
			assert_eq!(burn.sample_list.last(), Some(&0));
			let last_ms = sample_ms(burn.sample_list.len() - 1);
			assert!(last_ms < DURATION, "{last_ms}");
			let svg = effects(&burn);
			let document = Document::parse(&svg).unwrap();
			assert!(
				effect(&document, "data-rig")
					.attribute("transform")
					.is_none()
			);
			for key in ["data-glow", "data-sparks", "e", "data-flames", "data-bolt"] {
				let node = effect(&document, key);
				assert_eq!(node.attribute("opacity"), Some("0"), "{key}");
				let fade = fade_of(node);
				assert_eq!(
					fade.attribute("values").unwrap().rsplit(';').next(),
					Some("0")
				);
				assert!(linear_at(fade, DURATION).abs() < 1e-9, "{key}");
			}
			for node in document.descendants() {
				if node.has_tag_name("animateTransform") {
					assert_eq!(
						node.attribute("values").unwrap().rsplit(';').next(),
						Some("0")
					);
					assert_eq!(node.attribute("fill"), Some("freeze"));
				}
			}
			let band = effect(&document, "data-band")
				.first_element_child()
				.unwrap();
			assert_eq!(
				band.attribute("values").unwrap().rsplit(';').next(),
				Some(MUTED)
			);
		}
	}

	#[test]
	fn effects_are_deterministic() {
		let first = effects(&burn(ramp()));
		assert_eq!(first, effects(&burn(ramp())));
		assert!(first.is_ascii());
		let shake = |offset: usize| {
			let mut sample_list = vec![0; offset];
			sample_list.extend([1_000; 60]);
			let svg = effects(&burn(sample_list));
			let document = Document::parse(&svg).unwrap();
			let animation = flicker_of(effect(&document, "data-rig"));
			let value_list: Vec<String> = animation
				.attribute("values")
				.unwrap()
				.split(';')
				.skip(offset)
				.take(50)
				.map(str::to_owned)
				.collect();
			value_list
		};
		assert_ne!(shake(0), shake(7));
		assert_ne!(hash(1, 1), hash(2, 1));
		assert_ne!(hash(1, 1), hash(1, 2));
	}

	#[test]
	fn quiet_heat_adds_no_animation() {
		for (sample_list, jiggles) in [(Vec::new(), 0), (vec![0; 30], 0), (vec![150; 30], 1)] {
			let svg = effects(&burn(sample_list));
			assert!(!svg.contains("<animate attributeName=\"opacity\""), "{svg}");
			assert!(!svg.contains("attributeName=\"stroke\""), "{svg}");
			assert_eq!(svg.matches("<animateTransform").count(), jiggles, "{svg}");
			let document = Document::parse(&svg).unwrap();
			for key in ["data-glow", "data-sparks", "e", "data-flames", "data-bolt"] {
				assert_eq!(effect(&document, key).attribute("opacity"), Some("0"));
			}
		}
	}
}
