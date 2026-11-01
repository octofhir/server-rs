/**
 * PromQL functions, aggregations and keywords for completion and hover. Signatures follow the
 * Prometheus documentation; descriptions are short summaries.
 */

export interface PromqlFunction {
  name: string;
  signature: string;
  doc: string;
  /** Snippet inserted on completion. */
  snippet: string;
}

function fn(name: string, args: string, doc: string, snippet?: string): PromqlFunction {
  return { name, signature: `${name}(${args})`, doc, snippet: snippet ?? `${name}($0)` };
}

export const FUNCTIONS: PromqlFunction[] = [
  fn("rate", "v range-vector", "Per-second average rate of increase of a counter over the range.", "rate($1[${2:\\$__rate_interval}])$0"),
  fn("irate", "v range-vector", "Per-second rate from the last two samples in the range; reacts fast, noisy.", "irate($1[${2:\\$__rate_interval}])$0"),
  fn("increase", "v range-vector", "Increase of a counter over the range, extrapolated to its edges.", "increase($1[${2:\\$__range}])$0"),
  fn("delta", "v range-vector", "Difference between the first and last value of a gauge over the range."),
  fn("idelta", "v range-vector", "Difference between the last two samples of a gauge in the range."),
  fn("deriv", "v range-vector", "Per-second derivative of a gauge, by simple linear regression."),
  fn("predict_linear", "v range-vector, t scalar", "Predicted value of a gauge t seconds from now, by linear regression."),
  fn("resets", "v range-vector", "Number of counter resets within the range."),
  fn("changes", "v range-vector", "Number of times the value changed within the range."),
  fn(
    "histogram_quantile",
    "φ scalar, b instant-vector",
    "φ-quantile (0 ≤ φ ≤ 1) from histogram buckets; aggregate with `sum by (le)` first.",
    "histogram_quantile(${1:0.95}, sum by (le) (rate(${2}_bucket[${3:\\$__rate_interval}])))$0"
  ),
  fn("histogram_count", "v instant-vector", "Observation count of native histograms."),
  fn("histogram_sum", "v instant-vector", "Sum of observations of native histograms."),
  fn("histogram_fraction", "lower scalar, upper scalar, v instant-vector", "Fraction of observations between two values."),
  fn("avg_over_time", "v range-vector", "Average of all points in the range."),
  fn("min_over_time", "v range-vector", "Minimum of all points in the range."),
  fn("max_over_time", "v range-vector", "Maximum of all points in the range."),
  fn("sum_over_time", "v range-vector", "Sum of all points in the range."),
  fn("count_over_time", "v range-vector", "Number of points in the range."),
  fn("quantile_over_time", "φ scalar, v range-vector", "φ-quantile of the points in the range."),
  fn("stddev_over_time", "v range-vector", "Population standard deviation of the points in the range."),
  fn("last_over_time", "v range-vector", "Most recent point in the range."),
  fn("present_over_time", "v range-vector", "1 for any series with a point in the range."),
  fn("absent", "v instant-vector", "1-element vector when v has no elements, otherwise empty; for alerting on missing series."),
  fn("absent_over_time", "v range-vector", "1-element vector when no series has points in the range."),
  fn("abs", "v instant-vector", "Absolute value."),
  fn("ceil", "v instant-vector", "Rounds up to the nearest integer."),
  fn("floor", "v instant-vector", "Rounds down to the nearest integer."),
  fn("round", "v instant-vector, to_nearest=1 scalar", "Rounds to the nearest multiple of to_nearest."),
  fn("clamp", "v instant-vector, min scalar, max scalar", "Clamps values to [min, max]."),
  fn("clamp_min", "v instant-vector, min scalar", "Clamps values to at least min."),
  fn("clamp_max", "v instant-vector, max scalar", "Clamps values to at most max."),
  fn("exp", "v instant-vector", "Exponential function."),
  fn("ln", "v instant-vector", "Natural logarithm."),
  fn("log2", "v instant-vector", "Binary logarithm."),
  fn("log10", "v instant-vector", "Decimal logarithm."),
  fn("sqrt", "v instant-vector", "Square root."),
  fn("sgn", "v instant-vector", "Sign: 1, 0 or -1."),
  fn("scalar", "v instant-vector", "The single element's value as a scalar; NaN unless v has exactly one element."),
  fn("vector", "s scalar", "The scalar as a vector without labels."),
  fn("time", "", "Seconds since the epoch at evaluation time.", "time()"),
  fn("timestamp", "v instant-vector", "Timestamp of each sample."),
  fn("sort", "v instant-vector", "Sorts ascending by value (instant queries)."),
  fn("sort_desc", "v instant-vector", "Sorts descending by value (instant queries)."),
  fn("sort_by_label", "v instant-vector, label string, ...", "Sorts by the given label values."),
  fn(
    "label_replace",
    "v instant-vector, dst string, replacement string, src string, regex string",
    "Sets label dst to replacement where src matches regex ($1 refers to groups).",
    'label_replace($1, "${2:dst}", "${3:\\$1}", "${4:src}", "${5:(.*)}")$0'
  ),
  fn("label_join", "v instant-vector, dst string, separator string, src string, ...", "Joins the values of src labels into dst."),
  fn("day_of_week", "v=vector(time()) instant-vector", "Day of the week in UTC (0 = Sunday)."),
  fn("hour", "v=vector(time()) instant-vector", "Hour of the day in UTC."),
  fn("minute", "v=vector(time()) instant-vector", "Minute of the hour in UTC."),
];

export const AGGREGATIONS: PromqlFunction[] = [
  fn("sum", "v instant-vector", "Sum over dimensions; add `by (label)` to keep labels.", "sum by (${1:label}) ($0)"),
  fn("avg", "v instant-vector", "Average over dimensions."),
  fn("min", "v instant-vector", "Minimum over dimensions."),
  fn("max", "v instant-vector", "Maximum over dimensions."),
  fn("count", "v instant-vector", "Number of series."),
  fn("group", "v instant-vector", "1 for each group; keeps only labels."),
  fn("stddev", "v instant-vector", "Population standard deviation over dimensions."),
  fn("stdvar", "v instant-vector", "Population variance over dimensions."),
  fn("topk", "k scalar, v instant-vector", "Largest k elements by value.", "topk(${1:10}, $0)"),
  fn("bottomk", "k scalar, v instant-vector", "Smallest k elements by value.", "bottomk(${1:10}, $0)"),
  fn("quantile", "φ scalar, v instant-vector", "φ-quantile over dimensions.", "quantile(${1:0.95}, $0)"),
  fn("count_values", "label string, v instant-vector", "Number of series per distinct value."),
];

export const KEYWORDS = [
  { name: "by", doc: "Keep only these labels in the aggregation." },
  { name: "without", doc: "Drop these labels in the aggregation, keep the rest." },
  { name: "on", doc: "Match series of a binary operation only on these labels." },
  { name: "ignoring", doc: "Match series of a binary operation ignoring these labels." },
  { name: "group_left", doc: "Many-to-one match: each series on the left may match one on the right." },
  { name: "group_right", doc: "One-to-many match: each series on the right may match one on the left." },
  { name: "offset", doc: "Shift the selector back in time, e.g. `offset 1d`." },
  { name: "bool", doc: "Comparison returns 0 or 1 instead of filtering." },
  { name: "and", doc: "Intersection of two vectors." },
  { name: "or", doc: "Union of two vectors." },
  { name: "unless", doc: "Elements of the left vector without a match on the right." },
];

/** Dashboard variables the server substitutes before a query runs. */
export const BUILTIN_VARIABLES = [
  { name: "$__rate_interval", doc: "Rate window covering at least four scrapes and one step. Use in `rate(x[$__rate_interval])`." },
  { name: "$__interval", doc: "The chart's step." },
  { name: "$__range", doc: "The dashboard's time range, e.g. for `increase(x[$__range])` in tables." },
];

export const DURATIONS = ["$__rate_interval", "$__range", "$__interval", "1m", "5m", "15m", "1h", "6h", "1d"];

const ALL = new Map([...FUNCTIONS, ...AGGREGATIONS].map((item) => [item.name, item]));

export function lookupFunction(name: string): PromqlFunction | undefined {
  return ALL.get(name);
}
