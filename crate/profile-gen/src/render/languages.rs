use super::clock::{Clock, race_ms};
use super::crossing::{Track, close_runs};
use super::feed::{Feed, Shown, feed_list, growth, paired, shown_list, throw_list};
use super::feeder::{
	FLIGHT_RADIUS, Flight, POUCH_X, absorbed_ms, feeder, flight_ms, flights, fork, throw_ms,
};
use super::format::{
	animate, animate_transform, arrow, compact, discrete, escape, float, key_time, month, num,
	percent, totals_label, trend_word, whole_ms,
};
use super::heat::Heat;
use super::ranking::{Cell, cell_at, grid, settle};
use super::schedule::{Shot, schedule};
use super::ticker::{Row, ticker};
use super::tube::tube_capacity;
use crate::icons::icon_path;
use crate::language::Language;
use crate::theme::{FONT_STACK, Palette, Variant, palette};
use crate::timeline::{Frame, LanguageSummary, Timeline};

pub(super) const WIDTH: f64 = 880.0;
const HEIGHT: f64 = 432.0;
pub(super) const GROUND: f64 = 300.0;
const CHART_HEIGHT: f64 = 196.0;
const MIN_BAR_HEIGHT: f64 = 2.0;
const GUTTER: f64 = 60.0;
const RIGHT_MARGIN: f64 = 8.0;
const USABLE_WIDTH: f64 = WIDTH - GUTTER - RIGHT_MARGIN;
const MAX_SLOT: f64 = 64.0;
const BAR_RATIO: f64 = 0.72;
const TOP_RADIUS: f64 = 2.0;
const LABEL_PAD: f64 = 6.0;
const HEADER_GAP: f64 = 4.0;
const HEADER_LIST: [&str; 5] = ["share", "lines", "repos", "since", "past year"];
const GLYPH_EM: f64 = 0.66;
const THIN_EM: f64 = 0.31;
const NARROW_EM: f64 = 0.46;
const BROAD_EM: f64 = 0.82;
const WIDE_EM: f64 = 1.0;
const NAME_FONT_PX: f64 = 10.0;

pub(super) const TOKEN_RADIUS: f64 = 10.0;
const DARK_INK_THRESHOLD: u32 = 150_000;
const GROUND_DRAW_MS: f64 = 600.0;
const FADE_MS: f64 = 250.0;
const LEGEND_DIP: f64 = 0.1;
const DIP_BRIDGE_TICKS: usize = 4;
const CONTACT_MARGIN: f64 = 2.0;

const TOKEN_ROW: f64 = GROUND + 24.0;
const NAME_ROW: f64 = GROUND + 52.0;
const SHARE_ROW: f64 = GROUND + 68.0;
const LINES_ROW: f64 = GROUND + 82.0;
const REPOS_ROW: f64 = GROUND + 96.0;
const SINCE_ROW: f64 = GROUND + 110.0;
const TREND_ROW: f64 = GROUND + 124.0;
const HEADER_ROW_LIST: [f64; 5] = [SHARE_ROW, LINES_ROW, REPOS_ROW, SINCE_ROW, TREND_ROW];
const SHARE_STEP: f64 = 18.0;
const ROW_STEP: f64 = 16.0;
const DATE_X: f64 = WIDTH - 8.0;
const DATE_ROW: f64 = 72.0;
const DATE_STEP: f64 = 20.0;
const TOTALS_ROW: f64 = DATE_ROW + 18.0;
const TOTALS_STEP: f64 = 16.0;

#[must_use]
pub fn render_languages(timeline: &Timeline, variant: Variant, seconds_per_year: f64) -> String {
	let colors = palette(variant);
	let race = Race::new(timeline, seconds_per_year);
	[
		header(timeline, &race, colors),
		defs(&race, colors),
		ground(colors),
		headers(&race),
		feeder(&race.flight_list, &race.heat, race.clock.duration(), colors),
		race.lanes
			.iter()
			.map(|lane| column(lane, &race))
			.collect::<Vec<_>>()
			.concat(),
		flights(&race.flight_list, race.clock.duration()),
		fork(&race.heat, race.clock.duration()),
		date_and_totals(timeline, &race.clock),
		"</svg>".to_owned(),
	]
	.concat()
}

fn name_width(name: &str) -> f64 {
	name.chars().map(glyph_em).sum::<f64>() * NAME_FONT_PX
}

const fn glyph_em(glyph: char) -> f64 {
	match glyph {
		' ' | '!' | '\'' | ',' | '.' | ':' | ';' | 'i' | 'j' | 'l' => THIN_EM,
		'(' | ')' | '-' | '/' | 'I' | 'J' | '[' | '\\' | ']' | '_' | '`' | 'f' | 'r' | 't'
		| '{' | '}' => NARROW_EM,
		'%' | '@' | 'M' | 'W' | 'm' | 'w' => WIDE_EM,
		'&' | 'A' | 'B' | 'D' | 'G' | 'H' | 'K' | 'N' | 'O' | 'Q' | 'R' | 'U' | 'V' | 'X' | 'Y' => {
			BROAD_EM
		}
		_ => GLYPH_EM,
	}
}

struct Layout {
	slot: f64,
	left: f64,
	last_slot: usize,
}

impl Layout {
	fn new(count: usize) -> Self {
		let slot = (USABLE_WIDTH / float(count.max(1))).min(MAX_SLOT).floor();
		Self {
			slot,
			left: GUTTER + slot.mul_add(-float(count), USABLE_WIDTH) / 2.0,
			last_slot: count.saturating_sub(1),
		}
	}

	fn center(&self, slot: usize) -> f64 {
		self.slot
			.mul_add(float(slot.min(self.last_slot)) + 0.5, self.left)
			.round()
	}

	fn label_room(&self) -> f64 {
		self.slot - LABEL_PAD
	}

	fn bar_width(&self) -> f64 {
		(self.slot * BAR_RATIO).round()
	}
}

struct Lane<'a> {
	index: usize,
	summary: &'a LanguageSummary,
	cells: Vec<Cell>,
	x: Vec<f64>,
	growth: Vec<(u32, f64)>,
	shown: Vec<Shown>,
	crossing: Vec<(usize, usize)>,
}

impl Lane<'_> {
	const fn name(&self) -> &'static str {
		self.summary.language.name
	}

	fn last(&self) -> Cell {
		self.cells.last().copied().unwrap_or_default()
	}

	fn at(&self, tick: usize, clock: &Clock) -> Cell {
		cell_at(&self.cells, clock.frame_of(tick))
	}

	fn final_height(&self) -> f64 {
		self.growth.last().map_or(0.0, |(_, height)| *height)
	}
}

struct Race<'a> {
	clock: Clock,
	layout: Layout,
	lanes: Vec<Lane<'a>>,
	flight_list: Vec<Flight>,
	heat: Heat,
}

impl<'a> Race<'a> {
	fn new(timeline: &'a Timeline, seconds_per_year: f64) -> Self {
		let languages: &[LanguageSummary] = if timeline.frames.is_empty() {
			&[]
		} else {
			&timeline.languages
		};
		let frames = timeline.frames.len();
		let race = race_ms(timeline, seconds_per_year);
		let provisional = Clock::new(frames, race, 0);
		let row_list = grid(timeline, &provisional);
		let layout = Layout::new(languages.len().min(timeline.top_n));
		let top_lines = row_list
			.iter()
			.filter_map(|row| row.last())
			.map(|cell| cell.lines)
			.max()
			.unwrap_or(0);
		let feed_list = feed_list(&row_list);
		let plan = schedule(
			&throw_list(&feed_list, provisional.tick_ms, |feed| {
				let slot = cell_at(&row_list[feed.lane], feed.frame).slot;
				flight_ms(
					layout.center(slot) - POUCH_X,
					landing_y(bar_height(feed.fed_before, top_lines)),
				)
			}),
			tube_capacity(),
		);
		let clock = Clock::new(frames, race, u32::try_from(-plan.start_ms()).unwrap_or(0));
		let mut lanes: Vec<Lane> = languages
			.iter()
			.zip(row_list)
			.enumerate()
			.map(|(index, (summary, cells))| lane(index, summary, cells, &clock, &layout))
			.collect();
		let mut flight_list: Vec<Flight> = paired(&feed_list, &plan.shots)
			.into_iter()
			.map(|(feed, shot)| flight(feed, shot, &lanes[feed.lane], &clock))
			.collect();
		let cells_list: Vec<&[Cell]> = lanes.iter().map(|lane| lane.cells.as_slice()).collect();
		let shown = shown_list(&cells_list, &flight_list, &clock);
		for (lane, shown) in lanes.iter_mut().zip(shown) {
			lane.growth = growth(lane.index, &lane.cells, &flight_list)
				.into_iter()
				.map(|(ms, lines)| (ms, bar_height(lines, top_lines)))
				.collect();
			lane.shown = shown;
		}
		for flight in &mut flight_list {
			let lane = &lanes[flight.lane];
			flight.landing_y = landing_y(height_before(&lane.growth, flight.land_ms));
			flight.to_x = aim_x(flight, &lane.x, clock.tick_ms, layout.bar_width() / 2.0);
		}
		lanes.retain(|lane| lane.cells.iter().any(|cell| cell.visible));
		let crossing_list = crossing_list(&lanes, &clock, layout.label_room());
		for (lane, crossing) in lanes.iter_mut().zip(crossing_list) {
			lane.crossing = crossing;
		}
		let heat = Heat {
			start_ms: u32::try_from(plan.start_ms() + clock.lead_ms()).unwrap_or(0),
			sample_list: plan.heat,
		};
		Self {
			clock,
			layout,
			lanes,
			flight_list,
			heat,
		}
	}
}

fn crossing_list(lanes: &[Lane], clock: &Clock, reach: f64) -> Vec<Vec<(usize, usize)>> {
	let shown_grid: Vec<Vec<bool>> = lanes
		.iter()
		.map(|lane| shown_ticks(&lane.shown, clock))
		.collect();
	let ranked_grid: Vec<Vec<bool>> = lanes
		.iter()
		.map(|lane| {
			(0..clock.ticks())
				.map(|tick| lane.at(tick, clock).visible)
				.collect()
		})
		.collect();
	let track_list: Vec<Track> = lanes
		.iter()
		.zip(shown_grid.iter().zip(&ranked_grid))
		.map(|(lane, (shown, ranked))| Track {
			x: &lane.x,
			shown,
			ranked,
		})
		.collect();
	close_runs(&track_list, reach, DIP_BRIDGE_TICKS)
}

fn flight(feed: &Feed, shot: &Shot, lane: &Lane, clock: &Clock) -> Flight {
	let at = |ms: i64| u32::try_from(ms + clock.lead_ms()).unwrap_or(u32::MAX);
	let fire_ms = throw_ms(at(shot.fire_ms), clock.duration());
	Flight {
		lane: feed.lane,
		name: lane.name(),
		frame: feed.frame,
		enter_ms: throw_ms(at(shot.enter_ms), clock.duration()).min(fire_ms),
		fire_ms,
		land_ms: at(shot.land_ms),
		to_x: 0.0,
		landing_y: 0.0,
	}
}

fn landing_y(bar_height: f64) -> f64 {
	GROUND - bar_height - FLIGHT_RADIUS
}

fn height_before(growth: &[(u32, f64)], ms: u32) -> f64 {
	growth
		.iter()
		.take_while(|(step_ms, _)| *step_ms < ms)
		.last()
		.map_or(0.0, |(_, height)| *height)
}

fn aim_x(flight: &Flight, x_list: &[f64], tick_ms: u32, half_width: f64) -> f64 {
	let fire = f64::from(flight.fire_ms);
	let land = f64::from(flight.land_ms);
	let absorbed = absorbed_ms(flight);
	let share = (land - fire) / (absorbed - fire);
	let column = x_at(x_list, land, tick_ms);
	let reach = (half_width - CONTACT_MARGIN).max(0.0);
	let contact = (x_at(x_list, absorbed, tick_ms) - POUCH_X)
		.mul_add(share, POUCH_X)
		.clamp(column - reach, column + reach);
	POUCH_X + (contact - POUCH_X) / share
}

fn x_at(x_list: &[f64], ms: f64, tick_ms: u32) -> f64 {
	let tick = u32::try_from(whole_ms(ms.floor())).unwrap_or(u32::MAX) / tick_ms;
	let last = x_list.len().saturating_sub(1);
	let index = usize::try_from(tick).unwrap_or(usize::MAX).min(last);
	let from = x_list.get(index).copied().unwrap_or(POUCH_X);
	let to = x_list.get(index + 1).copied().unwrap_or(from);
	(to - from).mul_add((ms / f64::from(tick_ms) - float(index)).min(1.0), from)
}

fn bar_height(lines: u64, top_lines: u64) -> f64 {
	if lines == 0 {
		return 0.0;
	}
	let ratio = (float(lines) / float(top_lines.max(1))).sqrt();
	(ratio * CHART_HEIGHT).max(MIN_BAR_HEIGHT)
}

fn lane<'a>(
	index: usize,
	summary: &'a LanguageSummary,
	cells: Vec<Cell>,
	clock: &Clock,
	layout: &Layout,
) -> Lane<'a> {
	let first_visible = cells.iter().position(|cell| cell.visible).unwrap_or(0);
	let target_list: Vec<f64> = (0..clock.ticks())
		.map(|tick| {
			let frame = clock.frame_of(tick).max(first_visible);
			layout.center(cell_at(&cells, frame).slot)
		})
		.collect();
	Lane {
		index,
		summary,
		x: settle(&target_list, f64::from(clock.tick_ms)),
		growth: Vec::new(),
		shown: Vec::new(),
		crossing: Vec::new(),
		cells,
	}
}

fn header(timeline: &Timeline, race: &Race, colors: Palette) -> String {
	format!(
		concat!(
			"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-labelledby=\"title desc\">",
			"<title id=\"title\">Languages over time</title>",
			"<desc id=\"desc\">{desc}</desc>",
			"<style>",
			"text{{font:11px {font};fill:{text};text-anchor:middle;font-variant-numeric:tabular-nums}}",
			".s,.n{{font-weight:600}}",
			".n{{font-size:10px}}",
			".m text,text.m{{font-size:10px;fill:{muted}}}",
			".h text{{text-anchor:end}}",
			".d text{{font-size:13px;text-anchor:end;fill:{muted}}}",
			".d .c{{font-size:11px}}",
			".t{{font-size:11px;font-weight:700}}",
			"</style>",
		),
		w = WIDTH,
		h = HEIGHT,
		desc = description(timeline, race),
		font = FONT_STACK,
		text = colors.text,
		muted = colors.muted,
	)
}

fn description(timeline: &Timeline, race: &Race) -> String {
	let span = timeline.frames.first().zip(timeline.frames.last());
	let period = span.map_or_else(String::new, |(first, last)| {
		format!(" from {} to {}", first.week, last.week)
	});
	let mut ranked: Vec<&Lane> = race
		.lanes
		.iter()
		.filter(|lane| lane.last().visible)
		.collect();
	ranked.sort_by_key(|lane| lane.last().rank);
	let summary: Vec<String> = ranked
		.iter()
		.map(|lane| {
			let cell = lane.last();
			format!(
				"{} {} ({} lines, since {}, trend {})",
				escape(lane.name()),
				percent(cell.share_bp),
				compact(cell.lines),
				lane.summary.since.year(),
				trend_word(lane.summary.trend.direction()),
			)
		})
		.collect();
	format!(
		"Share of lines added per language{period}, racing week by week, fed by a slingshot. Final: {}.",
		summary.join("; ")
	)
}

fn defs(race: &Race, colors: Palette) -> String {
	let half = race.layout.slot / 2.0;
	let tokens: Vec<String> = race
		.lanes
		.iter()
		.map(|lane| token(lane.index, lane.summary.language))
		.collect();
	format!(
		concat!(
			"<defs>",
			"{above_ground}",
			"<pattern id=\"h\" width=\"8\" height=\"6\" patternUnits=\"userSpaceOnUse\">",
			"<path d=\"M8 0 2 6\" stroke=\"{ground}\"/>",
			"</pattern>",
			"{share}{lines}{repos}",
			"<clipPath id=\"cd\"><rect x=\"{date_x}\" y=\"{date_y}\" width=\"140\" height=\"18\"/></clipPath>",
			"<clipPath id=\"ct\"><rect x=\"{totals_x}\" y=\"{totals_y}\" width=\"220\" height=\"15\"/></clipPath>",
			"{tokens}",
			"</defs>",
		),
		above_ground = clip("k", half, 0.0, GROUND),
		ground = colors.ground,
		share = clip("cs", half, SHARE_ROW - 12.0, 15.0),
		lines = clip("cl", half, LINES_ROW - 10.5, 13.5),
		repos = clip("cr", half, REPOS_ROW - 10.5, 13.5),
		date_x = num(-140.0, 1),
		date_y = num(DATE_ROW - 14.0, 1),
		totals_x = num(-220.0, 1),
		totals_y = num(TOTALS_ROW - 12.0, 1),
		tokens = tokens.concat(),
	)
}

fn clip(id: &str, half: f64, top: f64, height: f64) -> String {
	format!(
		"<clipPath id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
		num(-half, 1),
		num(top, 1),
		num(2.0 * half, 1),
		num(height, 1),
	)
}

fn token(index: usize, language: Language) -> String {
	let ink = ink(language.color);
	let glyph = icon_path(language.name).map_or_else(
		|| {
			format!(
				"<text class=\"t\" y=\"4\" style=\"fill:{ink}\">{}</text>",
				escape(language.name.get(..1).unwrap_or_default())
			)
		},
		|path| format!("<path transform=\"matrix(.5 0 0 .5 -6 -6)\" fill=\"{ink}\" d=\"{path}\"/>"),
	);
	format!(
		"<g id=\"i{index}\"><circle r=\"{}\" fill=\"{}\"/>{glyph}</g>",
		num(TOKEN_RADIUS, 1),
		language.color
	)
}

fn ink(color: &str) -> &'static str {
	let channel = |range| {
		color
			.get(range)
			.and_then(|hex| u32::from_str_radix(hex, 16).ok())
			.unwrap_or(0)
	};
	let luma = channel(1..3) * 299 + channel(3..5) * 587 + channel(5..7) * 114;
	if luma > DARK_INK_THRESHOLD {
		"#000"
	} else {
		"#fff"
	}
}

fn ground(colors: Palette) -> String {
	format!(
		concat!(
			"<path d=\"M0 {g}H{w}\" stroke=\"{ground}\" stroke-width=\"2\" stroke-dasharray=\"{w}\">{draw}</path>",
			"<rect y=\"{hatch}\" width=\"{w}\" height=\"6\" fill=\"url(#h)\">{grow}</rect>",
		),
		g = num(GROUND, 1),
		w = num(WIDTH, 1),
		ground = colors.ground,
		draw = animate(
			"stroke-dashoffset",
			&format!("{};0", num(WIDTH, 0)),
			GROUND_DRAW_MS
		),
		hatch = num(GROUND + 1.0, 1),
		grow = animate("width", &format!("0;{}", num(WIDTH, 0)), GROUND_DRAW_MS),
	)
}

fn column(lane: &Lane, race: &Race) -> String {
	let clock = &race.clock;
	let duration = clock.duration();
	let width = race.layout.bar_width();
	let x_list: Vec<String> = lane.x.iter().map(|x| num(*x, 1)).collect();
	let name = escape(lane.name());
	let trend = lane.summary.trend;
	format!(
		concat!(
			"<g data-column=\"{name}\" transform=\"translate({x})\"{hidden}>{slide}{fade}",
			"<g clip-path=\"url(#k)\">",
			"<rect data-bar=\"{name}\" x=\"{left}\" width=\"{width}\" rx=\"{radius}\" fill=\"{color}\" transform=\"matrix(1 0 0 -1 0 {base})\" height=\"{height}\">{grow}</rect>",
			"</g>",
			"<g>{dip}<use href=\"#i{index}\" y=\"{token_y}\"/>",
			"{name_label}{legend}{since}{trend}",
			"</g></g>",
		),
		name = name,
		x = num(lane.x.last().copied().unwrap_or(0.0), 1),
		hidden = if lane.last().visible {
			""
		} else {
			" opacity=\"0\""
		},
		slide = animate_transform("translate", &x_list.join(";"), duration),
		fade = fade(&lane.shown, duration),
		dip = dip(&lane.crossing, clock),
		left = num(-width / 2.0, 1),
		width = num(width, 1),
		radius = num(TOP_RADIUS, 1),
		color = lane.summary.language.color,
		base = num(GROUND + TOP_RADIUS, 1),
		height = num(lane.final_height() + TOP_RADIUS, 1),
		grow = growth_animation(lane, duration),
		index = lane.index,
		token_y = num(TOKEN_ROW, 1),
		name_label = name_label(lane.name(), race.layout.label_room()),
		legend = legend(lane, clock),
		since = label(SINCE_ROW, "m", &lane.summary.since.year().to_string()),
		trend = label(
			TREND_ROW,
			"m",
			&format!("{} {}", arrow(trend.direction()), compact(trend.last_year))
		),
	)
}

fn growth_animation(lane: &Lane, duration: f64) -> String {
	discrete(
		"<animate attributeName=\"height\"",
		&num(TOP_RADIUS, 1),
		lane.growth
			.iter()
			.map(|(ms, height)| (*ms, num(height + TOP_RADIUS, 1)))
			.collect(),
		duration,
	)
}

fn merged(shown: &[Shown]) -> Vec<Shown> {
	let mut merged: Vec<Shown> = Vec::new();
	for run in shown {
		let off = run.off.map(|off| off.max(run.on + FADE_MS));
		match merged.last_mut() {
			Some(last) if last.off.is_some_and(|last_off| run.on < last_off + FADE_MS) => {
				last.off = off;
			}
			_ => merged.push(Shown { on: run.on, off }),
		}
	}
	merged
}

fn shown_ticks(shown: &[Shown], clock: &Clock) -> Vec<bool> {
	let margin = f64::from(clock.tick_ms);
	let run_list = merged(shown);
	(0..clock.ticks())
		.map(|tick| {
			let ms = clock.ms(tick);
			run_list.iter().any(|run| {
				run.on - margin <= ms && run.off.is_none_or(|off| ms < off + FADE_MS + margin)
			})
		})
		.collect()
}

fn dip_runs(crossing: &[(usize, usize)], last: usize) -> Vec<(usize, usize)> {
	let mut run_list: Vec<(usize, usize)> = Vec::new();
	for (start, end) in crossing.iter().filter(|(start, _)| *start < last) {
		let ramp_in = start.saturating_sub(1);
		match run_list.last_mut() {
			Some(run) if ramp_in <= run.1 + 2 => run.1 = run.1.max(*end),
			_ => run_list.push((*start, *end)),
		}
	}
	run_list
}

fn dip(crossing: &[(usize, usize)], clock: &Clock) -> String {
	let last = clock.ticks() - 1;
	let run_list = dip_runs(crossing, last);
	if run_list.is_empty() {
		return String::new();
	}
	let mut point_list: Vec<(usize, f64)> = vec![(0, 1.0)];
	for (start, end) in run_list {
		for point in [
			(start.saturating_sub(1), 1.0),
			(start, LEGEND_DIP),
			((end + 1).min(last - 1), LEGEND_DIP),
			((end + 2).min(last), 1.0),
		] {
			if point_list
				.last()
				.is_some_and(|previous| previous.0 == point.0)
			{
				point_list.pop();
			}
			point_list.push(point);
		}
	}
	if point_list.last().is_some_and(|point| point.0 < last) {
		point_list.push((last, 1.0));
	}
	let duration = clock.duration();
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(tick, _)| key_time(clock.ms(*tick), duration))
		.collect();
	let value_list: Vec<String> = point_list
		.iter()
		.map(|(_, opacity)| num(*opacity, 2))
		.collect();
	format!(
		"<animate attributeName=\"opacity\" values=\"{}\" keyTimes=\"{}\" dur=\"{}ms\" fill=\"freeze\"/>",
		value_list.join(";"),
		key_list.join(";"),
		num(duration, 0),
	)
}

fn fade(shown: &[Shown], duration: f64) -> String {
	let merged = merged(shown);
	let mut point_list: Vec<(f64, bool)> =
		vec![(0.0, merged.first().is_some_and(|first| first.on <= 0.0))];
	for run in &merged {
		if run.on > 0.0 {
			point_list.extend([(run.on, false), (run.on + FADE_MS, true)]);
		}
		if let Some(off) = run.off {
			point_list.extend([(off, true), (off + FADE_MS, false)]);
		}
	}
	if point_list.len() < 2 {
		return String::new();
	}
	let opacity = |visible: &bool| if *visible { "1" } else { "0" };
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(ms, _)| key_time(ms.min(duration), duration))
		.chain(["1".to_owned()])
		.collect();
	let value_list: Vec<&str> = point_list
		.iter()
		.map(|(_, visible)| opacity(visible))
		.chain(point_list.last().map(|(_, visible)| opacity(visible)))
		.collect();
	format!(
		"<animate attributeName=\"opacity\" values=\"{}\" keyTimes=\"{}\" dur=\"{}ms\" fill=\"freeze\"/>",
		value_list.join(";"),
		key_list.join(";"),
		num(duration, 0),
	)
}

fn name_label(name: &str, room: f64) -> String {
	let fit = if name_width(name) > room {
		format!(
			" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"",
			num(room, 1)
		)
	} else {
		String::new()
	};
	format!(
		"<text class=\"n\" y=\"{}\"{fit}>{}</text>",
		num(NAME_ROW, 1),
		escape(name)
	)
}

fn headers(race: &Race) -> String {
	if race.lanes.is_empty() {
		return String::new();
	}
	let x = num(race.layout.left - HEADER_GAP, 1);
	let text_list: Vec<String> = HEADER_LIST
		.iter()
		.zip(HEADER_ROW_LIST)
		.map(|(header, y)| format!("<text x=\"{x}\" y=\"{}\">{header}</text>", num(y, 1)))
		.collect();
	format!("<g class=\"m h\">{}</g>", text_list.concat())
}

fn label(y: f64, class: &str, content: &str) -> String {
	format!(
		"<text class=\"{class}\" y=\"{}\">{content}</text>",
		num(y, 1)
	)
}

fn legend(lane: &Lane, clock: &Clock) -> String {
	let duration = clock.duration();
	let cell_list: Vec<Cell> = clock
		.state_ticks()
		.into_iter()
		.map(|tick| lane.at(tick, clock))
		.collect();
	let share = ticker(
		cell_list
			.iter()
			.map(|cell| percent(cell.share_bp))
			.collect(),
		Row {
			clip: "cs",
			class: "s",
			y: SHARE_ROW,
			step: SHARE_STEP,
		},
		duration,
	);
	let lines = ticker(
		cell_list.iter().map(|cell| compact(cell.lines)).collect(),
		Row {
			clip: "cl",
			class: "m",
			y: LINES_ROW,
			step: ROW_STEP,
		},
		duration,
	);
	let repos = ticker(
		cell_list
			.iter()
			.map(|cell| cell.repos.to_string())
			.collect(),
		Row {
			clip: "cr",
			class: "m",
			y: REPOS_ROW,
			step: ROW_STEP,
		},
		duration,
	);
	[share, lines, repos].concat()
}

fn date_and_totals(timeline: &Timeline, clock: &Clock) -> String {
	let frame_list: Vec<Option<&Frame>> = clock
		.state_ticks()
		.into_iter()
		.map(|tick| timeline.frames.get(clock.frame_of(tick)))
		.collect();
	let state_list = |label: fn(&Frame) -> String| -> Vec<String> {
		frame_list
			.iter()
			.map(|frame| frame.map_or_else(String::new, label))
			.collect()
	};
	let date = Row {
		clip: "cd",
		class: "d",
		y: DATE_ROW,
		step: DATE_STEP,
	};
	let totals = Row {
		clip: "ct",
		class: "c",
		y: TOTALS_ROW,
		step: TOTALS_STEP,
	};
	format!(
		"<g class=\"d\" transform=\"translate({} 0)\">{}{}</g>",
		num(DATE_X, 1),
		ticker(
			state_list(|frame| month(frame.week)),
			date,
			clock.duration()
		),
		ticker(
			state_list(|frame| totals_label(frame.language_count, frame.total_lines)),
			totals,
			clock.duration()
		),
	)
}

#[cfg(test)]
mod tests {
	use super::super::feeder::POUCH_Y;
	use super::super::probe::{
		Pool, drop_at, drop_begins, linear_at, off_line, vertex_list, wrapped,
	};
	use super::super::ranking::grid;
	use super::*;
	use crate::cache::Usage;
	use crate::config::MIN_SECONDS_PER_YEAR;
	use crate::history::Week;
	use crate::theme::{Palette, palette};
	use crate::timeline::{Entry, Frame, Trend, timeline};
	use jiff::Span;
	use jiff::civil::Date;
	use roxmltree::{Document, Node};
	use std::ops::Range;

	const PACE: f64 = 5.0;
	const VALUE_FONT_PX: f64 = 11.0;
	const FINAL_ORDER: [&str; 5] = ["Rust", "C", "Python", "Nix", "TypeScript"];
	const FINAL_LINES: [f64; 5] = [4200.0, 3000.0, 1500.0, 800.0, 300.0];
	const HIDDEN_AT_END: [&str; 2] = ["Shell", "Go"];
	const LARGE_NAME_LIST: [&str; 18] = [
		"Rust",
		"C",
		"C++",
		"Python",
		"Nix",
		"TypeScript",
		"JavaScript",
		"Shell",
		"Makefile",
		"Go",
		"Lua",
		"Haskell",
		"CSS",
		"HTML",
		"SCSS",
		"OpenSCAD",
		"Dockerfile",
		"Boo",
	];

	fn week(index: i64) -> Week {
		Date::constant(2020, 1, 6).saturating_add(Span::new().weeks(index))
	}

	fn spread(usage: &mut Usage, name: &'static str, weeks: Range<i64>, lines: u64) {
		for index in weeks {
			*usage
				.weeks
				.entry(week(index))
				.or_default()
				.entry(name)
				.or_default() += lines;
		}
	}

	fn fixture() -> Timeline {
		let mut usage = Usage::default();
		spread(&mut usage, "Rust", 0..150, 28);
		spread(&mut usage, "C", 0..30, 100);
		spread(&mut usage, "Python", 50..150, 15);
		spread(&mut usage, "Nix", 60..140, 10);
		spread(&mut usage, "TypeScript", 100..150, 6);
		spread(&mut usage, "Shell", 10..130, 1);
		spread(&mut usage, "Go", 20..100, 1);
		let first_use: [(&'static str, &[i64]); 5] = [
			("Rust", &[0, 40, 100]),
			("C", &[0, 5]),
			("Python", &[50]),
			("Nix", &[60, 61]),
			("TypeScript", &[100]),
		];
		for (name, indices) in first_use {
			usage
				.first_use
				.insert(name, indices.iter().map(|index| week(*index)).collect());
		}
		let result = timeline(&usage, 5, 150);
		assert_eq!(result.frames.len(), 150);
		assert_eq!(result.languages.len(), 7);
		result
	}

	fn wanderer() -> Timeline {
		let mut usage = Usage::default();
		spread(&mut usage, "Rust", 0..60, 100);
		spread(&mut usage, "C", 0..10, 50);
		spread(&mut usage, "Go", 10..25, 70);
		spread(&mut usage, "C", 25..40, 120);
		spread(&mut usage, "Go", 40..60, 200);
		timeline(&usage, 2, 520)
	}

	fn large() -> Timeline {
		let mut usage = Usage::default();
		for (offset, name) in (0_u64..).zip(LARGE_NAME_LIST) {
			for index in 0..520 {
				let lines = (u64::try_from(index).unwrap() * 37 + offset * 101) % 997;
				spread(&mut usage, name, index..index + 1, lines);
			}
			usage
				.first_use
				.insert(name, (0..40).map(|index| week(index * 13)).collect());
		}
		timeline(&usage, 16, 520)
	}

	fn value_width(text: &str) -> f64 {
		float(text.chars().count()) * GLYPH_EM * VALUE_FONT_PX
	}

	fn number(node: Node, name: &str) -> f64 {
		node.attribute(name).unwrap().parse().unwrap()
	}

	fn ms(node: Node, name: &str) -> f64 {
		node.attribute(name)
			.unwrap()
			.trim_end_matches("ms")
			.parse()
			.unwrap()
	}

	fn tagged<'a>(document: &'a Document, key: &str, value: &str) -> Node<'a, 'a> {
		document
			.descendants()
			.find(|node| node.attribute(key) == Some(value))
			.unwrap()
	}

	fn text_of(node: Node) -> String {
		node.descendants()
			.filter(Node::is_text)
			.filter_map(|part| part.text())
			.collect()
	}

	fn text_list(node: Node) -> Vec<String> {
		node.descendants()
			.filter(|child| child.has_tag_name("text"))
			.map(text_of)
			.collect()
	}

	fn translate_list(node: Node) -> Vec<f64> {
		let animation = node
			.children()
			.find(|child| {
				child.has_tag_name("animateTransform")
					&& child.attribute("type") == Some("translate")
			})
			.unwrap();
		animation
			.attribute("values")
			.unwrap()
			.split(';')
			.map(|pair| pair.split(' ').next().unwrap().parse().unwrap())
			.collect()
	}

	fn column<'a>(document: &'a Document, name: &str) -> Node<'a, 'a> {
		tagged(document, "data-column", name)
	}

	fn column_x(document: &Document, name: &str) -> f64 {
		let transform = column(document, name).attribute("transform").unwrap();
		transform
			.trim_start_matches("translate(")
			.trim_end_matches(')')
			.parse()
			.unwrap()
	}

	fn has_slide(node: Node) -> bool {
		node.children().any(|child| {
			child.has_tag_name("animateTransform")
				&& child.attribute("type") == Some("translate")
				&& child.attribute("calcMode").is_none()
		})
	}

	fn shown_text(column: Node, clip: &str, row: f64) -> String {
		let clipped = column
			.descendants()
			.find(|node| node.attribute("clip-path") == Some(&format!("url(#{clip})")))
			.unwrap();
		let stack = clipped.first_element_child().unwrap();
		let offset: f64 = stack
			.attribute("transform")
			.unwrap()
			.trim_start_matches("translate(0 ")
			.trim_end_matches(')')
			.parse()
			.unwrap();
		stack
			.children()
			.find(|node| {
				node.has_tag_name("text") && (number(*node, "y") + offset - row).abs() < 0.01
			})
			.map(text_of)
			.unwrap()
	}

	fn height_list(document: &Document, name: &str) -> Vec<f64> {
		let animation = tagged(document, "data-bar", name)
			.descendants()
			.find(|node| node.attribute("attributeName") == Some("height"))
			.unwrap();
		animation
			.attribute("values")
			.unwrap()
			.split(';')
			.map(|value| value.parse().unwrap())
			.collect()
	}

	fn shows_from_start(node: Node) -> bool {
		node.ancestors().all(|ancestor| {
			ancestor.attribute("opacity") != Some("0")
				&& ancestor
					.children()
					.all(|child| child.attribute("attributeName") != Some("opacity"))
		})
	}

	fn fade_points(column: Node) -> Vec<(f64, f64)> {
		column
			.children()
			.find(|child| child.attribute("attributeName") == Some("opacity"))
			.map_or_else(Vec::new, |fade| {
				let duration = ms(fade, "dur");
				let key_list = fade.attribute("keyTimes").unwrap().split(';');
				let value_list = fade.attribute("values").unwrap().split(';');
				key_list
					.zip(value_list)
					.map(|(key, value)| {
						(
							key.parse::<f64>().unwrap() * duration,
							value.parse().unwrap(),
						)
					})
					.collect()
			})
	}

	fn opacity_steps(column: Node) -> Vec<f64> {
		let mut step_list: Vec<f64> = fade_points(column)
			.into_iter()
			.map(|(_, value)| value)
			.collect();
		step_list.dedup();
		step_list
	}

	fn first_rise_ms(column: Node) -> Option<f64> {
		let point_list = fade_points(column);
		let rise = point_list.iter().position(|(_, value)| *value > 0.0)?;
		(rise > 0).then(|| point_list[rise - 1].0)
	}

	fn single(name: &'static str) -> Timeline {
		Timeline {
			languages: vec![LanguageSummary {
				language: Language {
					name,
					color: "#123456",
				},
				since: week(0),
				trend: Trend {
					last_year: 10,
					previous_year: 0,
				},
			}],
			frames: vec![Frame {
				week: week(0),
				entries: vec![Entry {
					language: 0,
					lines: 10,
					share_bp: 10_000,
					repos: 1,
				}],
				language_count: 1,
				total_lines: 10,
			}],
			top_n: 2,
		}
	}

	fn end_ms(svg: &str) -> f64 {
		let document = Document::parse(svg).unwrap();
		let ms = |node: Node, name| {
			node.attribute(name).map_or(0.0, |value: &str| {
				value
					.split(';')
					.map(|part| part.trim_end_matches("ms").parse().unwrap())
					.fold(0.0, f64::max)
			})
		};
		document
			.descendants()
			.map(|node| ms(node, "begin") + ms(node, "dur"))
			.fold(0.0, f64::max)
	}

	fn color_list(palette: Palette) -> [&'static str; 3] {
		[palette.text, palette.muted, palette.ground]
	}

	fn shown_at(column: Node, ms: f64) -> bool {
		let point_list = fade_points(column);
		if point_list.is_empty() {
			return column.attribute("opacity") != Some("0");
		}
		let after = point_list
			.iter()
			.position(|(at, _)| *at > ms + 0.5)
			.unwrap();
		point_list[after - 1].1.max(point_list[after].1) > 0.0
	}

	fn fall_list(column: Node) -> Vec<(f64, f64)> {
		let point_list = fade_points(column);
		let rise_list: Vec<f64> = point_list
			.windows(2)
			.filter(|pair| pair[0].1 < pair[1].1)
			.map(|pair| pair[0].0)
			.collect();
		point_list
			.windows(2)
			.filter(|pair| pair[0].1 > pair[1].1)
			.map(|pair| {
				let next_rise = rise_list
					.iter()
					.copied()
					.find(|rise| *rise > pair[0].0)
					.unwrap_or(f64::INFINITY);
				(pair[0].0, next_rise)
			})
			.collect()
	}

	fn visible_growth_frames(cells: &[Cell]) -> Vec<usize> {
		let mut previous = 0;
		cells
			.iter()
			.enumerate()
			.filter_map(|(frame, cell)| {
				let grew = cell.lines > previous;
				previous = cell.lines;
				(cell.visible && grew).then_some(frame)
			})
			.collect()
	}

	fn height_step_list(document: &Document, name: &str) -> Vec<(f64, f64)> {
		let animation = tagged(document, "data-bar", name)
			.descendants()
			.find(|node| node.attribute("attributeName") == Some("height"))
			.unwrap();
		assert_eq!(animation.attribute("calcMode"), Some("discrete"));
		let duration = ms(animation, "dur");
		let key_list = animation.attribute("keyTimes").unwrap().split(';');
		let value_list = animation.attribute("values").unwrap().split(';');
		key_list
			.zip(value_list)
			.map(|(key, value)| {
				(
					key.parse::<f64>().unwrap() * duration,
					value.parse::<f64>().unwrap() - TOP_RADIUS,
				)
			})
			.collect()
	}

	fn height_at(step_list: &[(f64, f64)], at: f64) -> f64 {
		step_list.iter().rfind(|(step, _)| *step <= at).unwrap().1
	}

	fn slide_at(column: Node, at: f64) -> f64 {
		let animation = column
			.children()
			.find(|child| child.attribute("type") == Some("translate"))
			.unwrap();
		let x_list = translate_list(column);
		let step = ms(animation, "dur") / float(x_list.len() - 1);
		let index = (0..x_list.len() - 1)
			.rfind(|index| float(*index) * step <= at)
			.unwrap();
		let part = (at / step - float(index)).min(1.0);
		(x_list[index + 1] - x_list[index]).mul_add(part, x_list[index])
	}

	fn number_list(node: Node, name: &str) -> Vec<f64> {
		node.attribute(name)
			.unwrap()
			.split(';')
			.map(|value| value.trim().trim_end_matches("ms").parse().unwrap())
			.collect()
	}

	fn assert_valid_smil(svg: &str) {
		assert!(!svg.contains("NaN"));
		assert!(!svg.contains("inf"));
		let document = Document::parse(svg).unwrap();
		for node in document.descendants() {
			if node.has_attribute("dur") {
				let duration = ms(node, "dur");
				assert!(duration.is_finite() && duration > 0.0, "{node:?}");
				assert_eq!(node.attribute("fill"), Some("freeze"), "{node:?}");
			}
			if node.has_attribute("begin") {
				let begin_list = number_list(node, "begin");
				assert!(
					begin_list
						.iter()
						.all(|begin| begin.is_finite() && *begin >= 0.0)
				);
				assert!(
					begin_list.windows(2).all(|pair| pair[1] > pair[0]),
					"{node:?}"
				);
			}
			if node.attribute("calcMode") == Some("spline") {
				let value_count = node.attribute("values").unwrap().split(';').count();
				let spline_list = node.attribute("keySplines").unwrap().split(';');
				assert_eq!(spline_list.clone().count(), value_count - 1, "{node:?}");
				assert!(
					spline_list
						.flat_map(|spline| spline.split(' '))
						.all(|control| (0.0..=1.0).contains(&control.parse::<f64>().unwrap()))
				);
			}
			if node.has_attribute("keyTimes") {
				let key_list = number_list(node, "keyTimes");
				let value_count = node.attribute("values").unwrap().split(';').count();
				assert_eq!(key_list.len(), value_count, "{node:?}");
				assert!(key_list[0] == 0.0, "{node:?}");
				assert!(key_list.iter().all(|key| (0.0..=1.0).contains(key)));
				assert!(
					key_list.windows(2).all(|pair| pair[0] <= pair[1]),
					"{node:?}"
				);
				if node.attribute("calcMode") != Some("discrete") {
					assert!(
						(key_list.last().unwrap() - 1.0).abs() < f64::EPSILON,
						"{node:?}"
					);
				}
			}
		}
	}

	fn burst() -> Timeline {
		let mut usage = Usage::default();
		for name in [
			"Rust", "C", "Python", "Nix", "Go", "Lua", "Shell", "Haskell",
		] {
			spread(&mut usage, name, 0..20, 40);
		}
		timeline(&usage, 8, 520)
	}

	fn swap() -> Timeline {
		let mut usage = Usage::default();
		spread(&mut usage, "Rust", 0..40, 100);
		spread(&mut usage, "C", 0..40, 50);
		spread(&mut usage, "Go", 10..40, 120);
		timeline(&usage, 2, 520)
	}

	fn crowd() -> Timeline {
		let mut usage = Usage::default();
		for name in ["Rust", "C", "Python", "Nix", "Go", "Lua"] {
			spread(&mut usage, name, 0..30, 50);
		}
		timeline(&usage, 6, 520)
	}

	fn flight_nodes<'a>(document: &'a Document) -> Vec<Node<'a, 'a>> {
		document
			.descendants()
			.filter(|node| node.attribute("data-flight").is_some())
			.collect()
	}

	fn fire_list(document: &Document, name: &str) -> Vec<f64> {
		let mut list: Vec<f64> = flight_nodes(document)
			.into_iter()
			.filter(|node| node.attribute("data-flight") == Some(name))
			.flat_map(|node| Pool::of(node).begin_list)
			.collect();
		list.sort_by(f64::total_cmp);
		list
	}

	fn apex_ms(pool: &Pool, begin: f64) -> f64 {
		pool.samples(begin, 400)
			.into_iter()
			.min_by(|left, right| (left.1).1.total_cmp(&(right.1).1))
			.unwrap()
			.0
	}

	fn contact_ms(pool: &Pool, begin: f64, step_list: &[(f64, f64)]) -> f64 {
		let landing = pool.vanish(begin);
		let apex = apex_ms(pool, begin);
		(0_u32..40_000)
			.map(|step| f64::from(step).mul_add(0.25, apex))
			.take_while(|at| *at <= landing + 10.0)
			.find(|at| pool.at(*at, begin).1 + FLIGHT_RADIUS >= GROUND - height_at(step_list, *at))
			.unwrap()
	}

	fn paced() -> impl Iterator<Item = (Timeline, f64)> {
		[fixture(), swap(), wanderer()]
			.into_iter()
			.flat_map(|data| [(data.clone(), PACE), (data, MIN_SECONDS_PER_YEAR)])
	}

	#[test]
	fn one_flight_per_visible_growth() {
		let data = fixture();
		let race = Race::new(&data, PACE);
		let row_list = grid(&data, &race.clock);
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let mut total = 0;
		for (index, (summary, cells)) in data.languages.iter().zip(&row_list).enumerate() {
			let name = summary.language.name;
			let expected = visible_growth_frames(cells);
			assert_eq!(fire_list(&document, name).len(), expected.len(), "{name}");
			let mut frame_list: Vec<usize> = race
				.flight_list
				.iter()
				.filter(|flight| flight.lane == index)
				.map(|flight| flight.frame)
				.collect();
			frame_list.sort_unstable();
			assert_eq!(frame_list, expected, "{name}");
			total += expected.len();
		}
		assert_eq!(race.flight_list.len(), total);
		assert!(total > 300, "{total}");
		for node in flight_nodes(&document) {
			let name = node.attribute("data-flight").unwrap();
			let index = data
				.languages
				.iter()
				.position(|summary| summary.language.name == name)
				.unwrap();
			assert_eq!(node.attribute("href"), Some(format!("#h{index}").as_str()));
		}
	}

	#[test]
	fn every_flight_lands_before_its_frame_shows() {
		for data in [fixture(), wanderer(), burst(), swap(), crowd()] {
			let race = Race::new(&data, PACE);
			let clock = &race.clock;
			assert!(!race.flight_list.is_empty());
			for flight in &race.flight_list {
				let frame_ms = clock.ms(clock.lead + flight.frame);
				assert!(
					f64::from(flight.land_ms) <= frame_ms,
					"{} {frame_ms}",
					flight.land_ms
				);
				assert!(flight.enter_ms <= flight.fire_ms && flight.fire_ms < flight.land_ms);
			}
		}
	}

	#[test]
	fn bar_grows_only_on_landing() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in FINAL_ORDER {
			let step_list = height_step_list(&document, name);
			let mut contact_list: Vec<f64> = flight_nodes(&document)
				.into_iter()
				.filter(|node| node.attribute("data-flight") == Some(name))
				.flat_map(|node| {
					let pool = Pool::of(node);
					pool.begin_list
						.iter()
						.map(|begin| contact_ms(&pool, *begin, &step_list))
						.collect::<Vec<_>>()
				})
				.collect();
			contact_list.sort_by(f64::total_cmp);
			assert_eq!(step_list[0], (0.0, 0.0), "{name}");
			assert_eq!(step_list.len(), contact_list.len() + 1, "{name}");
			for (contact, pair) in contact_list.iter().zip(step_list.windows(2)) {
				assert!(
					(contact - pair[1].0).abs() <= 3.0,
					"{name} {contact} {pair:?}"
				);
				assert!(pair[1].1 >= pair[0].1, "{name}");
			}
			let final_height = number(tagged(&document, "data-bar", name), "height") - TOP_RADIUS;
			assert!(
				(step_list.last().unwrap().1 - final_height).abs() < 0.1,
				"{name}"
			);
		}
	}

	#[test]
	fn entering_column_opens_on_first_fire() {
		let mut entered = 0;
		for data in [fixture(), wanderer()] {
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			for summary in &data.languages {
				let name = summary.language.name;
				let fires = fire_list(&document, name);
				let point_list = fade_points(column(&document, name));
				for pair in point_list.windows(2).filter(|pair| pair[0].1 < pair[1].1) {
					let rise = pair[0].0;
					assert!(
						fires.iter().any(|fire| (fire - rise).abs() <= 1.0),
						"{name} {rise}"
					);
					entered += 1;
				}
			}
		}
		assert!(entered >= 6, "{entered}");
	}

	#[test]
	fn bar_steps_never_follow_their_frame() {
		for data in [fixture(), swap(), wanderer()] {
			let race = Race::new(&data, PACE);
			let clock = &race.clock;
			let duration = clock.duration();
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			for lane in &race.lanes {
				let animation = tagged(&document, "data-bar", lane.name())
					.descendants()
					.find(|node| node.attribute("attributeName") == Some("height"))
					.unwrap();
				let step_key_list: Vec<f64> = number_list(animation, "keyTimes");
				let mut fed: Vec<&Flight> = race
					.flight_list
					.iter()
					.filter(|flight| flight.lane == lane.index)
					.collect();
				fed.sort_by_key(|flight| flight.land_ms);
				assert_eq!(step_key_list.len(), fed.len() + 1, "{}", lane.name());
				for (step_key, flight) in step_key_list[1..].iter().zip(fed) {
					let frame_key: f64 = key_time(clock.ms(clock.lead + flight.frame), duration)
						.parse()
						.unwrap();
					assert!(
						*step_key <= frame_key,
						"{} {step_key} {frame_key}",
						lane.name()
					);
				}
			}
		}
	}

	fn landing_miss(document: &Document, pool: &Pool, name: &str, begin: f64) -> f64 {
		let column = column(document, name);
		let width = number(tagged(document, "data-bar", name), "width");
		let contact = contact_ms(pool, begin, &height_step_list(document, name));
		(pool.at(contact, begin).0 - slide_at(column, contact)).abs() - width / 2.0
	}

	#[test]
	fn circles_land_on_their_column() {
		for (data, pace) in paced() {
			let svg = render_languages(&data, Variant::Dark, pace);
			let document = Document::parse(&svg).unwrap();
			let mut landed = 0;
			for node in flight_nodes(&document) {
				let name = node.attribute("data-flight").unwrap();
				let pool = Pool::of(node);
				for begin in &pool.begin_list {
					let miss = landing_miss(&document, &pool, name, *begin);
					assert!(miss <= 0.0, "{name} {begin} {miss}");
					landed += 1;
				}
			}
			assert!(landed > 20, "{landed}");
		}
		let dense = large();
		let svg = render_languages(&dense, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let sampled: Vec<(Node, f64)> = flight_nodes(&document)
			.into_iter()
			.flat_map(|node| {
				Pool::of(node)
					.begin_list
					.into_iter()
					.step_by(7)
					.map(move |begin| (node, begin))
			})
			.step_by(13)
			.collect();
		assert!(sampled.len() >= 60, "{}", sampled.len());
		for (node, begin) in sampled {
			let name = node.attribute("data-flight").unwrap();
			let miss = landing_miss(&document, &Pool::of(node), name, begin);
			assert!(miss <= 0.0, "{name} {begin} {miss}");
		}
	}

	fn assert_starts_in_pouch(start: (f64, f64)) {
		assert!((start.0 - POUCH_X).abs() <= 1.0, "{start:?}");
		assert!((start.1 - POUCH_Y).abs() <= 1.0, "{start:?}");
	}

	fn assert_reused_once_hidden(pool: &Pool) {
		assert!(
			pool.reuse_gaps().iter().all(|gap| *gap > 0.0),
			"{:?}",
			pool.reuse_gaps()
		);
	}

	#[test]
	fn long_races_keep_the_pouch_and_reuse_hidden_circles() {
		let data = fixture();
		let pace = 60.0;
		assert!(Race::new(&data, pace).clock.duration() > 140_000.0);
		let svg = render_languages(&data, Variant::Dark, pace);
		assert_valid_smil(&svg);
		let document = Document::parse(&svg).unwrap();
		let mut started = 0;
		for node in flight_nodes(&document) {
			let pool = Pool::of(node);
			assert_reused_once_hidden(&pool);
			for begin in &pool.begin_list {
				assert_starts_in_pouch(pool.at(*begin, *begin));
				assert!(pool.shown(*begin) && pool.shown(pool.vanish(*begin) - 1.0));
				started += 1;
			}
		}
		assert!(started > 300, "{started}");
	}

	#[test]
	fn flights_follow_a_ballistic_arc() {
		let mut gravity_list: Vec<f64> = Vec::new();
		let mut checked = 0;
		for (data, pace) in paced() {
			let svg = render_languages(&data, Variant::Dark, pace);
			let document = Document::parse(&svg).unwrap();
			for node in flight_nodes(&document) {
				let pool = Pool::of(node);
				let gravity = pool.gravity();
				gravity_list.push(gravity);
				for quarter in 0..=4 {
					let flown = pool.drop_ms() * float(quarter) / 4.0;
					let fallen = pool.drop_at(flown).1 - pool.drop_at(0.0).1;
					assert!(
						(fallen - gravity * flown * flown / 2.0).abs() <= 0.5,
						"{fallen}"
					);
				}
				for begin in &pool.begin_list {
					let sample_list = pool.samples(*begin, 40);
					let x_list: Vec<(f64, f64)> = sample_list
						.iter()
						.map(|(at, point)| (*at, point.0))
						.collect();
					assert!(off_line(&x_list) <= 0.5, "{begin} {}", off_line(&x_list));
					let lifted: Vec<(f64, f64)> = sample_list
						.iter()
						.map(|(at, point)| {
							let flown = at - begin;
							(*at, (gravity * flown / 2.0).mul_add(-flown, point.1))
						})
						.collect();
					assert!(off_line(&lifted) <= 0.5, "{begin} {}", off_line(&lifted));
					let (start, end) = (sample_list[0].1, sample_list[40].1);
					assert_starts_in_pouch(start);
					let apex = sample_list
						.iter()
						.map(|(_, point)| point.1)
						.fold(f64::INFINITY, f64::min);
					assert!(apex < start.1.min(end.1) - 8.0, "{apex} {start:?} {end:?}");
					assert!(sample_list[39].1.1 < end.1, "{begin}");
					checked += 1;
				}
			}
		}
		assert!(checked > 100, "{checked}");
		let dense = render_languages(&large(), Variant::Dark, PACE);
		let document = Document::parse(&dense).unwrap();
		for node in flight_nodes(&document) {
			let pool = Pool::of(node);
			for begin in &pool.begin_list {
				assert_starts_in_pouch(pool.at(*begin, *begin));
			}
		}
		assert!(
			gravity_list
				.iter()
				.all(|gravity| (gravity / gravity_list[0] - 1.0).abs() < 1e-3),
			"{gravity_list:?}"
		);
	}

	#[test]
	fn in_air_circles_keep_their_path() {
		let mut moving = 0;
		for (data, pace) in paced() {
			let svg = render_languages(&data, Variant::Dark, pace);
			let document = Document::parse(&svg).unwrap();
			for node in flight_nodes(&document) {
				let name = node.attribute("data-flight").unwrap();
				let column = column(&document, name);
				let step_list = height_step_list(&document, name);
				let pool = Pool::of(node);
				for begin in &pool.begin_list {
					let landing = contact_ms(&pool, *begin, &step_list);
					let absorbed = f64::midpoint(landing, pool.vanish(*begin));
					let target = slide_at(column, absorbed);
					if (target - slide_at(column, *begin)).abs() < 1.0 {
						continue;
					}
					moving += 1;
					let x_list: Vec<(f64, f64)> = pool
						.samples(*begin, 40)
						.iter()
						.map(|(at, point)| (*at, point.0))
						.collect();
					assert!(off_line(&x_list) <= 0.5, "{begin}");
					let x = pool.at(absorbed, *begin).0;
					assert!((x - target).abs() <= 1.5, "{begin} {x} {target}");
				}
			}
		}
		assert!(moving >= 3, "{moving}");
	}

	fn ms_between(from: f64, to: f64, step: u32) -> impl Iterator<Item = f64> {
		(0_u32..100_000)
			.map(move |index| f64::from(index * step) + from)
			.take_while(move |at| *at < to)
	}

	#[test]
	fn circles_vanish_once_half_sunk() {
		let mut beside = f64::NEG_INFINITY;
		let mut absorbed = 0;
		for (data, pace) in paced() {
			let svg = render_languages(&data, Variant::Dark, pace);
			let document = Document::parse(&svg).unwrap();
			for node in flight_nodes(&document) {
				let name = node.attribute("data-flight").unwrap();
				let column = column(&document, name);
				let half = number(tagged(&document, "data-bar", name), "width") / 2.0;
				let step_list = height_step_list(&document, name);
				let pool = Pool::of(node);
				let hidden_y = GROUND + drawn_extent(&document, node);
				assert!(hidden_y >= GROUND + FLIGHT_RADIUS + 6.0, "{hidden_y}");
				assert!(pool.static_at().1 >= hidden_y, "{name}");
				assert_reused_once_hidden(&pool);
				let next_list = pool.begin_list.iter().skip(1).copied();
				for (begin, next) in pool.begin_list.iter().zip(next_list.chain([f64::INFINITY])) {
					let contact = contact_ms(&pool, *begin, &step_list);
					let top = GROUND - height_at(&step_list, contact - 1.0);
					let centered = pool.reaching(*begin, top);
					let vanish = pool.vanish(*begin);
					assert!(
						vanish >= centered - 1.0 && vanish <= centered + 2.0,
						"{name} {begin} {centered} {vanish}"
					);
					assert!(
						ms_between(*begin, vanish, 1).all(|at| pool.shown(at)),
						"{name} {begin}"
					);
					for at in ms_between(contact, vanish, 1) {
						beside =
							beside.max((pool.at(at, *begin).0 - slide_at(column, at)).abs() - half);
					}
					assert!(
						ms_between(vanish, next.min(begin + 3_000.0), 2)
							.all(|at| { !pool.shown(at) || pool.at(at, *begin).1 >= hidden_y }),
						"{name} {begin}"
					);
					absorbed += 1;
				}
			}
		}
		assert!(absorbed > 100, "{absorbed}");
		assert!(beside <= 0.0, "{beside}");
	}

	fn scale_of(node: Node) -> f64 {
		node.attribute("transform")
			.and_then(|transform| transform.strip_prefix("scale("))
			.map_or(1.0, |scale| {
				scale
					.trim_end_matches(')')
					.split(' ')
					.next()
					.unwrap()
					.parse()
					.unwrap()
			})
	}

	fn drawn_radius(document: &Document, node: Node) -> f64 {
		let inner = if node.has_tag_name("circle") {
			return scale_of(node) * number(node, "r");
		} else if node.has_tag_name("use") {
			tagged(document, "id", &node.attribute("href").unwrap()[1..])
		} else {
			node.first_element_child().unwrap()
		};
		scale_of(node) * drawn_radius(document, inner)
	}

	fn drawn_extent(document: &Document, node: Node) -> f64 {
		let reach = if node.has_tag_name("circle") {
			number(node, "r")
				+ node
					.attribute("stroke-width")
					.map_or(0.0, |width| width.parse::<f64>().unwrap() / 2.0)
		} else if node.has_tag_name("use") {
			drawn_extent(
				document,
				tagged(document, "id", &node.attribute("href").unwrap()[1..]),
			)
		} else {
			node.children()
				.filter(|child| {
					child.has_tag_name("circle")
						|| child.has_tag_name("use")
						|| child.has_tag_name("g")
				})
				.map(|child| drawn_extent(document, child))
				.fold(0.0, f64::max)
		};
		scale_of(node) * reach
	}

	fn tube_marker_list<'a>(document: &'a Document) -> Vec<Node<'a, 'a>> {
		document
			.descendants()
			.filter(|node| node.has_tag_name("marker"))
			.collect()
	}

	#[test]
	fn flying_circles_are_half_the_legend_logo() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let legend = column(&document, "Rust")
			.descendants()
			.find(|node| node.has_tag_name("use"))
			.unwrap();
		let logo = drawn_radius(&document, legend);
		assert!((logo - TOKEN_RADIUS).abs() < 1e-9, "{logo}");
		let flight_list = flight_nodes(&document);
		assert!(flight_list.len() > 2, "{}", flight_list.len());
		for node in flight_list.into_iter().chain(tube_marker_list(&document)) {
			let radius = drawn_radius(&document, node);
			assert!((radius - logo / 2.0).abs() < 1e-9, "{node:?} {radius}");
		}
	}

	#[test]
	fn flying_circles_draw_in_front_of_bars() {
		for data in [fixture(), large()] {
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			let order: Vec<Node> = document.descendants().collect();
			let last_bar = order
				.iter()
				.rposition(|node| node.has_attribute("data-bar"))
				.unwrap();
			let first_flight = order
				.iter()
				.position(|node| node.has_attribute("data-flight"))
				.unwrap();
			assert!(first_flight > last_bar, "{first_flight} {last_bar}");
		}
	}

	#[test]
	fn tube_holds_what_fits() {
		let svg = render_languages(&large(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let tube = tagged(&document, "id", "tb").first_element_child().unwrap();
		let step = 2.0 * drawn_radius(&document, tube_marker_list(&document)[0]);
		let capacity = (number(tube, "height") / step).floor();
		let window = tagged(&document, "id", "tw").first_element_child().unwrap();
		let bottom = number(window, "y");
		let animation = window.first_element_child().unwrap();
		let peak = number_list(animation, "values")
			.iter()
			.map(|edge| ((bottom - edge) / step).round())
			.fold(0.0, f64::max);
		assert!((step - TOKEN_RADIUS).abs() < 1e-9, "{step}");
		assert!((peak - capacity).abs() < 1e-9, "{peak} {capacity}");
		assert!(capacity >= 17.0, "{capacity}");
	}

	#[test]
	fn flying_logos_are_never_scaled() {
		for data in [large(), wanderer(), swap()] {
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			assert!(
				document
					.descendants()
					.all(|node| node.attribute("type") != Some("scale"))
			);
			for node in flight_nodes(&document) {
				assert!(
					node.ancestors()
						.filter_map(|ancestor| ancestor.attribute("transform"))
						.all(|transform| !transform.contains("scale")),
					"{node:?}"
				);
				assert!(node.attribute("href").unwrap().starts_with("#h"));
			}
		}
	}

	#[test]
	fn first_week_burst_extends_lead() {
		let data = burst();
		let race = Race::new(&data, PACE);
		let clock = &race.clock;
		let first_frame = race
			.flight_list
			.iter()
			.filter(|flight| flight.frame == 0)
			.count();
		assert_eq!(first_frame, 8);
		let first_tick_ms = clock.ms(clock.lead);
		assert!(first_tick_ms > 1_600.0, "{first_tick_ms}");
		let earliest = race
			.flight_list
			.iter()
			.map(|flight| flight.enter_ms)
			.min()
			.unwrap();
		assert!(earliest < clock.tick_ms, "{earliest}");
		let svg = render_languages(&data, Variant::Dark, PACE);
		assert_valid_smil(&svg);
	}

	#[test]
	fn boundary_swap_feeds_entering_column_after_leaving_lands() {
		let data = swap();
		let race = Race::new(&data, PACE);
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let go = column(&document, "Go");
		let c = column(&document, "C");
		assert!(first_rise_ms(go).is_some());
		assert_eq!(fall_list(c), [(fall_list(c)[0].0, f64::INFINITY)]);
		let wanderer = wanderer();
		let svg = render_languages(&wanderer, Variant::Dark, PACE);
		let wandering = Document::parse(&svg).unwrap();
		let back = fall_list(column(&wandering, "C"));
		assert_eq!(back.len(), 2);
		assert!(back[0].1 > back[0].0 && back[0].1 < back[1].0);
		for (index, summary) in data.languages.iter().enumerate() {
			let column = column(&document, summary.language.name);
			let landing_list: Vec<f64> = race
				.flight_list
				.iter()
				.filter(|flight| flight.lane == index)
				.map(|flight| f64::from(flight.land_ms))
				.collect();
			for landing in &landing_list {
				assert!(shown_at(column, *landing), "{index} {landing}");
			}
			for (fall, next_rise) in fall_list(column) {
				assert!(
					landing_list
						.iter()
						.all(|landing| !(fall + 1.0..next_rise).contains(landing)),
					"{index} {fall}"
				);
			}
		}
	}

	#[test]
	fn simultaneous_shots_render_valid_smil() {
		let data = crowd();
		let race = Race::new(&data, 0.1);
		assert!(
			race.flight_list
				.windows(2)
				.any(|pair| pair[1].fire_ms == pair[0].fire_ms)
		);
		for data in [crowd(), fixture(), burst()] {
			for pace in [0.1, PACE] {
				assert_valid_smil(&render_languages(&data, Variant::Dark, pace));
			}
		}
	}

	#[test]
	fn fade_merges_blips_and_keeps_short_runs() {
		let blip = [
			Shown {
				on: 0.0,
				off: Some(300.0),
			},
			Shown {
				on: 500.0,
				off: None,
			},
		];
		assert_eq!(fade(&blip, 900.0), "");
		let gap = [
			Shown {
				on: 0.0,
				off: Some(300.0),
			},
			Shown {
				on: 600.0,
				off: None,
			},
		];
		let animation = fade(&gap, 1_000.0);
		assert!(animation.contains("values=\"1;1;0;0;1;1\""), "{animation}");
		assert!(
			animation.contains("keyTimes=\"0;.3;.55;.6;.85;1\""),
			"{animation}"
		);
		let short = [Shown {
			on: 100.0,
			off: Some(150.0),
		}];
		let animation = fade(&short, 1_000.0);
		assert!(animation.contains("values=\"0;0;1;1;0;0\""), "{animation}");
		assert!(
			animation.contains("keyTimes=\"0;.1;.35;.35;.6;1\""),
			"{animation}"
		);
		let late = [Shown {
			on: 0.0,
			off: Some(900.0),
		}];
		let animation = fade(&late, 1_000.0);
		assert!(animation.contains("keyTimes=\"0;.9;1;1\""), "{animation}");
	}

	#[test]
	fn belt_is_gone() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for node in document.descendants() {
			for attribute in node.attributes() {
				let mentions_belt = attribute.name().contains("belt")
					|| (["id", "class"].contains(&attribute.name())
						&& attribute.value().contains("belt"));
				assert!(!mentions_belt, "{node:?}");
			}
		}
		assert!(!svg.contains("url(#cb)"));
		assert!(!svg.contains("data-drop"));
		assert!(!svg.contains("stroke-dasharray=\"2 10\""));
	}

	#[test]
	fn slingshot_is_drawn_in_git_color() {
		for data in [fixture(), Timeline::default()] {
			let svg = render_languages(&data, Variant::Light, PACE);
			let document = Document::parse(&svg).unwrap();
			let sling_list: Vec<Node> = document
				.descendants()
				.filter(|node| node.has_tag_name("g") && node.has_attribute("data-slingshot"))
				.collect();
			assert_eq!(sling_list.len(), 1);
			let sling = sling_list[0];
			assert_eq!(sling.attribute("stroke"), Some("#e84e31"));
			for node in sling.descendants() {
				if let Some(stroke) = node.attribute("stroke") {
					assert_eq!(stroke, "#e84e31", "{node:?}");
				}
			}
			let mut ring_list: Vec<(f64, f64)> = sling
				.descendants()
				.filter(|node| node.has_tag_name("circle"))
				.map(|ring| (number(ring, "cx"), number(ring, "cy")))
				.collect();
			assert_eq!(ring_list.len(), 3);
			ring_list
				.sort_by(|left, right| left.1.total_cmp(&right.1).then(left.0.total_cmp(&right.0)));
			let (left_prong, right_prong, foot) = (ring_list[0], ring_list[1], ring_list[2]);
			assert!((left_prong.1 - right_prong.1).abs() < 0.01);
			assert!(left_prong.0 < right_prong.0);
			assert!(foot.1 > right_prong.1 + 40.0);
			assert!((foot.0 - right_prong.0).abs() < 0.01);
			assert!(foot.1 < GROUND);
		}
	}

	#[test]
	fn slingshot_is_at_most_half_as_tall() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let ring_list: Vec<(f64, f64)> = tagged(&document, "data-slingshot", "fork")
			.descendants()
			.filter(|node| node.has_tag_name("circle"))
			.map(|ring| (number(ring, "cx"), number(ring, "cy")))
			.collect();
		let foot = ring_list
			.iter()
			.copied()
			.max_by(|left, right| left.1.total_cmp(&right.1))
			.unwrap();
		let prong_list: Vec<(f64, f64)> = ring_list
			.iter()
			.copied()
			.filter(|ring| *ring != foot)
			.collect();
		assert_eq!(prong_list.len(), 2);
		assert!(
			(prong_list[0].0 - prong_list[1].0).abs() <= 22.0,
			"{prong_list:?}"
		);
		for prong in prong_list {
			assert!(foot.1 - prong.1 <= 47.0, "{prong:?} {foot:?}");
		}
		assert!((GROUND - foot.1).abs() <= 8.0, "{foot:?}");
	}

	#[test]
	fn fork_is_painted_over_flights_and_flights_over_columns() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let order: Vec<Node> = document.descendants().collect();
		let place = |node: Node| order.iter().position(|other| *other == node).unwrap();
		let fork = place(tagged(&document, "data-slingshot", "fork"));
		let flight_list: Vec<usize> = flight_nodes(&document).into_iter().map(place).collect();
		let last_column = document
			.descendants()
			.filter(|node| node.has_attribute("data-column"))
			.flat_map(|column| column.descendants())
			.map(place)
			.max()
			.unwrap();
		assert!(flight_list.len() > 5, "{}", flight_list.len());
		for flight in flight_list {
			assert!(
				last_column < flight && flight < fork,
				"{last_column} {flight} {fork}"
			);
		}
		assert!(place(tagged(&document, "data-band", "")) < fork);
	}

	#[test]
	fn idle_timelines_draw_a_static_slingshot() {
		let mut flat = single("Rust");
		let first = flat.frames[0].clone();
		for index in 1..4 {
			flat.frames.push(Frame {
				week: week(index),
				..first.clone()
			});
		}
		let blank = Timeline {
			frames: vec![Frame {
				entries: Vec::new(),
				..first
			}],
			..single("Rust")
		};
		for (data, flights) in [(Timeline::default(), 0), (blank, 0), (flat, 1)] {
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			assert!(svg.contains("data-slingshot"));
			assert!(!svg.contains("NaN"));
			let fired: usize = flight_nodes(&document)
				.into_iter()
				.map(|node| Pool::of(node).begin_list.len())
				.sum();
			assert_eq!(fired, flights);
		}
	}

	#[test]
	fn svg_is_well_formed() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let root = document.root_element();
		assert!(root.has_tag_name("svg"));
		assert_eq!(root.attribute("role"), Some("img"));
		assert!(root.attribute("viewBox").is_some());
		let title = root.children().find(|node| node.has_tag_name("title"));
		assert_eq!(title.unwrap().text(), Some("Languages over time"));
		let desc = root.children().find(|node| node.has_tag_name("desc"));
		let desc = desc.unwrap().text().unwrap();
		assert!(desc.contains("Rust 42.0%"), "{desc}");
		assert!(desc.contains("TypeScript 3.0%"), "{desc}");
		for hidden in ["other", "Shell", "Go "] {
			assert!(!desc.contains(hidden), "{desc}");
		}
	}

	#[test]
	fn final_bars_follow_the_sqrt_scale_in_final_order() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let mut bar_list: Vec<(f64, &str, f64)> = FINAL_ORDER
			.iter()
			.map(|name| {
				let rect = tagged(&document, "data-bar", name);
				let height = number(rect, "height") - TOP_RADIUS;
				(column_x(&document, name), *name, height)
			})
			.collect();
		bar_list.sort_by(|left, right| left.0.total_cmp(&right.0));
		let order: Vec<&str> = bar_list.iter().map(|bar| bar.1).collect();
		assert_eq!(order, FINAL_ORDER);
		assert!((bar_list[0].2 - CHART_HEIGHT).abs() < 0.01);
		let ratio = CHART_HEIGHT / FINAL_LINES[0].sqrt();
		for (bar, lines) in bar_list.iter().zip(FINAL_LINES) {
			let relative = (bar.2 / lines.sqrt() - ratio).abs() / ratio;
			assert!(relative < 5e-3, "{bar:?}");
		}
		assert!(shows_from_start(tagged(&document, "data-bar", "Rust")));
	}

	#[test]
	fn each_column_draws_only_its_live_bar() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		assert!(!svg.contains("data-ghost"));
		let document = Document::parse(&svg).unwrap();
		for name in FINAL_ORDER.iter().chain(&HIDDEN_AT_END) {
			let rect_list: Vec<Node> = column(&document, name)
				.descendants()
				.filter(|node| node.has_tag_name("rect"))
				.collect();
			assert_eq!(rect_list.len(), 1, "{name}");
			assert_eq!(rect_list[0].attribute("data-bar"), Some(*name));
			assert_eq!(rect_list[0].attribute("opacity"), None, "{name}");
		}
	}

	#[test]
	fn bars_are_flat_with_small_square_bottomed_top_radius() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		assert!(!svg.contains("linearGradient"));
		assert!(!svg.contains("url(#g)"));
		assert!(!svg.contains("href=\"#b"));
		let document = Document::parse(&svg).unwrap();
		let radius = num(TOP_RADIUS, 1);
		for name in FINAL_ORDER {
			let rect = tagged(&document, "data-bar", name);
			assert!(rect.has_tag_name("rect"), "{name}");
			assert_eq!(rect.attribute("rx"), Some(radius.as_str()), "{name}");
			let clip = rect
				.ancestors()
				.find_map(|node| node.attribute("clip-path"))
				.unwrap();
			let id = clip.trim_start_matches("url(#").trim_end_matches(')');
			let clip_rect = tagged(&document, "id", id).first_element_child().unwrap();
			let top = clip_rect.attribute("y").map_or(0.0, |y| y.parse().unwrap());
			let bottom = top + number(clip_rect, "height");
			assert!((bottom - GROUND).abs() < 0.01, "{name} {bottom}");
		}
		let rust = height_list(&document, "Rust");
		assert!((rust[0] - TOP_RADIUS).abs() < 0.01, "{}", rust[0]);
	}

	#[test]
	fn legend_visible_from_first_frame() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in ["Rust", "C"] {
			let column = column(&document, name);
			assert_one_legend_group(column);
			assert!(shows_from_start(column), "{name}");
			assert!((legend_opacity(column, 0.0) - 1.0).abs() < 1e-9, "{name}");
		}
	}

	#[test]
	fn column_enters_when_first_within_top_n() {
		let data = fixture();
		let clock = Race::new(&data, PACE).clock;
		let visible = grid(&data, &clock);
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for (summary, cells) in data.languages.iter().zip(&visible) {
			let name = summary.language.name;
			let column = column(&document, name);
			let first = cells.iter().position(|cell| cell.visible).unwrap();
			let Some(rise) = first_rise_ms(column) else {
				assert_eq!(first, 0, "{name}");
				assert_eq!(column.attribute("opacity"), None, "{name}");
				continue;
			};
			let entry_tick = clock.lead + first;
			assert!(first > 0, "{name}");
			assert!(rise < clock.ms(entry_tick), "{name} {rise}");
			let x_list = translate_list(column);
			assert!(
				x_list[..entry_tick]
					.iter()
					.all(|x| (*x - x_list[entry_tick]).abs() < 0.01),
				"{name}"
			);
		}
		assert!(first_rise_ms(column(&document, "Python")).is_some());
		let nix = data
			.languages
			.iter()
			.position(|summary| summary.language.name == "Nix")
			.unwrap();
		let present = visible[nix].iter().position(|cell| cell.present).unwrap();
		let shown = visible[nix].iter().position(|cell| cell.visible).unwrap();
		assert!(shown > present);
	}

	#[test]
	fn columns_falling_below_top_n_fade_out_and_end_hidden() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in HIDDEN_AT_END {
			let column = column(&document, name);
			assert_eq!(column.attribute("opacity"), Some("0"), "{name}");
			assert_eq!(opacity_steps(column), [0.0, 1.0, 0.0], "{name}");
		}
		let shown_at_end = document
			.descendants()
			.filter(|node| node.attribute("data-column").is_some())
			.filter(|node| node.attribute("opacity").is_none())
			.count();
		assert_eq!(shown_at_end, FINAL_ORDER.len());
	}

	#[test]
	fn column_reenters_after_dropping_below_top_n() {
		let data = wanderer();
		let race = Race::new(&data, PACE);
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let c = column(&document, "C");
		assert_eq!(opacity_steps(c), [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(c.attribute("opacity"), Some("0"));
		let go = column(&document, "Go");
		assert_eq!(opacity_steps(go), [0.0, 1.0, 0.0, 1.0]);
		assert_eq!(go.attribute("opacity"), None);
		let c_texts = text_list(c);
		assert!(c_texts.iter().any(|text| text == "2020"), "{c_texts:?}");
		let point_list = fade_points(c);
		assert!(point_list.windows(2).all(|pair| pair[0].0 <= pair[1].0));
		let c_index = data
			.languages
			.iter()
			.position(|summary| summary.language.name == "C")
			.unwrap();
		let cells = &grid(&data, &race.clock)[c_index];
		let c_flights: Vec<&Flight> = race
			.flight_list
			.iter()
			.filter(|flight| flight.lane == c_index)
			.collect();
		assert!(c_flights.len() >= 2);
		for flight in c_flights {
			assert!(cells[flight.frame].visible, "{}", flight.frame);
			assert!(shown_at(c, f64::from(flight.land_ms)), "{}", flight.land_ms);
		}
	}

	#[test]
	fn column_moves_as_one() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in FINAL_ORDER.iter().chain(&HIDDEN_AT_END) {
			let column = column(&document, name);
			assert!(has_slide(column), "{name}");
			let name_text = column
				.descendants()
				.find(|node| node.has_tag_name("text") && node.text() == Some(name))
				.unwrap();
			let member_list = [tagged(&document, "data-bar", name), name_text];
			for member in member_list {
				let mover = member.ancestors().find(|node| has_slide(*node)).unwrap();
				assert_eq!(mover, column, "{name}");
			}
		}
		assert_eq!(
			document
				.descendants()
				.filter(|node| has_slide(*node))
				.filter(|node| node
					.children()
					.all(|child| !child.has_attribute("data-flight")))
				.count(),
			data.languages.len()
		);
	}

	#[test]
	fn bar_height_never_decreases() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in FINAL_ORDER {
			let heights = height_list(&document, name);
			assert!(heights.windows(2).all(|pair| pair[0] <= pair[1]), "{name}");
			let final_height = number(tagged(&document, "data-bar", name), "height");
			assert!(
				(heights.last().unwrap() - final_height).abs() < 0.1,
				"{name}"
			);
		}
		let python = height_list(&document, "Python");
		let first_shown = python.iter().find(|height| **height > TOP_RADIUS).unwrap();
		assert!(*first_shown >= TOP_RADIUS + 2.0);
	}

	#[test]
	fn columns_reorder_when_language_overtakes() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let first_slot = column_x(&document, "Rust");
		let second_slot = column_x(&document, "C");
		assert!(
			FINAL_ORDER
				.iter()
				.all(|name| column_x(&document, name) >= first_slot)
		);
		let rust = translate_list(column(&document, "Rust"));
		let c = translate_list(column(&document, "C"));
		assert!((rust.last().unwrap() - first_slot).abs() < 1.0);
		assert!((c.last().unwrap() - second_slot).abs() < 1.0);
		assert!(rust.iter().any(|x| (x - second_slot).abs() < 1.0));
		assert!(c.iter().any(|x| (x - first_slot).abs() < 1.0));
	}

	#[test]
	fn hidden_columns_park_on_the_last_slot() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let last_slot = column_x(&document, "TypeScript");
		for name in HIDDEN_AT_END {
			let x_list = translate_list(column(&document, name));
			assert!((x_list.last().unwrap() - last_slot).abs() < 1.0, "{name}");
		}
	}

	#[test]
	fn labels_show_share_lines_repos_since_trend() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let has = |list: &[String], expected: &str| list.iter().any(|text| text == expected);
		let rust = text_list(column(&document, "Rust"));
		for expected in [
			"Rust",
			"21.9%",
			"42.0%",
			"28",
			"4.2k",
			"1",
			"3",
			"2020",
			"\u{2192} 1.4k",
		] {
			assert!(has(&rust, expected), "{expected} in {rust:?}");
		}
		let nix = text_list(column(&document, "Nix"));
		assert!(has(&nix, "2021") && has(&nix, "\u{2191} 420"), "{nix:?}");
		let go = column(&document, "Go");
		assert_eq!(shown_text(go, "cl", LINES_ROW), "80");
		assert_eq!(shown_text(go, "cr", REPOS_ROW), "0");
		let go_texts = text_list(go);
		assert!(
			has(&go_texts, "2020") && has(&go_texts, "\u{2193} 2"),
			"{go_texts:?}"
		);
		let root = text_list(document.root());
		assert!(has(&root, "2022-11"));
		for header in HEADER_LIST {
			let count = root.iter().filter(|text| *text == header).count();
			assert_eq!(count, 1, "{header}");
		}
	}

	#[test]
	fn totals_tick_under_the_date_and_end_on_final_totals() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let root = document.root_element();
		assert_eq!(shown_text(root, "cd", DATE_ROW), "2022-11");
		assert_eq!(
			shown_text(root, "ct", TOTALS_ROW),
			"7 languages - 10k lines"
		);
		let totals = root
			.descendants()
			.find(|node| node.attribute("clip-path") == Some("url(#ct)"))
			.unwrap();
		let state_list = text_list(totals);
		assert!(state_list.len() > 10, "{state_list:?}");
		assert_eq!(state_list[0], "2 languages - 128 lines");
		assert!(
			state_list
				.iter()
				.any(|text| text.starts_with("5 languages"))
		);
		let date = root
			.descendants()
			.find(|node| node.attribute("clip-path") == Some("url(#cd)"))
			.unwrap();
		assert_eq!(totals.parent(), date.parent());
		const {
			assert!(TOTALS_ROW - DATE_ROW >= TOTALS_STEP);
			assert!(TOTALS_ROW < GROUND - CHART_HEIGHT - TOP_RADIUS);
		}
	}

	#[test]
	fn final_state_has_one_text_set_per_language() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		assert!(!svg.contains("data-live"));
		for name in FINAL_ORDER {
			let named = document
				.descendants()
				.filter(|node| node.has_tag_name("text") && node.text() == Some(name))
				.count();
			assert_eq!(named, 1, "{name}");
		}
		assert_eq!(svg.matches("url(#cs)").count(), data.languages.len());
		let rust = column(&document, "Rust");
		assert_eq!(shown_text(rust, "cs", SHARE_ROW), "42.0%");
		assert_eq!(shown_text(rust, "cl", LINES_ROW), "4.2k");
		assert_eq!(shown_text(rust, "cr", REPOS_ROW), "3");
		let typescript = column(&document, "TypeScript");
		assert_eq!(shown_text(typescript, "cs", SHARE_ROW), "3.0%");
	}

	#[test]
	fn sixteen_columns_fit_without_overlap() {
		let data = large();
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let shown: Vec<Node> = document
			.descendants()
			.filter(|node| node.attribute("data-column").is_some())
			.filter(|node| node.attribute("opacity").is_none())
			.collect();
		assert_eq!(shown.len(), 16);
		let layout = Layout::new(16);
		assert!(layout.slot >= 48.0, "{}", layout.slot);
		assert!(layout.bar_width() < layout.slot - 8.0);
		let mut x_list: Vec<f64> = shown
			.iter()
			.map(|node| column_x(&document, node.attribute("data-column").unwrap()))
			.collect();
		x_list.sort_by(f64::total_cmp);
		assert!(
			x_list
				.windows(2)
				.all(|pair| (pair[1] - pair[0] - layout.slot).abs() <= 1.0)
		);
		assert!(x_list[0] - layout.slot / 2.0 >= GUTTER - 1.0);
		assert!(x_list[15] + layout.slot / 2.0 <= WIDTH);
		let room = layout.slot - LABEL_PAD;
		for node in &shown {
			let name = node.attribute("data-column").unwrap();
			let label = node
				.descendants()
				.find(|child| child.has_tag_name("text") && child.text() == Some(name))
				.unwrap();
			let drawn = label
				.attribute("textLength")
				.map_or_else(|| name_width(name), |length| length.parse().unwrap());
			assert!(drawn <= room, "{name} {drawn}");
			for (clip, row) in [("cs", SHARE_ROW), ("cl", LINES_ROW), ("cr", REPOS_ROW)] {
				let value = shown_text(*node, clip, row);
				assert!(value_width(&value) <= room, "{name} {value}");
			}
		}
		let javascript = column(&document, "JavaScript");
		let label = javascript
			.descendants()
			.find(|child| child.text() == Some("JavaScript"))
			.unwrap();
		assert_eq!(label.attribute("lengthAdjust"), Some("spacingAndGlyphs"));
		let header = document
			.descendants()
			.find(|node| node.has_tag_name("text") && node.text() == Some("past year"))
			.unwrap();
		let header_x = number(header, "x");
		assert!(header_x <= x_list[0] - layout.slot / 2.0, "{header_x}");
	}

	const SPACE_PX: f64 = 2.59;
	const NAME_GAP_PX: f64 = 2.0 * SPACE_PX;
	const RENDERED_NAME_PX: [(&str, f64); 30] = [
		("Rust", 22.48),
		("C", 6.38),
		("C++", 17.81),
		("Nix", 16.97),
		("Python", 35.64),
		("TypeScript", 52.44),
		("JavaScript", 50.0),
		("Shell", 24.09),
		("Makefile", 43.47),
		("Dockerfile", 51.27),
		("Go", 13.44),
		("Java", 21.09),
		("Lua", 18.27),
		("HTML", 28.53),
		("CSS", 17.39),
		("SCSS", 22.91),
		("OpenSCAD", 52.95),
		("GLSL", 24.06),
		("Boo", 19.11),
		("Haskell", 36.88),
		("Assembly", 47.64),
		("CMake", 33.95),
		("Vim script", 50.34),
		("Markdown", 53.67),
		("RON", 22.7),
		("PHP", 20.22),
		("Jupyter Notebook", 88.58),
		("PowerShell", 55.38),
		("WebAssembly", 69.36),
		("Mathematica", 66.73),
	];

	const NOTO_GLYPH_CENTI_PX: [u16; 95] = [
		259, 286, 472, 645, 572, 900, 750, 266, 339, 339, 544, 572, 284, 322, 284, 413, 572, 572,
		572, 572, 572, 572, 572, 572, 572, 572, 284, 284, 572, 572, 572, 477, 897, 689, 672, 636,
		739, 559, 548, 723, 764, 389, 331, 664, 564, 942, 813, 795, 628, 795, 659, 550, 578, 756,
		650, 967, 667, 623, 578, 331, 413, 331, 572, 411, 361, 603, 633, 514, 633, 591, 386, 633,
		656, 305, 305, 619, 305, 981, 656, 619, 633, 633, 453, 497, 433, 656, 569, 856, 578, 569,
		488, 394, 550, 394, 572,
	];

	#[test]
	fn every_glyph_estimate_covers_its_rendered_width() {
		for (glyph, centi) in (' '..='~').zip(NOTO_GLYPH_CENTI_PX) {
			let rendered = f64::from(centi) / 100.0;
			let estimate = glyph_em(glyph) * NAME_FONT_PX;
			assert!(estimate >= rendered, "{glyph:?} {estimate} < {rendered}");
		}
	}

	fn dip_clock() -> Clock {
		Clock::new(40, 3_900, 0)
	}

	#[test]
	fn fade_ending_on_a_tick_stops_counting_one_tick_later() {
		let clock = dip_clock();
		let off = 10.0f64.mul_add(f64::from(clock.tick_ms), -FADE_MS);
		let on_tick = shown_ticks(
			&[Shown {
				on: 0.0,
				off: Some(off),
			}],
			&clock,
		);
		assert!(on_tick[10] && !on_tick[11], "{on_tick:?}");
		let past_tick = shown_ticks(
			&[Shown {
				on: 0.0,
				off: Some(off + 1.0),
			}],
			&clock,
		);
		assert!(past_tick[11] && !past_tick[12], "{past_tick:?}");
	}

	fn dip_animation(crossing: &[(usize, usize)]) -> String {
		wrapped(&dip(crossing, &dip_clock()))
	}

	fn dip_at(document: &Document, tick: usize) -> f64 {
		let animation = document.root_element().first_element_child().unwrap();
		linear_at(animation, dip_clock().ms(tick))
	}

	fn assert_keys_increase(animation: Node) {
		let key_list = number_list(animation, "keyTimes");
		assert!(
			key_list.windows(2).all(|pair| pair[0] < pair[1]),
			"{key_list:?}"
		);
	}

	#[test]
	fn dip_ramps_over_the_tick_before_and_after_a_run() {
		let svg = dip_animation(&[(10, 14)]);
		let document = Document::parse(&svg).unwrap();
		assert!((dip_at(&document, 9) - 1.0).abs() < 1e-3);
		assert!((dip_at(&document, 10) - LEGEND_DIP).abs() < 1e-3);
		assert!((dip_at(&document, 15) - LEGEND_DIP).abs() < 1e-3);
		assert!((dip_at(&document, 16) - 1.0).abs() < 1e-3);
	}

	#[test]
	fn dip_key_times_strictly_increase() {
		let last = dip_clock().ticks() - 1;
		assert_eq!(dip(&[(last, last)], &dip_clock()), "");
		let crossing_cases: [&[(usize, usize)]; 6] = [
			&[(0, 2)],
			&[(1, 2)],
			&[(5, 6), (7, 8), (10, 11), (13, 14), (17, 18)],
			&[(20, last - 1)],
			&[(20, 22), (last - 1, last)],
			&[(20, 22), (last, last)],
		];
		for crossing in crossing_cases {
			let svg = dip_animation(crossing);
			let document = Document::parse(&svg).unwrap();
			assert_keys_increase(document.root_element().first_element_child().unwrap());
			assert!((dip_at(&document, last) - 1.0).abs() < 1e-3, "{crossing:?}");
		}
		let svg = dip_animation(&[(0, 2)]);
		let starts_low = Document::parse(&svg).unwrap();
		assert!((dip_at(&starts_low, 0) - LEGEND_DIP).abs() < 1e-3);
	}

	#[test]
	fn name_estimate_covers_rendered_width() {
		for (name, rendered) in RENDERED_NAME_PX {
			let estimate = name_width(name);
			assert!(estimate >= rendered, "{name} {estimate} < {rendered}");
			assert!(
				estimate <= rendered * 1.25,
				"{name} {estimate} >> {rendered}"
			);
		}
	}

	fn drawn_name_width(column: Node) -> f64 {
		let label = name_text(column);
		label.attribute("textLength").map_or_else(
			|| name_width(label.text().unwrap()),
			|length| length.parse().unwrap(),
		)
	}

	#[test]
	fn names_never_touch_at_rest() {
		for data in [large(), real_like()] {
			let slot = Race::new(&data, PACE).layout.slot;
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			let column_list: Vec<Node> = document
				.descendants()
				.filter(|node| node.attribute("data-column").is_some())
				.collect();
			for column in &column_list {
				let drawn = drawn_name_width(*column);
				assert!(drawn <= slot - NAME_GAP_PX, "{drawn}");
			}
			let mut placed: Vec<(f64, f64)> = column_list
				.iter()
				.filter(|column| column.attribute("opacity").is_none())
				.map(|column| {
					(
						column_x(&document, column.attribute("data-column").unwrap()),
						drawn_name_width(*column),
					)
				})
				.collect();
			placed.sort_by(|left, right| left.0.total_cmp(&right.0));
			assert!(placed.len() > 10, "{}", placed.len());
			for pair in placed.windows(2) {
				let gap = (pair[1].0 - pair[0].0) - f64::midpoint(pair[0].1, pair[1].1);
				assert!(gap >= NAME_GAP_PX, "{pair:?} {gap}");
			}
		}
	}

	fn leap() -> Timeline {
		let mut usage = Usage::default();
		spread(&mut usage, "Rust", 0..40, 40);
		spread(&mut usage, "C", 0..40, 30);
		spread(&mut usage, "Python", 0..40, 20);
		spread(&mut usage, "Go", 0..40, 10);
		spread(&mut usage, "Go", 20..21, 2000);
		timeline(&usage, 4, 520)
	}

	fn opacity_at(node: Node, ms: f64) -> f64 {
		node.children()
			.find(|child| child.attribute("attributeName") == Some("opacity"))
			.map_or(1.0, |fade| linear_at(fade, ms))
	}

	fn name_text<'a>(column: Node<'a, 'a>) -> Node<'a, 'a> {
		let name = column.attribute("data-column").unwrap();
		column
			.descendants()
			.find(|node| node.has_tag_name("text") && node.text() == Some(name))
			.unwrap()
	}

	fn legend_opacity(column: Node, ms: f64) -> f64 {
		name_text(column)
			.ancestors()
			.filter(Node::is_element)
			.map(|ancestor| opacity_at(ancestor, ms))
			.product()
	}

	fn assert_one_legend_group<'a>(column: Node<'a, 'a>) -> Node<'a, 'a> {
		let legend = name_text(column)
			.ancestors()
			.find(|ancestor| ancestor.parent() == Some(column))
			.unwrap();
		for node in column.descendants() {
			if node.has_tag_name("text") || node.has_tag_name("use") {
				assert!(node.ancestors().any(|ancestor| ancestor == legend));
			}
		}
		legend
	}

	struct Tracks<'a> {
		column_list: Vec<Node<'a, 'a>>,
		x_list: Vec<Vec<f64>>,
		tick_ms: f64,
		slot: f64,
	}

	impl<'a> Tracks<'a> {
		fn of(document: &'a Document, slot: f64) -> Self {
			let column_list: Vec<Node> = document
				.descendants()
				.filter(|node| node.attribute("data-column").is_some())
				.collect();
			let x_list: Vec<Vec<f64>> = column_list
				.iter()
				.map(|column| translate_list(*column))
				.collect();
			let slide = column_list[0]
				.children()
				.find(|child| child.attribute("type") == Some("translate"))
				.unwrap();
			let tick_ms = ms(slide, "dur") / float(x_list[0].len() - 1);
			Self {
				column_list,
				x_list,
				tick_ms,
				slot,
			}
		}

		fn ticks(&self) -> usize {
			self.x_list[0].len()
		}

		fn x_at_half_tick(&self, lane: usize, half_tick: usize) -> f64 {
			let track = &self.x_list[lane];
			f64::midpoint(track[half_tick / 2], track[half_tick.div_ceil(2)])
		}

		fn close(&self, x_list: &[f64], left: usize, right: usize) -> bool {
			(x_list[left] - x_list[right]).abs() < self.slot - 0.01
		}

		fn calm_at(&self, tick: usize) -> bool {
			let same = |left: f64, right: f64| (left - right).abs() < 1e-9;
			(0..self.column_list.len()).all(|lane| {
				let mut window = tick - 2..=tick + 2;
				let fade_list: Vec<f64> = window
					.clone()
					.map(|at| opacity_at(self.column_list[lane], float(at) * self.tick_ms))
					.collect();
				window.all(|at| same(self.x_list[lane][at], self.x_list[lane][tick]))
					&& fade_list.iter().all(|fade| same(*fade, fade_list[0]))
					&& (same(fade_list[0], 0.0) || same(fade_list[0], 1.0))
			})
		}
	}

	#[derive(Default)]
	struct Seen {
		crossing: usize,
		dipped: usize,
		spared: usize,
		rest: usize,
	}

	fn assert_labels_never_overlap(data: &Timeline, pace: f64) -> Seen {
		let slot = Race::new(data, pace).layout.slot;
		let widest_label = slot - LABEL_PAD;
		let svg = render_languages(data, Variant::Dark, pace);
		let document = Document::parse(&svg).unwrap();
		let tracks = Tracks::of(&document, slot);
		let lanes = tracks.column_list.len();
		for column in &tracks.column_list {
			let legend = assert_one_legend_group(*column);
			if let Some(dip) = legend
				.children()
				.find(|child| child.attribute("attributeName") == Some("opacity"))
			{
				assert_keys_increase(dip);
			}
		}
		let mut seen = Seen::default();
		for half_tick in 0..2 * (tracks.ticks() - 1) {
			let at = float(half_tick) * tracks.tick_ms / 2.0;
			let x_list: Vec<f64> = (0..lanes)
				.map(|lane| tracks.x_at_half_tick(lane, half_tick))
				.collect();
			let opacity_list: Vec<f64> = tracks
				.column_list
				.iter()
				.map(|column| legend_opacity(*column, at))
				.collect();
			for left in 0..lanes {
				for right in left + 1..lanes {
					if !tracks.close(&x_list, left, right) {
						continue;
					}
					let apart = (x_list[left] - x_list[right]).abs() >= widest_label;
					let lower = opacity_list[left].min(opacity_list[right]);
					assert!(lower <= 0.15 || apart, "{at} {left} {right} {lower}");
					let both_shown = opacity_at(tracks.column_list[left], at) > 0.0
						&& opacity_at(tracks.column_list[right], at) > 0.0;
					seen.crossing += usize::from(both_shown);
					seen.spared += usize::from(both_shown && apart && lower > 0.99);
					seen.dipped += usize::from(both_shown && !apart && lower <= 0.15);
				}
			}
		}
		for tick in 2..tracks.ticks() - 2 {
			if !tracks.calm_at(tick) {
				continue;
			}
			let at = float(tick) * tracks.tick_ms;
			let shown: Vec<usize> = (0..lanes)
				.filter(|lane| opacity_at(tracks.column_list[*lane], at) > 0.0)
				.collect();
			let x_list: Vec<f64> = (0..lanes)
				.map(|lane| tracks.x_at_half_tick(lane, 2 * tick))
				.collect();
			let apart = shown.iter().all(|left| {
				shown
					.iter()
					.all(|right| left == right || !tracks.close(&x_list, *left, *right))
			});
			if !apart {
				continue;
			}
			for lane in shown {
				let opacity = legend_opacity(tracks.column_list[lane], at);
				assert!((opacity - 1.0).abs() < 1e-3, "{at} {lane} {opacity}");
				seen.rest += 1;
			}
		}
		seen
	}

	#[test]
	fn crossing_columns_do_not_overlap_labels() {
		let data = leap();
		let race = Race::new(&data, PACE);
		let go = race.lanes.iter().find(|lane| lane.name() == "Go").unwrap();
		assert_eq!(go.x.first().copied(), Some(race.layout.center(3)));
		assert_eq!(go.x.last().copied(), Some(race.layout.center(0)));
		let mut spared = 0;
		for (data, pace) in paced().chain([(leap(), PACE), (leap(), MIN_SECONDS_PER_YEAR)]) {
			let seen = assert_labels_never_overlap(&data, pace);
			assert!(seen.crossing > 0 && seen.dipped > 0 && seen.rest > 0);
			spared += seen.spared;
		}
		assert!(spared > 0);
		assert!(assert_labels_never_overlap(&large(), PACE).dipped > 0);
	}

	#[test]
	fn uses_exact_language_colors() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Light, PACE);
		let document = Document::parse(&svg).unwrap();
		for summary in &data.languages {
			let name = summary.language.name;
			let color = Some(summary.language.color);
			assert_eq!(tagged(&document, "data-bar", name).attribute("fill"), color);
		}
		assert!(svg.contains("#3572A5"));
	}

	#[test]
	fn language_without_linguist_color_renders_grey_with_letter_logo() {
		let language = crate::language::language_by_name("Linker Script").unwrap();
		let logo = token(3, language);
		assert!(logo.contains("fill=\"#cccccc\""), "{logo}");
		assert!(logo.contains(">L</text>"), "{logo}");
	}

	#[test]
	fn no_external_references() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		assert_eq!(svg.matches("http").count(), 1);
		assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
		assert!(!svg.contains("<image"));
		assert!(!svg.contains("@import"));
		assert!(
			svg.match_indices("url(")
				.all(|(index, _)| svg[index + 4..].starts_with('#'))
		);
		let document = Document::parse(&svg).unwrap();
		for node in document.descendants() {
			if let Some(href) = node.attribute("href") {
				assert!(href.starts_with('#'), "{href}");
			}
		}
	}

	#[test]
	fn no_full_size_background() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let root = document.root_element();
		assert_eq!(root.attribute("style"), None);
		assert_eq!(root.attribute("fill"), None);
		let view_box: Vec<f64> = root
			.attribute("viewBox")
			.unwrap()
			.split(' ')
			.map(|value| value.parse().unwrap())
			.collect();
		let drawn = document.descendants().filter(|node| {
			node.has_tag_name("rect") && !node.ancestors().any(|parent| parent.has_tag_name("defs"))
		});
		for rect in drawn {
			let size = |name| {
				rect.attribute(name)
					.map_or(0.0, |value: &str| value.parse().unwrap())
			};
			assert!(
				size("width") < view_box[2] * 0.9 || size("height") < view_box[3] * 0.5,
				"{rect:?}"
			);
		}
	}

	fn real_like() -> Timeline {
		let mut usage = Usage::default();
		for (offset, name) in (0_i64..).zip(LARGE_NAME_LIST.iter().take(16)) {
			let period = 4 + offset % 5;
			for index in offset * 12..320 {
				if (index * 7 + offset * 5) % period == 0 {
					let lines = u64::try_from(40 + (index * 31 + offset * 17) % 900).unwrap();
					spread(&mut usage, name, index..index + 1, lines);
				}
			}
			usage.first_use.insert(name, vec![week(offset * 12)]);
		}
		timeline(&usage, 16, 520)
	}

	#[test]
	fn dense_history_with_effects_under_1mb() {
		let data = large();
		assert!(data.languages.len() >= 16);
		assert_eq!(data.frames.len(), 520);
		for variant in [Variant::Dark, Variant::Light] {
			let svg = render_languages(&data, variant, PACE);
			assert!(svg.len() < 1_000_000, "{}", svg.len());
			assert!(svg.contains("data-flames"));
			assert!(Document::parse(&svg).is_ok());
		}
	}

	#[test]
	fn real_like_history_stays_under_300kb() {
		let data = real_like();
		let race = Race::new(&data, PACE);
		assert!(race.flight_list.len() > 500, "{}", race.flight_list.len());
		for variant in [Variant::Dark, Variant::Light] {
			let svg = render_languages(&data, variant, PACE);
			assert!(svg.len() <= 300_000, "{}", svg.len());
		}
	}

	struct Steps {
		key_list: Vec<f64>,
		value_list: Vec<f64>,
	}

	impl Steps {
		fn of(animation: Node, pick: fn(&str) -> f64) -> Self {
			let duration = ms(animation, "dur");
			Self {
				key_list: number_list(animation, "keyTimes")
					.into_iter()
					.map(|key| key * duration)
					.collect(),
				value_list: animation
					.attribute("values")
					.unwrap()
					.split(';')
					.map(pick)
					.collect(),
			}
		}

		fn at(&self, at: f64) -> f64 {
			let index = self.key_list.partition_point(|key| *key <= at + 0.01);
			self.value_list[index.max(1) - 1]
		}
	}

	fn tube_box(document: &Document) -> (f64, f64) {
		let rect = tagged(document, "id", "tb").first_element_child().unwrap();
		let top = number(rect, "y");
		(top, top + number(rect, "height"))
	}

	#[test]
	fn tube_never_shows_more_than_fits() {
		for (data, least_drops) in [(large(), 0), (real_like(), 20)] {
			let svg = render_languages(&data, Variant::Dark, PACE);
			let document = Document::parse(&svg).unwrap();
			let (top, bottom) = tube_box(&document);
			let radius = drawn_extent(&document, tube_marker_list(&document)[0]);
			assert!((radius - FLIGHT_RADIUS).abs() < 1e-9, "{radius}");
			let capacity = ((bottom - top) / (2.0 * radius)).floor();
			let window = Steps::of(
				tagged(&document, "id", "tw")
					.first_element_child()
					.unwrap()
					.first_element_child()
					.unwrap(),
				|value| value.parse().unwrap(),
			);
			let step_node = document
				.descendants()
				.find(|node| node.has_attribute("data-step"))
				.unwrap();
			let step = Steps::of(step_node.first_element_child().unwrap(), |value| {
				value.split(' ').nth(1).unwrap().parse().unwrap()
			});
			let mut vertex_y: Vec<f64> = step_node
				.descendants()
				.filter(|node| node.has_attribute("marker-mid"))
				.flat_map(|tape| {
					let mut list = vertex_list(tape.attribute("d").unwrap());
					list.pop();
					list
				})
				.map(|vertex| vertex.1)
				.collect();
			vertex_y.sort_by(f64::total_cmp);
			let mut instant_list: Vec<f64> = window
				.key_list
				.iter()
				.chain(&step.key_list)
				.copied()
				.collect();
			instant_list.sort_by(f64::total_cmp);
			let mut peak = 0.0;
			for at in instant_list {
				let (edge, offset) = (window.at(at), step.at(at));
				let shown = vertex_y
					.iter()
					.map(|y| y + offset)
					.filter(|y| y + radius > edge + 0.01 && y - radius < bottom - 0.01)
					.inspect(|y| {
						assert!(
							*y - radius >= edge - 0.01 && *y + radius <= bottom + 0.01,
							"{at} {y}"
						);
					})
					.count();
				let shown = float(shown);
				assert!(shown <= capacity, "{at} {shown}");
				peak = f64::max(peak, shown);
			}
			assert!((peak - capacity).abs() < 1e-9, "{peak}");
			let drop_list: Vec<Node> = document
				.descendants()
				.filter(|node| node.has_attribute("data-entry"))
				.collect();
			let mut dropped = 0;
			for node in drop_list {
				for begin in drop_begins(node) {
					dropped += 1;
					for at in (0..=400).map(|step| begin + f64::from(step)) {
						let y = drop_at(node, begin, at);
						let edge = window.at(at);
						assert!(
							y + radius <= edge + 0.5 || y - radius >= edge - 0.5,
							"{begin} {at} {y} {edge}"
						);
					}
				}
			}
			assert!(dropped >= least_drops, "{dropped}");
		}
	}

	#[test]
	fn effects_end_calm() {
		for (data, colors) in [
			(real_like(), palette(Variant::Dark)),
			(burst(), palette(Variant::Light)),
		] {
			let variant = if colors == palette(Variant::Dark) {
				Variant::Dark
			} else {
				Variant::Light
			};
			let svg = render_languages(&data, variant, PACE);
			let document = Document::parse(&svg).unwrap();
			let rig = document
				.descendants()
				.find(|node| node.has_attribute("data-rig"))
				.unwrap();
			assert!(rig.attribute("transform").is_none());
			for key in ["data-glow", "data-sparks", "data-flames", "data-bolt"] {
				let node = rig
					.descendants()
					.find(|node| node.has_attribute(key))
					.unwrap();
				assert_eq!(node.attribute("opacity"), Some("0"), "{key}");
			}
			assert_eq!(tagged(&document, "id", "e").attribute("opacity"), Some("0"));
			let mut animated = 0;
			for node in document.descendants() {
				let last = node
					.attribute("values")
					.and_then(|values| values.rsplit(';').next());
				match node.attribute("attributeName") {
					Some("opacity")
						if node.ancestors().any(|up| {
							up.has_attribute("data-rig") || up.attribute("id") == Some("e")
						}) =>
					{
						assert_eq!(last, Some("0"), "{node:?}");
						animated += 1;
					}
					Some("stroke") => assert_eq!(last, Some(colors.muted), "{node:?}"),
					Some("transform")
						if node
							.attribute("begin")
							.is_some_and(|begin| begin.ends_with("ms")) =>
					{
						assert_eq!(last, Some("0"), "{node:?}");
					}
					_ => {}
				}
			}
			assert!(animated >= 5, "{animated}");
			let (_, bottom) = tube_box(&document);
			let window = tagged(&document, "id", "tw").first_element_child().unwrap();
			assert!((number(window, "y") - bottom).abs() < 1e-9);
			let step_node = document
				.descendants()
				.find(|node| node.has_attribute("data-step"))
				.unwrap();
			let resting: f64 = step_node
				.attribute("transform")
				.unwrap()
				.trim_end_matches(')')
				.rsplit(' ')
				.next()
				.unwrap()
				.parse()
				.unwrap();
			let radius = drawn_extent(&document, tube_marker_list(&document)[0]);
			for tape in step_node
				.descendants()
				.filter(|node| node.has_attribute("marker-mid"))
			{
				for vertex in vertex_list(tape.attribute("d").unwrap()) {
					assert!(vertex.1 + resting - radius >= bottom - 1e-9);
				}
			}
			for node in document
				.descendants()
				.filter(|node| node.has_attribute("data-entry"))
			{
				assert!(number(node, "y") - drawn_extent(&document, node) > bottom);
			}
		}
	}

	#[test]
	fn heat_reaches_the_feeder() {
		let data = burst();
		let race = Race::new(&data, PACE);
		let earliest = race
			.flight_list
			.iter()
			.map(|flight| flight.enter_ms)
			.min()
			.unwrap();
		assert!(
			race.heat.start_ms >= earliest && race.heat.start_ms <= earliest + 20,
			"{}",
			race.heat.start_ms
		);
		assert!(race.heat.sample_list.iter().any(|heat| *heat > 900));
		let svg = render_languages(&data, Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let flames = document
			.descendants()
			.find(|node| node.has_attribute("data-flames"))
			.unwrap();
		let fade = flames.first_element_child().unwrap();
		assert!(number_list(fade, "values").iter().any(|value| *value > 0.5));
		assert_eq!(svg, render_languages(&data, Variant::Dark, PACE));
	}

	#[test]
	fn light_and_dark_differ_only_in_palette() {
		let data = fixture();
		let dark = render_languages(&data, Variant::Dark, PACE);
		let light = render_languages(&data, Variant::Light, PACE);
		assert_ne!(dark, light);
		let pair_list = color_list(palette(Variant::Dark))
			.into_iter()
			.zip(color_list(palette(Variant::Light)));
		let swapped = pair_list.fold(dark, |svg, (from, to)| svg.replace(from, to));
		assert_eq!(swapped, light);
	}

	#[test]
	fn animation_plays_once() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		assert!(!svg.contains("infinite"));
		assert!(!svg.contains("indefinite"));
		assert!(!svg.contains("repeatCount"));
		assert!(!svg.contains("repeatDur"));
		assert!(svg.contains("fill=\"freeze\""));
	}

	#[test]
	fn empty_timeline_renders_without_nan() {
		for variant in [Variant::Dark, Variant::Light] {
			let svg = render_languages(&Timeline::default(), variant, PACE);
			assert!(Document::parse(&svg).is_ok());
			assert!(svg.contains("<svg"));
			assert!(!svg.contains("NaN"));
			assert!(!svg.contains("inf"));
			assert!(!svg.contains("data-bar"));
		}
	}

	#[test]
	fn deterministic_output() {
		let first = render_languages(&fixture(), Variant::Dark, PACE);
		let second = render_languages(&fixture(), Variant::Dark, PACE);
		assert_eq!(first, second);
		assert!(first.is_ascii());
	}

	#[test]
	fn duration_grows_with_years_of_history() {
		let data = fixture();
		let after_lead = |pace: f64| {
			let clock = Race::new(&data, pace).clock;
			end_ms(&render_languages(&data, Variant::Dark, pace)) - clock.ms(clock.lead)
		};
		let normal = after_lead(PACE);
		let slow = after_lead(PACE * 2.0);
		let race = f64::from(race_ms(&data, PACE));
		assert!(race > 14_000.0, "{race}");
		assert!(
			(slow - normal - race).abs() < 300.0,
			"{normal} {slow} {race}"
		);
		let single = render_languages(&single("Rust"), Variant::Dark, PACE);
		assert!(single.contains("data-bar=\"Rust\""));
		assert!(end_ms(&single) <= 3_000.0, "{}", end_ms(&single));
	}

	#[test]
	fn inconsistent_timeline_renders_without_panic() {
		let languages_only = Timeline {
			frames: Vec::new(),
			..fixture()
		};
		let svg = render_languages(&languages_only, Variant::Dark, PACE);
		assert!(Document::parse(&svg).is_ok());
		assert!(!svg.contains("data-bar"));
		let mut stray = fixture();
		stray.frames[0].entries.push(Entry {
			language: 99,
			lines: 1,
			share_bp: 1,
			repos: 0,
		});
		let svg = render_languages(&stray, Variant::Dark, PACE);
		assert!(Document::parse(&svg).is_ok());
		assert_eq!(svg.matches("data-bar=").count(), 7);
	}

	#[test]
	fn candidate_never_shown_gets_no_column() {
		let mut usage = Usage::default();
		for (name, lines) in [("Rust", 100), ("C", 50), ("Go", 40), ("Nix", 30)] {
			spread(&mut usage, name, 0..1, lines);
		}
		spread(&mut usage, "C", 1..2, 150);
		spread(&mut usage, "Go", 2..3, 260);
		spread(&mut usage, "Nix", 3..4, 370);
		spread(&mut usage, "Nix", 4..30, 1);
		let data = timeline(&usage, 1, 520);
		assert!(
			data.languages
				.iter()
				.any(|summary| summary.language.name == "Go")
		);
		let clock = Race::new(&data, PACE).clock;
		let go = data
			.languages
			.iter()
			.position(|summary| summary.language.name == "Go")
			.unwrap();
		assert!(grid(&data, &clock)[go].iter().all(|cell| !cell.visible));
		let svg = render_languages(&data, Variant::Dark, PACE);
		assert!(!svg.contains("data-column=\"Go\""));
		assert!(!svg.contains("data-flight=\"Go\""));
		assert!(svg.contains("data-column=\"Nix\""));
	}

	#[test]
	fn language_without_lines_gets_no_column() {
		let mut data = single("Rust");
		data.languages.push(LanguageSummary {
			language: Language {
				name: "C",
				color: "#555555",
			},
			..data.languages[0].clone()
		});
		let svg = render_languages(&data, Variant::Dark, PACE);
		assert!(!svg.contains("data-column=\"C\""));
		assert!(svg.contains("data-flight=\"Rust\""));
		assert!(!svg.contains("data-flight=\"C\""));
	}

	#[test]
	fn names_are_xml_escaped() {
		let name = "A&<>\"'b";
		let svg = render_languages(&single(name), Variant::Light, PACE);
		assert!(svg.is_ascii());
		let document = Document::parse(&svg).unwrap();
		for key in ["data-bar", "data-column", "data-flight"] {
			assert_eq!(tagged(&document, key, name).attribute(key), Some(name));
		}
		let texts = text_list(column(&document, name));
		assert!(texts.iter().any(|text| text == name), "{texts:?}");
		let desc = document
			.root_element()
			.children()
			.find(|node| node.has_tag_name("desc"))
			.unwrap();
		assert!(desc.text().unwrap().contains(name));
	}
}
