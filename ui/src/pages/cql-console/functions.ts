/**
 * Curated CQL function reference used by the console's function palette.
 * Each entry inserts a Monaco snippet at the cursor (placeholders use ${n:..}).
 */

export interface CqlFn {
  name: string;
  /** Display signature, e.g. "Count(list)". */
  signature: string;
  /** Short description shown on hover. */
  doc: string;
  /** Monaco snippet inserted at cursor. */
  snippet: string;
}

export interface CqlFnCategory {
  label: string;
  functions: CqlFn[];
}

export const CQL_FUNCTIONS: CqlFnCategory[] = [
  {
    label: "Aggregates",
    functions: [
      {
        name: "Count",
        signature: "Count(list)",
        doc: "Number of items in a list.",
        snippet: "Count(${1:list})",
      },
      {
        name: "Sum",
        signature: "Sum(list)",
        doc: "Sum of numeric items.",
        snippet: "Sum(${1:list})",
      },
      {
        name: "Avg",
        signature: "Avg(list)",
        doc: "Arithmetic mean of numeric items.",
        snippet: "Avg(${1:list})",
      },
      { name: "Min", signature: "Min(list)", doc: "Smallest item.", snippet: "Min(${1:list})" },
      { name: "Max", signature: "Max(list)", doc: "Largest item.", snippet: "Max(${1:list})" },
      {
        name: "Median",
        signature: "Median(list)",
        doc: "Median of numeric items.",
        snippet: "Median(${1:list})",
      },
    ],
  },
  {
    label: "Lists",
    functions: [
      {
        name: "First",
        signature: "First(list)",
        doc: "First item of a list.",
        snippet: "First(${1:list})",
      },
      {
        name: "Last",
        signature: "Last(list)",
        doc: "Last item of a list.",
        snippet: "Last(${1:list})",
      },
      {
        name: "exists",
        signature: "exists(list)",
        doc: "True if the list has any item.",
        snippet: "exists(${1:list})",
      },
      {
        name: "distinct",
        signature: "distinct list",
        doc: "Remove duplicate items.",
        snippet: "distinct ${1:list}",
      },
      {
        name: "flatten",
        signature: "flatten list",
        doc: "Flatten a list of lists.",
        snippet: "flatten ${1:list}",
      },
      {
        name: "singleton from",
        signature: "singleton from list",
        doc: "The sole element of a list.",
        snippet: "singleton from ${1:list}",
      },
      {
        name: "IndexOf",
        signature: "IndexOf(list, e)",
        doc: "0-based index of an element.",
        snippet: "IndexOf(${1:list}, ${2:element})",
      },
    ],
  },
  {
    label: "Intervals",
    functions: [
      {
        name: "Interval",
        signature: "Interval[low, high]",
        doc: "Construct a closed interval.",
        snippet: "Interval[${1:low}, ${2:high}]",
      },
      {
        name: "start of",
        signature: "start of interval",
        doc: "Lower boundary of an interval.",
        snippet: "start of ${1:interval}",
      },
      {
        name: "end of",
        signature: "end of interval",
        doc: "Upper boundary of an interval.",
        snippet: "end of ${1:interval}",
      },
      {
        name: "width of",
        signature: "width of interval",
        doc: "Width (high − low) of an interval.",
        snippet: "width of ${1:interval}",
      },
      {
        name: "in",
        signature: "x in interval",
        doc: "True if a point falls inside the interval.",
        snippet: "${1:point} in ${2:interval}",
      },
      {
        name: "overlaps",
        signature: "a overlaps b",
        doc: "True if two intervals overlap.",
        snippet: "${1:a} overlaps ${2:b}",
      },
    ],
  },
  {
    label: "Clinical / dates",
    functions: [
      {
        name: "AgeInYears",
        signature: "AgeInYears()",
        doc: "Patient age in years (needs Patient context).",
        snippet: "AgeInYears()",
      },
      {
        name: "AgeInYearsAt",
        signature: "AgeInYearsAt(date)",
        doc: "Patient age in years at a given date.",
        snippet: "AgeInYearsAt(${1:date})",
      },
      { name: "Today", signature: "Today()", doc: "Current date.", snippet: "Today()" },
      { name: "Now", signature: "Now()", doc: "Current date and time.", snippet: "Now()" },
      {
        name: "CalculateAgeInYears",
        signature: "CalculateAgeInYears(birthDate)",
        doc: "Age in years from a birth date.",
        snippet: "CalculateAgeInYears(${1:birthDate})",
      },
    ],
  },
  {
    label: "Strings",
    functions: [
      {
        name: "Combine",
        signature: "Combine(list, sep)",
        doc: "Join a list of strings with a separator.",
        snippet: "Combine(${1:list}, ${2:', '})",
      },
      {
        name: "Split",
        signature: "Split(s, sep)",
        doc: "Split a string into a list.",
        snippet: "Split(${1:string}, ${2:','})",
      },
      {
        name: "Substring",
        signature: "Substring(s, start)",
        doc: "Substring from start (0-based).",
        snippet: "Substring(${1:string}, ${2:0})",
      },
      {
        name: "Upper",
        signature: "Upper(s)",
        doc: "Uppercase a string.",
        snippet: "Upper(${1:string})",
      },
      {
        name: "Lower",
        signature: "Lower(s)",
        doc: "Lowercase a string.",
        snippet: "Lower(${1:string})",
      },
      {
        name: "Length",
        signature: "Length(s)",
        doc: "Length of a string or list.",
        snippet: "Length(${1:value})",
      },
    ],
  },
  {
    label: "Logic & conversion",
    functions: [
      {
        name: "if then else",
        signature: "if c then a else b",
        doc: "Conditional expression.",
        snippet: "if ${1:cond} then ${2:a} else ${3:b}",
      },
      {
        name: "case",
        signature: "case when … then … end",
        doc: "Multi-branch conditional.",
        snippet: "case when ${1:cond} then ${2:a} else ${3:b} end",
      },
      {
        name: "Coalesce",
        signature: "Coalesce(a, b, …)",
        doc: "First non-null argument.",
        snippet: "Coalesce(${1:a}, ${2:b})",
      },
      {
        name: "ToString",
        signature: "ToString(x)",
        doc: "Convert a value to a string.",
        snippet: "ToString(${1:value})",
      },
      {
        name: "ToInteger",
        signature: "ToInteger(x)",
        doc: "Convert a value to an integer.",
        snippet: "ToInteger(${1:value})",
      },
      {
        name: "ToDecimal",
        signature: "ToDecimal(x)",
        doc: "Convert a value to a decimal.",
        snippet: "ToDecimal(${1:value})",
      },
    ],
  },
];
