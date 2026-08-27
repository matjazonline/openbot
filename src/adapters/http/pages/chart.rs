//! Small time-series charts, drawn as SVG on the server.
//!
//! Every chart here is emitted as markup by the same render that emits the rest of the panel, which
//! is the whole reason there is no charting library behind it. `/ui/dashboard` re-renders its panels
//! server-side on a five-second tick and swaps them in wholesale, so a canvas library would be torn
//! down and re-initialised on every tick unless it were held out of the swap and fed from a second
//! endpoint — a parallel data path to keep in step with this one. SVG has no such state to lose: it
//! re-renders with everything around it, and costs nothing before first paint.
//!
//! Colours arrive as CSS variables (`var(--color-success)`) rather than as Tailwind classes, so a
//! chart follows the theme controller in the top bar without a second palette for dark mode.
//!
//! # What the caller supplies
//!
//! A [`TimeChart`] takes *dense* buckets: one entry per slice of the window, gap-filled by the
//! query, oldest first. The x-axis is positional — bucket `n` sits at slot `n` — so a sparse series
//! would draw a quiet hour as if it had never happened.

use chrono::{DateTime, Utc};

use super::escape_html_text;

/// The chart's own coordinate space. Rendered `w-full`, so these are proportions as much as pixels.
const VIEW_WIDTH: f64 = 760.0;
const VIEW_HEIGHT: f64 = 210.0;

/// Room for the y tick labels on the left, the last x label on the right, and the x labels below.
const PAD_LEFT: f64 = 48.0;
const PAD_RIGHT: f64 = 32.0;
const PAD_TOP: f64 = 14.0;
const PAD_BOTTOM: f64 = 28.0;

const PLOT_WIDTH: f64 = VIEW_WIDTH - PAD_LEFT - PAD_RIGHT;
const PLOT_HEIGHT: f64 = VIEW_HEIGHT - PAD_TOP - PAD_BOTTOM;

/// How many intervals the y-axis is cut into. Four gaps, five labels including the baseline.
const TICKS: f64 = 4.0;

/// The gap that separates two touching marks, in the surface colour.
///
/// Stacked segments are separated by *negative space*, never by a stroke around each one: a border
/// is ink that is not data, and at this size it thickens every segment into a smudge.
const GAP: f64 = 2.0;

/// Columns are capped rather than filling their slot, so the leftover band reads as air.
const MAX_BAR_WIDTH: f64 = 24.0;

/// The rounded data-end. Square at the baseline, rounded where the value stops.
const CAP_RADIUS: f64 = 4.0;

/// A value so small it would otherwise render as nothing. One task is not no tasks.
const MIN_BAR_HEIGHT: f64 = 2.0;

/// At most this many x labels, whatever the bucket count, so they never overlap.
const MAX_X_LABELS: usize = 8;

/// The hover affordance, emitted once per page rather than once per chart.
///
/// A `:hover` rule rather than a JS listener: the panels are replaced wholesale every few seconds,
/// and any handler bound to a mark inside them would be thrown away with it. CSS survives the swap
/// because it was never attached to the swapped nodes in the first place.
pub(crate) const STYLE: &str = r##"<style>
    .chart-band { fill: var(--color-base-content); opacity: 0; transition: opacity .12s; }
    .chart-band:hover { opacity: .07; }
</style>"##;

/// How a series is drawn. One measure, one shape — never two shapes sharing an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChartKind {
    /// Part-to-whole over time: the segments of one column sum to that bucket's total.
    StackedColumn,
    /// Trend over time, where the series are separate measures rather than parts of a whole.
    Line,
    /// A single series whose level is the point. Filled, because there is nothing to hide behind it.
    Area,
}

/// What the y-axis counts, which is the only thing that decides how a tick reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YUnit {
    Count,
    Millis,
    Percent,
}

/// One line, band, or stack segment.
///
/// `values` is positional against [`TimeChart::buckets`] and must be the same length. A `None` is
/// *not measured* and is drawn as a break in the line — distinct from `Some(0.0)`, which is a real
/// zero. Stacks and areas have no way to draw a break, so they read `None` as zero; only [`Line`]
/// preserves the distinction, which is why latency uses it.
///
/// [`Line`]: ChartKind::Line
pub(crate) struct Series<'a> {
    pub label: &'a str,
    /// A CSS colour, normally a daisyUI variable so the theme carries it.
    pub color: &'a str,
    pub values: Vec<Option<f64>>,
}

pub(crate) struct TimeChart<'a> {
    /// Dense and ascending: one entry per slice of the window, gap-filled by the query.
    pub buckets: &'a [DateTime<Utc>],
    pub series: &'a [Series<'a>],
    pub kind: ChartKind,
    pub unit: YUnit,
    /// A `chrono` format string for the x labels — a day-long window needs the date, an hour does
    /// not. Comes from `DashboardWindow::tick_format`.
    pub tick_format: &'a str,
}

impl TimeChart<'_> {
    /// What one bucket's marks add up to, which is what the y-axis has to reach.
    ///
    /// Stacked columns sum their series, because the segments share a column. Lines and areas take
    /// the largest single value, because they share only the axis.
    fn peak(&self) -> f64 {
        (0..self.buckets.len())
            .map(|index| match self.kind {
                ChartKind::StackedColumn => self.stack_total(index),
                ChartKind::Line | ChartKind::Area => self
                    .series
                    .iter()
                    .filter_map(|series| series.at(index))
                    .fold(0.0, f64::max),
            })
            .fold(0.0, f64::max)
    }

    fn stack_total(&self, index: usize) -> f64 {
        self.series
            .iter()
            .filter_map(|series| series.at(index))
            .sum()
    }
}

impl Series<'_> {
    fn at(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied().flatten()
    }

    /// The last bucket this series actually measured — where a line ends and its label goes.
    fn last_measured(&self) -> Option<(usize, f64)> {
        self.values
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, value)| value.map(|value| (index, value)))
    }
}

/// The plot's value-to-pixel mapping, resolved once so every phase draws against the same one.
struct Frame {
    /// The top of the axis, always a clean multiple of `step`.
    max: f64,
    step: f64,
    buckets: usize,
}

/// A point in the SVG's coordinate space.
///
/// A struct rather than two `f64`s because that is the pair the compiler cannot otherwise keep
/// apart: every path segment here takes an x and a y of the same type, and a swapped pair produces
/// a chart that renders perfectly and is wrong.
#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

impl Frame {
    fn new(chart: &TimeChart<'_>) -> Self {
        let step = nice_step(chart.peak());

        Self {
            max: step * TICKS,
            step,
            buckets: chart.buckets.len().max(1),
        }
    }

    fn y(&self, value: f64) -> f64 {
        PAD_TOP + PLOT_HEIGHT * (1.0 - value / self.max)
    }

    fn baseline(&self) -> f64 {
        PAD_TOP + PLOT_HEIGHT
    }

    /// The full slot one bucket owns, marks and surrounding air together.
    fn band(&self) -> f64 {
        PLOT_WIDTH / self.buckets as f64
    }

    fn center(&self, index: usize) -> f64 {
        PAD_LEFT + self.band() * (index as f64 + 0.5)
    }

    fn point(&self, index: usize, value: f64) -> Point {
        Point {
            x: self.center(index),
            y: self.y(value),
        }
    }
}

/// Round a peak up to a tick step a reader can do arithmetic with: 1, 2, or 5 times a power of ten.
///
/// The step is chosen rather than the ceiling, so `max` comes out as `step * TICKS` and every
/// gridline lands on a whole number. Picking a nice *ceiling* instead gives clean ends and quarters
/// like 2.5 in between, which is the wrong half of the problem to solve.
fn nice_step(peak: f64) -> f64 {
    if peak <= 0.0 {
        // An idle window still gets a real axis. A chart whose y-axis is 0-to-0 has no scale at all,
        // and every mark in it would sit on the baseline claiming to be full height.
        return 1.0;
    }

    let rough = peak / TICKS;
    let magnitude = 10f64.powf(rough.log10().floor());

    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * magnitude)
        // Counts have no meaningful fraction, so never offer a step below one whole unit.
        .map(|step| step.max(1.0))
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * magnitude)
}

/// Label every nth bucket, counting back from the newest.
///
/// Anchored at the right rather than the left because the newest bucket is the one a reader looks
/// for first, and an anchor on the left leaves it unlabelled whenever the stride does not divide
/// the bucket count.
fn labels_bucket(index: usize, count: usize) -> bool {
    let stride = count.div_ceil(MAX_X_LABELS).max(1);

    (count - 1 - index).is_multiple_of(stride)
}

fn format_value(value: f64, unit: YUnit) -> String {
    match unit {
        YUnit::Count => thousands(value.round() as i64),
        YUnit::Millis if value >= 1000.0 => format!("{:.1}s", value / 1000.0),
        YUnit::Millis => format!("{}ms", value.round() as i64),
        YUnit::Percent => format!("{}%", value.round() as i64),
    }
}

/// Group digits so a six-figure token count can be read at a glance.
///
/// Shared with the stat rows on purpose: an axis tick and the figure above it are the same number,
/// and two formatters is two ways for them to disagree.
pub(crate) fn thousands(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }

    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// The whole chart: the frame, the marks, and the legend that names them.
pub(crate) fn time_chart(chart: &TimeChart<'_>) -> String {
    let frame = Frame::new(chart);

    format!(
        r##"<figure class="px-4 pb-2 pt-3">
            <svg viewBox="0 0 {VIEW_WIDTH} {VIEW_HEIGHT}" class="h-52 w-full" role="img"
                preserveAspectRatio="xMidYMid meet">
                {grid}
                {marks}
                {bands}
            </svg>
            {legend}
        </figure>"##,
        grid = grid(chart, &frame),
        marks = marks(chart, &frame),
        bands = hover_bands(chart, &frame),
        legend = legend(chart),
    )
}

/// Gridlines and both sets of tick labels — everything that is not data.
///
/// Solid hairlines one step off the surface. Dashes draw the eye to the grid, which is the one thing
/// on a chart that should never be looked at directly.
fn grid(chart: &TimeChart<'_>, frame: &Frame) -> String {
    let mut svg = String::new();

    let mut tick = 0.0;
    while tick <= frame.max + f64::EPSILON {
        let y = frame.y(tick);

        svg.push_str(&format!(
            r##"<line x1="{PAD_LEFT}" y1="{y:.1}" x2="{right:.1}" y2="{y:.1}"
                stroke="var(--color-base-300)" stroke-width="1" />
            <text x="{label_x:.1}" y="{text_y:.1}" text-anchor="end" font-size="10"
                fill="var(--color-base-content)" opacity="0.6">{label}</text>"##,
            right = PAD_LEFT + PLOT_WIDTH,
            label_x = PAD_LEFT - 8.0,
            text_y = y + 3.0,
            label = escape_html_text(&format_value(tick, chart.unit)),
        ));

        tick += frame.step;
    }

    for (index, bucket) in chart.buckets.iter().enumerate() {
        if !labels_bucket(index, chart.buckets.len()) {
            continue;
        }

        svg.push_str(&format!(
            r##"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-size="10"
                fill="var(--color-base-content)" opacity="0.6">{label}</text>"##,
            x = frame.center(index),
            y = frame.baseline() + 16.0,
            label = escape_html_text(&bucket.format(chart.tick_format).to_string()),
        ));
    }

    svg
}

fn marks(chart: &TimeChart<'_>, frame: &Frame) -> String {
    match chart.kind {
        ChartKind::StackedColumn => stacked_columns(chart, frame),
        ChartKind::Line => lines(chart, frame, false),
        ChartKind::Area => lines(chart, frame, true),
    }
}

/// One column per bucket, its segments stacked in the order the series were given.
fn stacked_columns(chart: &TimeChart<'_>, frame: &Frame) -> String {
    let width = frame.band().min(MAX_BAR_WIDTH);
    let mut svg = String::new();

    for index in 0..chart.buckets.len() {
        // Which segment carries the rounded cap: the last one in the stack that has any height.
        let topmost = chart
            .series
            .iter()
            .rposition(|series| series.at(index).unwrap_or(0.0) > 0.0);
        let mut base = 0.0;

        for (position, series) in chart.series.iter().enumerate() {
            let value = series.at(index).unwrap_or(0.0);
            if value <= 0.0 {
                continue;
            }

            let top = frame.y(base + value);
            let bottom = frame.y(base);
            base += value;

            // The gap belongs *between* segments, so it comes off the top of the lower one. The
            // topmost segment has nothing above it and keeps its full height.
            let is_top = topmost == Some(position);
            let inset = if is_top { 0.0 } else { GAP };
            let height = (bottom - top - inset).max(MIN_BAR_HEIGHT);
            let x = frame.center(index) - width / 2.0;

            svg.push_str(&segment(
                Point {
                    x,
                    y: bottom - height,
                },
                width,
                height,
                series.color,
                is_top,
            ));
        }
    }

    svg
}

/// One stack segment. Rounded where the data ends, square everywhere it continues.
fn segment(origin: Point, width: f64, height: f64, color: &str, capped: bool) -> String {
    if !capped {
        return format!(
            r##"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}"
                fill="{color}" />"##,
            x = origin.x,
            y = origin.y,
        );
    }

    let radius = CAP_RADIUS.min(width / 2.0).min(height);
    let (x, y) = (origin.x, origin.y);
    let right = x + width;
    let bottom = y + height;

    format!(
        r##"<path d="M {x:.1} {bottom:.1} L {x:.1} {corner:.1} Q {x:.1} {y:.1} {inner:.1} {y:.1}
            L {outer:.1} {y:.1} Q {right:.1} {y:.1} {right:.1} {corner:.1}
            L {right:.1} {bottom:.1} Z" fill="{color}" />"##,
        corner = y + radius,
        inner = x + radius,
        outer = right - radius,
    )
}

/// Lines, optionally filled down to the baseline, plus an end marker and one direct label each.
///
/// A `None` bucket ends the current run and the next measured value starts a new one, so an unmeasured
/// stretch is a visible break rather than a line drawn straight across it.
fn lines(chart: &TimeChart<'_>, frame: &Frame, filled: bool) -> String {
    let mut svg = String::new();

    for series in chart.series {
        let runs = measured_runs(chart, frame, series);

        for run in &runs {
            if filled {
                svg.push_str(&format!(
                    r##"<path d="{path} L {last:.1} {base:.1} L {first:.1} {base:.1} Z"
                        fill="{color}" fill-opacity="0.1" />"##,
                    path = polyline(run),
                    last = run[run.len() - 1].x,
                    first = run[0].x,
                    base = frame.baseline(),
                    color = series.color,
                ));
            }

            svg.push_str(&format!(
                r##"<path d="{path}" fill="none" stroke="{color}" stroke-width="2"
                    stroke-linecap="round" stroke-linejoin="round" />"##,
                path = polyline(run),
                color = series.color,
            ));
        }

        svg.push_str(&end_marker(chart, frame, series));
    }

    svg
}

/// The measured stretches of one series, each a run of consecutive buckets that has a value.
///
/// A lone measured bucket between two gaps becomes a one-point run, which `polyline` renders as a
/// dot rather than dropping it — an isolated reading is still a reading.
fn measured_runs(chart: &TimeChart<'_>, frame: &Frame, series: &Series<'_>) -> Vec<Vec<Point>> {
    let mut runs: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();

    for index in 0..chart.buckets.len() {
        match series.at(index) {
            Some(value) => current.push(frame.point(index, value)),
            None if !current.is_empty() => runs.push(std::mem::take(&mut current)),
            None => {}
        }
    }

    if !current.is_empty() {
        runs.push(current);
    }

    runs
}

fn polyline(points: &[Point]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let command = if index == 0 { 'M' } else { 'L' };
            format!("{command} {:.1} {:.1}", point.x, point.y)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A dot on the newest measured value, with its number beside it.
///
/// The one direct label each series gets. A number on every point is noise nobody reads, and the
/// axis plus the hover readout already carry the rest.
fn end_marker(chart: &TimeChart<'_>, frame: &Frame, series: &Series<'_>) -> String {
    let Some((index, value)) = series.last_measured() else {
        return String::new();
    };

    let point = frame.point(index, value);

    format!(
        r##"<circle cx="{x:.1}" cy="{y:.1}" r="4" fill="{color}"
            stroke="var(--color-base-200)" stroke-width="2" />
        <text x="{label_x:.1}" y="{label_y:.1}" text-anchor="end" font-size="10" font-weight="600"
            fill="var(--color-base-content)" opacity="0.75">{label}</text>"##,
        x = point.x,
        y = point.y,
        color = series.color,
        label_x = point.x - 8.0,
        // Above the dot unless that would leave the plot, in which case below it.
        label_y = if point.y < PAD_TOP + 14.0 {
            point.y + 14.0
        } else {
            point.y - 8.0
        },
        label = escape_html_text(&format_value(value, chart.unit)),
    )
}

/// A transparent strip per bucket carrying that bucket's readout.
///
/// The hit target is the whole slot rather than the mark, so a reader aims at a moment in time
/// instead of at a two-pixel column — and an empty bucket still answers when asked.
fn hover_bands(chart: &TimeChart<'_>, frame: &Frame) -> String {
    (0..chart.buckets.len())
        .map(|index| {
            format!(
                r##"<rect class="chart-band" x="{x:.1}" y="{PAD_TOP}" width="{width:.1}"
                    height="{PLOT_HEIGHT}"><title>{readout}</title></rect>"##,
                x = frame.center(index) - frame.band() / 2.0,
                width = frame.band(),
                readout = escape_html_text(&readout(chart, index)),
            )
        })
        .collect()
}

/// What one bucket says when you point at it: the time, then every series at that time.
fn readout(chart: &TimeChart<'_>, index: usize) -> String {
    let values = chart
        .series
        .iter()
        .map(|series| match series.at(index) {
            Some(value) => format!("{} {}", format_value(value, chart.unit), series.label),
            None => format!("no {}", series.label),
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{} — {values}",
        chart.buckets[index].format(chart.tick_format)
    )
}

/// Names the series, whenever there is more than one to tell apart.
///
/// A single-series chart gets none: there is only one colour on screen, and the panel's own heading
/// already says what it is. A legend box with one swatch in it restates the heading and costs a row.
fn legend(chart: &TimeChart<'_>) -> String {
    if chart.series.len() < 2 {
        return String::new();
    }

    let keys: String = chart
        .series
        .iter()
        .map(|series| {
            format!(
                r##"<span class="flex items-center gap-1.5">
                    <span class="inline-block h-2 w-2 rounded-sm" style="background:{color}"></span>
                    {label}
                </span>"##,
                color = series.color,
                label = escape_html_text(series.label),
            )
        })
        .collect();

    format!(
        r##"<figcaption class="flex items-center gap-4 pt-1 text-xs opacity-70">{keys}</figcaption>"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn buckets(count: usize) -> Vec<DateTime<Utc>> {
        (0..count)
            .map(|index| {
                Utc.timestamp_opt(1_700_000_000 + index as i64 * 300, 0)
                    .unwrap()
            })
            .collect()
    }

    fn chart<'a>(
        marks: &'a [DateTime<Utc>],
        series: &'a [Series<'a>],
        kind: ChartKind,
    ) -> TimeChart<'a> {
        TimeChart {
            buckets: marks,
            series,
            kind,
            unit: YUnit::Count,
            tick_format: "%H:%M",
        }
    }

    #[test]
    fn nice_step_rounds_up_to_something_a_reader_can_divide() {
        assert_eq!(nice_step(4.0), 1.0);
        assert_eq!(nice_step(8.0), 2.0);
        assert_eq!(nice_step(17.0), 5.0);
        assert_eq!(nice_step(400.0), 100.0);
        assert_eq!(nice_step(4200.0), 2000.0);
    }

    #[test]
    fn an_idle_window_still_gets_a_scale() {
        // A 0-to-0 axis has no scale, and every mark drawn against it would claim full height.
        assert_eq!(nice_step(0.0), 1.0);

        let marks = buckets(4);
        let series = [Series {
            label: "completed",
            color: "var(--color-success)",
            values: vec![Some(0.0); 4],
        }];
        let frame = Frame::new(&chart(&marks, &series, ChartKind::StackedColumn));

        assert_eq!(frame.max, 4.0);
        assert_eq!(frame.y(0.0), frame.baseline());
    }

    #[test]
    fn a_counted_axis_never_steps_in_fractions() {
        // Three tasks over four ticks is 0.75 of a task per gridline, which is not a number of
        // tasks. The floor at one whole unit is what stops the axis reading 0 / 0.75 / 1.5.
        assert_eq!(nice_step(3.0), 1.0);
    }

    #[test]
    fn a_stack_is_scaled_against_its_total_and_a_line_against_its_largest() {
        let marks = buckets(2);
        let series = [
            Series {
                label: "completed",
                color: "a",
                values: vec![Some(6.0), Some(2.0)],
            },
            Series {
                label: "failed",
                color: "b",
                values: vec![Some(6.0), Some(1.0)],
            },
        ];

        assert_eq!(
            chart(&marks, &series, ChartKind::StackedColumn).peak(),
            12.0
        );
        assert_eq!(chart(&marks, &series, ChartKind::Line).peak(), 6.0);
    }

    #[test]
    fn the_newest_bucket_is_always_labelled() {
        // Anchored on the right: 12 buckets over a stride of 2 would otherwise leave the one the
        // reader looks at first without a time under it.
        assert!(labels_bucket(11, 12));
        assert!(labels_bucket(9, 12));
        assert!(!labels_bucket(10, 12));

        // Fewer buckets than the label budget: every one gets a label.
        assert!((0..6).all(|index| labels_bucket(index, 6)));
    }

    #[test]
    fn an_unmeasured_bucket_breaks_the_line_instead_of_diving_to_zero() {
        let marks = buckets(5);
        let series = [Series {
            label: "p50",
            color: "var(--color-primary)",
            values: vec![Some(100.0), None, Some(200.0), Some(300.0), None],
        }];
        let subject = chart(&marks, &series, ChartKind::Line);
        let runs = measured_runs(&subject, &Frame::new(&subject), &series[0]);

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 1);
        assert_eq!(runs[1].len(), 2);
    }

    #[test]
    fn a_line_ends_on_its_last_measured_bucket_not_its_last_bucket() {
        let series = Series {
            label: "p95",
            color: "c",
            values: vec![Some(10.0), Some(20.0), None],
        };

        assert_eq!(series.last_measured(), Some((1, 20.0)));
    }

    #[test]
    fn one_series_gets_no_legend_and_two_get_one() {
        let marks = buckets(2);
        let solo = [Series {
            label: "open",
            color: "a",
            values: vec![Some(1.0), Some(2.0)],
        }];
        assert_eq!(legend(&chart(&marks, &solo, ChartKind::Area)), "");

        let pair = [
            Series {
                label: "completed",
                color: "a",
                values: vec![Some(1.0), Some(2.0)],
            },
            Series {
                label: "failed",
                color: "b",
                values: vec![Some(0.0), Some(1.0)],
            },
        ];
        let rendered = legend(&chart(&marks, &pair, ChartKind::StackedColumn));
        assert!(rendered.contains("completed"));
        assert!(rendered.contains("failed"));
    }

    #[test]
    fn every_bucket_answers_when_pointed_at_including_the_empty_ones() {
        let marks = buckets(3);
        let series = [Series {
            label: "completed",
            color: "a",
            values: vec![Some(2.0), Some(0.0), Some(5.0)],
        }];
        let subject = chart(&marks, &series, ChartKind::StackedColumn);

        assert_eq!(
            hover_bands(&subject, &Frame::new(&subject))
                .matches("<title>")
                .count(),
            3
        );
        assert!(readout(&subject, 1).contains("0 completed"));
    }

    #[test]
    fn an_unmeasured_bucket_reads_as_unmeasured_rather_than_as_zero() {
        let marks = buckets(1);
        let series = [Series {
            label: "p50",
            color: "a",
            values: vec![None],
        }];
        let subject = TimeChart {
            unit: YUnit::Millis,
            ..chart(&marks, &series, ChartKind::Line)
        };

        assert!(subject.peak() == 0.0);
        assert!(readout(&subject, 0).contains("no p50"));
    }

    #[test]
    fn milliseconds_become_seconds_once_they_stop_being_readable() {
        assert_eq!(format_value(412.0, YUnit::Millis), "412ms");
        assert_eq!(format_value(1500.0, YUnit::Millis), "1.5s");
        assert_eq!(format_value(12_000.0, YUnit::Count), "12,000");
        assert_eq!(format_value(12.4, YUnit::Percent), "12%");
    }

    #[test]
    fn a_value_too_small_to_see_is_still_drawn() {
        // One task in a window whose peak is a thousand is a fraction of a pixel tall. It gets the
        // floor instead, because "one" and "none" must not look the same.
        let marks = buckets(2);
        let series = [Series {
            label: "completed",
            color: "a",
            values: vec![Some(1000.0), Some(1.0)],
        }];
        let subject = chart(&marks, &series, ChartKind::StackedColumn);
        let svg = stacked_columns(&subject, &Frame::new(&subject));

        assert_eq!(svg.matches("<path").count(), 2);
    }
}
