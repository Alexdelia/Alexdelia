use super::clock::{Clock, race_ms};
use super::format::{
	animate, animate_transform, arrow, compact, escape, float, month, num, percent, trend_word,
};
use super::ranking::{Cell, cell_at, grid, settle};
use super::ticker::{Row, ticker};
use crate::icons::icon_path;
use crate::language::Language;
use crate::theme::{FONT_STACK, Palette, Variant, palette};
use crate::timeline::{LanguageSummary, Timeline};

const WIDTH: f64 = 880.0;
const HEIGHT: f64 = 432.0;
const GROUND: f64 = 300.0;
const CHART_HEIGHT: f64 = 196.0;
const MIN_BAR_HEIGHT: f64 = 2.0;
const GUTTER: f64 = 60.0;
const RIGHT_MARGIN: f64 = 8.0;
const USABLE_WIDTH: f64 = WIDTH - GUTTER - RIGHT_MARGIN;
const MAX_SLOT: f64 = 64.0;
const BAR_RATIO: f64 = 0.72;
const GHOST_OPACITY: &str = ".22";
const TOP_RADIUS: f64 = 2.0;
const LABEL_PAD: f64 = 4.0;
const HEADER_GAP: f64 = 4.0;
const HEADER_LIST: [&str; 5] = ["share", "lines", "repos", "since", "past year"];
const GLYPH_EM: f64 = 0.62;
const NAME_FONT_PX: f64 = 10.0;

const BELT_LEFT: f64 = -40.0;
const BELT_RIGHT: f64 = 862.0;
const BELT_TOP: f64 = 16.0;
const BELT_THICKNESS: f64 = 14.0;
const BELT_AXLE: f64 = BELT_TOP + BELT_THICKNESS / 2.0;
const CLEAT_OFFSET: f64 = 1.5;
const ROLLER_RADIUS: f64 = 4.5;
const DRUM_RADIUS: f64 = 5.5;
const ROLLER_FIRST: f64 = 24.0;
const ROLLER_GAP: f64 = 56.0;
const HANGER_LIST: [f64; 4] = [96.0, 330.0, 564.0, 798.0];
const BELT_SPEED: f64 = 0.8;

const TOKEN_RADIUS: f64 = 10.0;
const RIDE_Y: f64 = BELT_TOP + BELT_THICKNESS + CLEAT_OFFSET + TOKEN_RADIUS;
const SPAWN_X: f64 = -TOKEN_RADIUS - 4.0;
const SINK_DEPTH: f64 = 2.0;
const TOKEN_GAP: f64 = 4.0;
const TOKEN_SPACING_MS: f64 = (TOKEN_RADIUS + TOKEN_RADIUS + TOKEN_GAP) / BELT_SPEED;
const DARK_INK_THRESHOLD: u32 = 150_000;

const BUILD_MS: f64 = 600.0;
const FALL_MS: f64 = 450.0;
const FADE_MS: f64 = 250.0;
const GROW_MS: f64 = 450.0;
const SINK_MS: f64 = 120.0;
const DROP_BUDGET: u64 = 48;

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

#[must_use]
pub fn render_languages(timeline: &Timeline, variant: Variant, seconds_per_year: f64) -> String {
	let colors = palette(variant);
	let race = Race::new(timeline, seconds_per_year);
	[
		header(timeline, &race, colors),
		defs(&race, colors),
		belt(&race.clock, colors),
		ground(colors),
		headers(&race),
		drop_list(&race)
			.iter()
			.map(logo)
			.collect::<Vec<_>>()
			.concat(),
		race.lanes
			.iter()
			.map(|lane| column(lane, &race))
			.collect::<Vec<_>>()
			.concat(),
		date_ticker(timeline, &race.clock),
		"</svg>".to_owned(),
	]
	.concat()
}

fn name_width(name: &str) -> f64 {
	float(name.chars().count()) * GLYPH_EM * NAME_FONT_PX
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
	height: Vec<f64>,
	shown: Vec<bool>,
	appear_tick: usize,
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
}

struct Race<'a> {
	clock: Clock,
	layout: Layout,
	top_lines: u64,
	lanes: Vec<Lane<'a>>,
}

impl<'a> Race<'a> {
	fn new(timeline: &'a Timeline, seconds_per_year: f64) -> Self {
		let languages: &[LanguageSummary] = if timeline.frames.is_empty() {
			&[]
		} else {
			&timeline.languages
		};
		let clock = Clock::new(timeline.frames.len(), race_ms(timeline, seconds_per_year));
		let layout = Layout::new(languages.len().min(timeline.top_n));
		let grid = grid(timeline, &clock);
		let top_lines = grid
			.iter()
			.filter_map(|row| row.last())
			.map(|cell| cell.lines)
			.max()
			.unwrap_or(0);
		let lanes = languages
			.iter()
			.zip(grid)
			.enumerate()
			.filter(|(_, (_, cells))| cells.iter().any(|cell| cell.visible))
			.map(|(index, (summary, cells))| {
				lane(index, summary, cells, &clock, &layout, top_lines)
			})
			.collect();
		Self {
			clock,
			layout,
			top_lines,
			lanes,
		}
	}
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
	top_lines: u64,
) -> Lane<'a> {
	let first_visible = cells.iter().position(|cell| cell.visible).unwrap_or(0);
	let target_list: Vec<f64> = (0..clock.ticks())
		.map(|tick| {
			let frame = clock.frame_of(tick).max(first_visible);
			layout.center(cell_at(&cells, frame).slot)
		})
		.collect();
	let height = (0..clock.ticks())
		.map(|tick| {
			let lines = if clock.is_lead(tick) {
				0
			} else {
				cell_at(&cells, clock.frame_of(tick)).lines
			};
			bar_height(lines, top_lines)
		})
		.collect();
	let shown = (0..clock.ticks())
		.map(|tick| cell_at(&cells, clock.frame_of(tick)).visible)
		.collect();
	let appear_tick = if first_visible == 0 {
		0
	} else {
		clock.lead + first_visible
	};
	Lane {
		index,
		summary,
		x: settle(&target_list, f64::from(clock.tick_ms)),
		height,
		shown,
		appear_tick,
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
		"Share of lines added per language{period}, racing week by week, fed by a conveyor belt. Final: {}.",
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
			"<clipPath id=\"cb\"><rect width=\"{w}\" height=\"{belt_h}\">{grow}</rect></clipPath>",
			"{share}{lines}{repos}",
			"<clipPath id=\"cd\"><rect x=\"{date_x}\" y=\"{date_y}\" width=\"140\" height=\"18\"/></clipPath>",
			"{tokens}",
			"</defs>",
		),
		above_ground = clip("k", half, 0.0, GROUND),
		ground = colors.ground,
		w = WIDTH,
		belt_h = num(BELT_TOP + BELT_THICKNESS + 4.0, 1),
		grow = animate("width", &format!("0;{}", num(WIDTH, 0)), BUILD_MS),
		share = clip("cs", half, SHARE_ROW - 12.0, 15.0),
		lines = clip("cl", half, LINES_ROW - 10.5, 13.5),
		repos = clip("cr", half, REPOS_ROW - 10.5, 13.5),
		date_x = num(-140.0, 1),
		date_y = num(DATE_ROW - 14.0, 1),
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

fn belt(clock: &Clock, colors: Palette) -> String {
	let duration = clock.duration();
	let travel = BELT_SPEED * duration;
	let roller_list: Vec<String> =
		std::iter::successors(Some(ROLLER_FIRST), |x| Some(x + ROLLER_GAP))
			.take_while(|x| *x < BELT_RIGHT - BELT_THICKNESS)
			.map(|x| roller(x, ROLLER_RADIUS, travel, duration))
			.collect();
	let hangers: Vec<String> = HANGER_LIST
		.iter()
		.map(|x| format!("M{} 0V{}", num(*x, 1), num(BELT_TOP, 1)))
		.collect();
	let cleat = |y: f64, shift: f64| {
		format!(
			"<path d=\"M{left} {y}H{right}\" stroke=\"{accent}\" stroke-width=\"3\" stroke-dasharray=\"2 10\">{motion}</path>",
			left = num(BELT_LEFT, 1),
			y = num(y, 1),
			right = num(BELT_RIGHT, 1),
			accent = colors.belt_accent,
			motion = animate(
				"stroke-dashoffset",
				&format!("0;{}", num(shift, 0)),
				duration
			),
		)
	};
	format!(
		concat!(
			"<g clip-path=\"url(#cb)\" fill=\"none\" stroke=\"{belt}\">",
			"<path d=\"{hangers}\" stroke-width=\"1.5\"/>",
			"<path d=\"M{left} {top}H{right}a{r} {r} 0 0 1 0 {thick}H{left}\" fill=\"{belt}\" fill-opacity=\".2\" stroke-width=\"2\"/>",
			"{upper}{lower}{rollers}{drum}",
			"</g>",
		),
		belt = colors.belt,
		hangers = hangers.concat(),
		left = num(BELT_LEFT, 1),
		top = num(BELT_TOP, 1),
		right = num(BELT_RIGHT, 1),
		r = num(BELT_THICKNESS / 2.0, 1),
		thick = num(BELT_THICKNESS, 1),
		upper = cleat(BELT_TOP - CLEAT_OFFSET, travel),
		lower = cleat(BELT_TOP + BELT_THICKNESS + CLEAT_OFFSET, -travel),
		rollers = roller_list.concat(),
		drum = roller(BELT_RIGHT, DRUM_RADIUS, travel, duration),
	)
}

fn roller(x: f64, radius: f64, travel: f64, duration: f64) -> String {
	let turn = (-travel / radius).to_degrees();
	format!(
		"<g transform=\"translate({} {})\"><g>{}<circle r=\"{r}\"/><path d=\"M-{r} 0H{r}M0-{r}V{r}\"/></g></g>",
		num(x, 1),
		num(BELT_AXLE, 1),
		animate_transform("rotate", &format!("0;{}", num(turn, 0)), duration),
		r = num(radius, 1),
	)
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
			BUILD_MS
		),
		hatch = num(GROUND + 1.0, 1),
		grow = animate("width", &format!("0;{}", num(WIDTH, 0)), BUILD_MS),
	)
}

fn column(lane: &Lane, race: &Race) -> String {
	let clock = &race.clock;
	let duration = clock.duration();
	let width = race.layout.bar_width();
	let ghost_height = bar_height(lane.last().lines, race.top_lines) + TOP_RADIUS;
	let x_list: Vec<String> = lane.x.iter().map(|x| num(*x, 1)).collect();
	let height_list: Vec<String> = lane
		.height
		.iter()
		.map(|height| num(height + TOP_RADIUS, 1))
		.collect();
	let appear_ms = clock.ms(lane.appear_tick);
	let name = escape(lane.name());
	let trend = lane.summary.trend;
	let bar_shape = format!(
		"x=\"{}\" width=\"{}\" rx=\"{}\" fill=\"{}\" transform=\"matrix(1 0 0 -1 0 {})\"",
		num(-width / 2.0, 1),
		num(width, 1),
		num(TOP_RADIUS, 1),
		lane.summary.language.color,
		num(GROUND + TOP_RADIUS, 1),
	);
	format!(
		concat!(
			"<g data-column=\"{name}\" transform=\"translate({x})\"{hidden}>{slide}{fade}",
			"<g clip-path=\"url(#k)\">",
			"<rect data-ghost=\"{name}\" {shape} height=\"{ghost_height}\" opacity=\"{opacity}\">{ghost_growth}</rect>",
			"<rect data-bar=\"{name}\" {shape} height=\"{height}\">{grow}</rect>",
			"</g>",
			"<use href=\"#i{index}\" y=\"{token_y}\"/>",
			"{name_label}{legend}{since}{trend}",
			"</g>",
		),
		name = name,
		x = num(lane.x.last().copied().unwrap_or(0.0), 1),
		hidden = if lane.last().visible {
			""
		} else {
			" opacity=\"0\""
		},
		slide = animate_transform("translate", &x_list.join(";"), duration),
		fade = fade(&lane.shown, clock),
		shape = bar_shape,
		ghost_height = num(ghost_height, 1),
		opacity = GHOST_OPACITY,
		ghost_growth = grow_in(&num(ghost_height, 1), appear_ms),
		height = height_list.last().cloned().unwrap_or_default(),
		grow = animate("height", &height_list.join(";"), duration),
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

fn fade(shown: &[bool], clock: &Clock) -> String {
	let duration = clock.duration();
	let opacity = |visible: bool| if visible { "1" } else { "0" };
	let mut point_list: Vec<(f64, bool)> = shown
		.first()
		.map(|first| (0.0, *first))
		.into_iter()
		.collect();
	for (tick, pair) in (1..).zip(shown.windows(2)) {
		if pair[0] == pair[1] {
			continue;
		}
		let start = clock.ms(tick);
		let fading = point_list.len() > 1 && point_list.last().is_some_and(|end| end.0 > start);
		if fading {
			point_list.truncate(point_list.len() - 2);
		} else {
			point_list.push((start, pair[0]));
			point_list.push(((start + FADE_MS).min(duration), pair[1]));
		}
	}
	if point_list.len() < 2 {
		return String::new();
	}
	let key_list: Vec<String> = point_list
		.iter()
		.map(|(ms, _)| num(ms / duration, 4))
		.chain(["1".to_owned()])
		.collect();
	let value_list: Vec<&str> = point_list
		.iter()
		.map(|(_, visible)| opacity(*visible))
		.chain(point_list.last().map(|(_, visible)| opacity(*visible)))
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

fn grow_in(full_height: &str, appear_ms: f64) -> String {
	if appear_ms <= 0.0 {
		return String::new();
	}
	format!(
		"<animate attributeName=\"height\" values=\"0;0;{full_height}\" keyTimes=\"0;{};1\" dur=\"{}ms\" fill=\"freeze\"/>",
		num(appear_ms / (appear_ms + GROW_MS), 4),
		num(appear_ms + GROW_MS, 0),
	)
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

struct Drop<'a> {
	lane: &'a Lane<'a>,
	tick: usize,
	begin: f64,
}

fn drop_list<'a>(race: &'a Race) -> Vec<Drop<'a>> {
	let total: u64 = race.lanes.iter().map(|lane| lane.last().lines).sum();
	let mut list: Vec<Drop> = race
		.lanes
		.iter()
		.flat_map(|lane| {
			growth_frames(lane, total)
				.into_iter()
				.filter(|frame| cell_at(&lane.cells, *frame).visible)
				.map(|frame| race.clock.lead + frame)
				.map(move |tick| Drop {
					lane,
					tick,
					begin: race.clock.ms(tick) - trip_ms(lane.x[tick]),
				})
		})
		.collect();
	list.sort_by(|left, right| left.begin.total_cmp(&right.begin));
	let mut free = 0.0;
	for drop in &mut list {
		drop.begin = drop.begin.max(free);
		free = drop.begin + TOKEN_SPACING_MS;
	}
	list
}

fn growth_frames(lane: &Lane, total: u64) -> Vec<usize> {
	let lines = lane.last().lines;
	let quota = ((DROP_BUDGET * lines + total / 2) / total.max(1)).max(u64::from(lines > 0));
	let mut frame_list: Vec<usize> = (0..quota)
		.map(|step| {
			let threshold = ((2 * step + 1) * lines).div_ceil(2 * quota);
			lane.cells.partition_point(|cell| cell.lines < threshold)
		})
		.collect();
	frame_list.dedup();
	frame_list
}

fn ride_ms(x: f64) -> f64 {
	(x - SPAWN_X) / BELT_SPEED
}

fn trip_ms(x: f64) -> f64 {
	ride_ms(x) + FALL_MS
}

fn logo(drop: &Drop) -> String {
	let lane = drop.lane;
	let x = lane.x[drop.tick];
	let landing_y = GROUND - lane.height[drop.tick] + SINK_DEPTH;
	let duration = trip_ms(x);
	let begin = drop.begin;
	let timing = format!(
		"begin=\"{}ms\" dur=\"{}ms\"",
		num(begin, 0),
		num(duration, 0)
	);
	format!(
		concat!(
			"<use href=\"#i{index}\" data-drop=\"{name}\" opacity=\"0\">",
			"<animate attributeName=\"opacity\" values=\"1;1;0\" keyTimes=\"0;{fade};1\" {timing} fill=\"freeze\"/>",
			"<animateTransform attributeName=\"transform\" type=\"translate\" calcMode=\"spline\" ",
			"keyTimes=\"0;{ride};1\" keySplines=\"0 0 1 1;.55 0 1 1\" ",
			"values=\"{spawn} {ride_y};{x} {ride_y};{x} {land}\" {timing}/>",
			"</use>",
		),
		index = lane.index,
		name = escape(lane.name()),
		fade = num((duration - SINK_MS) / duration, 4),
		ride = num(ride_ms(x) / duration, 4),
		spawn = num(SPAWN_X, 1),
		ride_y = num(RIDE_Y, 1),
		x = num(x, 1),
		land = num(landing_y, 1),
		timing = timing,
	)
}

fn date_ticker(timeline: &Timeline, clock: &Clock) -> String {
	let state_list = clock
		.state_ticks()
		.into_iter()
		.map(|tick| {
			timeline
				.frames
				.get(clock.frame_of(tick))
				.map_or_else(String::new, |frame| month(frame.week))
		})
		.collect();
	let row = Row {
		clip: "cd",
		class: "d",
		y: DATE_ROW,
		step: DATE_STEP,
	};
	format!(
		"<g class=\"d\" transform=\"translate({} 0)\">{}</g>",
		num(DATE_X, 1),
		ticker(state_list, row, clock.duration())
	)
}

#[cfg(test)]
mod tests {
	use super::super::ranking::grid;
	use super::*;
	use crate::cache::Usage;
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

	fn clock_of(data: &Timeline) -> Clock {
		Clock::new(data.frames.len(), race_ms(data, PACE))
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

	fn ghost_rect<'a>(document: &'a Document, name: &str) -> Node<'a, 'a> {
		tagged(document, "data-ghost", name)
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
			}],
			top_n: 2,
		}
	}

	fn end_ms(svg: &str) -> f64 {
		let document = Document::parse(svg).unwrap();
		let ms = |node: Node, name| {
			node.attribute(name).map_or(0.0, |value: &str| {
				value.trim_end_matches("ms").parse().unwrap()
			})
		};
		document
			.descendants()
			.map(|node| ms(node, "begin") + ms(node, "dur"))
			.fold(0.0, f64::max)
	}

	fn color_list(palette: Palette) -> [&'static str; 5] {
		[
			palette.text,
			palette.muted,
			palette.ground,
			palette.belt,
			palette.belt_accent,
		]
	}

	fn drop_landing_list(document: &Document, name: &str) -> Vec<f64> {
		document
			.descendants()
			.filter(|node| node.attribute("data-drop") == Some(name))
			.map(|drop| {
				let motion = drop.first_element_child().unwrap();
				ms(motion, "begin") + ms(motion, "dur")
			})
			.collect()
	}

	fn drop_landing_ms_list(document: &Document, name: &str) -> Vec<u32> {
		let whole_ms = |node: Node, key: &str| -> u32 {
			node.attribute(key)
				.unwrap()
				.trim_end_matches("ms")
				.parse()
				.unwrap()
		};
		document
			.descendants()
			.filter(|node| node.attribute("data-drop") == Some(name))
			.map(|drop| {
				let motion = drop.first_element_child().unwrap();
				whole_ms(motion, "begin") + whole_ms(motion, "dur")
			})
			.collect()
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
	fn has_ghost_bar_per_language_at_final_lines() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let mut ghost_list: Vec<(f64, &str, f64)> = FINAL_ORDER
			.iter()
			.map(|name| {
				let rect = ghost_rect(&document, name);
				let height = number(rect, "height") - TOP_RADIUS;
				(column_x(&document, name), *name, height)
			})
			.collect();
		ghost_list.sort_by(|left, right| left.0.total_cmp(&right.0));
		let order: Vec<&str> = ghost_list.iter().map(|ghost| ghost.1).collect();
		assert_eq!(order, FINAL_ORDER);
		let ratio = ghost_list[0].2 / FINAL_LINES[0].sqrt();
		for (ghost, lines) in ghost_list.iter().zip(FINAL_LINES) {
			let relative = (ghost.2 / lines.sqrt() - ratio).abs() / ratio;
			assert!(relative < 5e-3, "{ghost:?}");
		}
		assert!(shows_from_start(ghost_rect(&document, "Rust")));
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
			for rect in [
				tagged(&document, "data-bar", name),
				ghost_rect(&document, name),
			] {
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
		}
		let rust = height_list(&document, "Rust");
		assert!((rust[0] - TOP_RADIUS).abs() < 0.01, "{}", rust[0]);
	}

	#[test]
	fn ghost_labels_visible_from_first_frame() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in ["Rust", "C"] {
			let column = column(&document, name);
			let name_text = column
				.descendants()
				.find(|node| node.has_tag_name("text") && node.text() == Some(name))
				.unwrap();
			assert!(shows_from_start(name_text), "{name}");
			let token = column
				.children()
				.find(|node| node.has_tag_name("use"))
				.unwrap();
			assert!(shows_from_start(token), "{name}");
		}
	}

	#[test]
	fn column_enters_when_first_within_top_n() {
		let data = fixture();
		let clock = clock_of(&data);
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
			assert!((rise - clock.ms(entry_tick)).abs() < 1.0, "{name} {rise}");
			let x_list = translate_list(column);
			assert!(
				x_list[..entry_tick]
					.iter()
					.all(|x| (*x - x_list[entry_tick]).abs() < 0.01),
				"{name}"
			);
			let ghost_growth = ghost_rect(&document, name)
				.children()
				.find(|child| child.attribute("attributeName") == Some("height"))
				.unwrap();
			assert!(
				ghost_growth
					.attribute("values")
					.unwrap()
					.starts_with("0;0;")
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
		let clock = clock_of(&data);
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
		let cells = &grid(&data, &clock)[c_index];
		let landing_list = drop_landing_ms_list(&document, "C");
		assert!(landing_list.len() >= 2, "{landing_list:?}");
		for landing in landing_list {
			let tick = usize::try_from((landing + clock.tick_ms / 2) / clock.tick_ms).unwrap();
			let frame = clock.frame_of(tick);
			assert!(cells[frame].visible, "{landing} {frame}");
		}
	}

	#[test]
	fn first_drop_lands_after_entry() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		for name in FINAL_ORDER.iter().chain(&HIDDEN_AT_END) {
			let entry = first_rise_ms(column(&document, name)).unwrap_or(0.0);
			for landing in drop_landing_list(&document, name) {
				assert!(landing + 1.0 >= entry, "{name} {landing} {entry}");
			}
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
			let member_list = [
				ghost_rect(&document, name),
				tagged(&document, "data-bar", name),
				name_text,
			];
			for member in member_list {
				let mover = member.ancestors().find(|node| has_slide(*node)).unwrap();
				assert_eq!(mover, column, "{name}");
			}
		}
		assert_eq!(
			document
				.descendants()
				.filter(|node| has_slide(*node))
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
			let ghost = number(ghost_rect(&document, name), "height");
			assert!((heights.last().unwrap() - ghost).abs() < 0.1, "{name}");
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

	#[test]
	fn uses_exact_language_colors() {
		let data = fixture();
		let svg = render_languages(&data, Variant::Light, PACE);
		let document = Document::parse(&svg).unwrap();
		for summary in &data.languages {
			let name = summary.language.name;
			let color = Some(summary.language.color);
			assert_eq!(tagged(&document, "data-bar", name).attribute("fill"), color);
			assert_eq!(ghost_rect(&document, name).attribute("fill"), color);
		}
		assert!(svg.contains("#3572A5"));
	}

	#[test]
	fn logos_drop_in_proportion_to_growth() {
		let svg = render_languages(&fixture(), Variant::Dark, PACE);
		let document = Document::parse(&svg).unwrap();
		let count_list: Vec<usize> = FINAL_ORDER
			.iter()
			.map(|name| drop_landing_list(&document, name).len())
			.collect();
		assert!(
			count_list.windows(2).all(|pair| pair[0] >= pair[1]),
			"{count_list:?}"
		);
		assert!(count_list.iter().all(|count| *count >= 1));
		assert!(count_list[0] > count_list[2] * 2);
		for node in document.descendants() {
			if let Some(begin) = node.attribute("begin") {
				let ms: f64 = begin.trim_end_matches("ms").parse().unwrap();
				assert!(ms >= 0.0, "{begin}");
			}
		}
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

	#[test]
	fn size_under_1mb_at_520_weeks_16_languages() {
		let data = large();
		assert!(data.languages.len() >= 16);
		assert_eq!(data.frames.len(), 520);
		for variant in [Variant::Dark, Variant::Light] {
			let svg = render_languages(&data, variant, PACE);
			assert!(svg.len() < 1_000_000, "{}", svg.len());
			assert!(Document::parse(&svg).is_ok());
		}
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
		let normal = end_ms(&render_languages(&data, Variant::Dark, PACE));
		let slow = end_ms(&render_languages(&data, Variant::Dark, PACE * 2.0));
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
		let clock = clock_of(&data);
		let go = data
			.languages
			.iter()
			.position(|summary| summary.language.name == "Go")
			.unwrap();
		assert!(grid(&data, &clock)[go].iter().all(|cell| !cell.visible));
		let svg = render_languages(&data, Variant::Dark, PACE);
		assert!(!svg.contains("data-column=\"Go\""));
		assert!(!svg.contains("data-drop=\"Go\""));
		assert!(svg.contains("data-column=\"Nix\""));
	}

	#[test]
	fn fade_reversal_within_a_fade_collapses() {
		let clock = Clock::new(10, 900);
		assert_eq!(clock.tick_ms, 100);
		let blip = [true, true, true, false, false, true, true, true];
		assert_eq!(fade(&blip, &clock), "");
		let gap = [true, true, true, false, false, false, true, true];
		let animation = fade(&gap, &clock);
		assert!(animation.contains("values=\"1;1;0;0;1;1\""), "{animation}");
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
		assert!(svg.contains("data-drop=\"Rust\""));
		assert!(!svg.contains("data-drop=\"C\""));
	}

	#[test]
	fn names_are_xml_escaped() {
		let name = "A&<>\"'b";
		let svg = render_languages(&single(name), Variant::Light, PACE);
		assert!(svg.is_ascii());
		let document = Document::parse(&svg).unwrap();
		for key in ["data-bar", "data-ghost", "data-column", "data-drop"] {
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
