use super::format::{float, num};

#[derive(Clone, Copy)]
pub(super) struct Row {
	pub(super) clip: &'static str,
	pub(super) class: &'static str,
	pub(super) y: f64,
	pub(super) step: f64,
}

pub(super) fn ticker(state_list: Vec<String>, row: Row, duration: f64) -> String {
	let mut unique: Vec<String> = Vec::new();
	let index_list: Vec<usize> = state_list
		.into_iter()
		.map(|state| {
			unique
				.iter()
				.position(|known| *known == state)
				.unwrap_or_else(|| {
					unique.push(state);
					unique.len() - 1
				})
		})
		.collect();
	let offset = |index: usize| num(-row.step * float(index), 1);
	let value_list: Vec<String> = index_list
		.iter()
		.map(|index| format!("0 {}", offset(*index)))
		.collect();
	let text_list: Vec<String> = unique
		.iter()
		.enumerate()
		.map(|(index, text)| {
			format!(
				"<text class=\"{}\" y=\"{}\">{text}</text>",
				row.class,
				num(row.step.mul_add(float(index), row.y), 1)
			)
		})
		.collect();
	format!(
		concat!(
			"<g clip-path=\"url(#{clip})\"><g transform=\"translate(0 {rest})\">",
			"<animateTransform attributeName=\"transform\" type=\"translate\" calcMode=\"discrete\" values=\"{values}\" dur=\"{dur}ms\" fill=\"freeze\"/>",
			"{texts}</g></g>",
		),
		clip = row.clip,
		rest = offset(index_list.last().copied().unwrap_or(0)),
		values = value_list.join(";"),
		dur = num(duration, 0),
		texts = text_list.concat(),
	)
}
