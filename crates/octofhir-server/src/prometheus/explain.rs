//! Explaining a PromQL query: its syntax tree, with every sub-expression evaluated on its own, so
//! one can see where a query gains or loses series (the idea of PromLens' tree view).

use futures::future::join_all;
use promql_parser::parser::{self, Expr, LabelModifier};
use serde::Serialize;
use serde_json::Value;

use super::client::PrometheusClient;
use super::error::MetricsError;

/// Most nodes evaluated per explain; deeper parts of a huge query are shown without results.
const MAX_EVALUATED: usize = 30;
/// Samples returned per node.
const MAX_SAMPLES: usize = 3;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainNode {
    /// The sub-expression as PromQL.
    pub expr: String,
    /// `aggregation`, `binary`, `function`, `selector`, `range-selector`, `subquery`, `unary`,
    /// `number` or `string`.
    pub kind: &'static str,
    /// Short name of the operation: `sum by (le)`, `rate`, `/`, the metric name, …
    pub label: String,
    pub children: Vec<ExplainNode>,
    /// Absent for literals and for nodes past [`MAX_EVALUATED`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<NodeResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeResult {
    /// Prometheus result type: `vector`, `matrix`, `scalar` or `string`.
    pub result_type: Option<String>,
    /// Number of series (1 for scalars).
    pub series: usize,
    pub samples: Vec<Value>,
    pub error: Option<String>,
}

/// Parses `query` (variables already substituted) into a tree.
pub fn tree(query: &str) -> Result<ExplainNode, MetricsError> {
    let expr = parser::parse(query).map_err(MetricsError::Query)?;
    Ok(node(&expr))
}

fn node(expr: &Expr) -> ExplainNode {
    let leaf = |kind, label: String| ExplainNode {
        expr: expr.to_string(),
        kind,
        label,
        children: Vec::new(),
        result: None,
    };
    match expr {
        // Parentheses only group; show what they contain.
        Expr::Paren(paren) => node(&paren.expr),
        Expr::Aggregate(agg) => {
            let modifier = match &agg.modifier {
                Some(LabelModifier::Include(labels)) => {
                    format!(" by ({})", labels.labels.join(", "))
                }
                Some(LabelModifier::Exclude(labels)) => {
                    format!(" without ({})", labels.labels.join(", "))
                }
                None => String::new(),
            };
            let mut children = Vec::new();
            if let Some(param) = &agg.param {
                children.push(node(param));
            }
            children.push(node(&agg.expr));
            ExplainNode {
                children,
                ..leaf("aggregation", format!("{}{modifier}", agg.op))
            }
        }
        Expr::Binary(binary) => ExplainNode {
            children: vec![node(&binary.lhs), node(&binary.rhs)],
            ..leaf("binary", binary.op.to_string())
        },
        Expr::Call(call) => ExplainNode {
            children: call.args.args.iter().map(|arg| node(arg)).collect(),
            ..leaf("function", call.func.name.to_string())
        },
        Expr::Unary(unary) => ExplainNode {
            children: vec![node(&unary.expr)],
            ..leaf("unary", "-".into())
        },
        Expr::Subquery(subquery) => ExplainNode {
            children: vec![node(&subquery.expr)],
            ..leaf("subquery", "subquery".into())
        },
        Expr::VectorSelector(vs) => leaf(
            "selector",
            vs.name.clone().unwrap_or_else(|| "series".into()),
        ),
        Expr::MatrixSelector(ms) => leaf(
            "range-selector",
            ms.vs.name.clone().unwrap_or_else(|| "series".into()),
        ),
        Expr::NumberLiteral(_) => leaf("number", expr.to_string()),
        Expr::StringLiteral(_) => leaf("string", expr.to_string()),
        Expr::Extension(_) => leaf("extension", "extension".into()),
    }
}

/// Evaluates every node of `root` (pre-order, up to [`MAX_EVALUATED`]) as an instant query.
pub async fn evaluate(client: &PrometheusClient, root: &mut ExplainNode, time: Option<f64>) {
    let mut exprs = Vec::new();
    collect(root, &mut exprs);
    let pending: Vec<(usize, String)> = exprs
        .into_iter()
        .enumerate()
        .filter_map(|(index, expr)| expr.map(|e| (index, e)))
        .take(MAX_EVALUATED)
        .collect();
    let outcomes = join_all(pending.iter().map(|(_, expr)| client.query(expr, time))).await;
    let mut results: Vec<Option<NodeResult>> = Vec::new();
    for ((index, _), outcome) in pending.iter().zip(outcomes) {
        results.resize_with(index + 1, || None);
        results[*index] = Some(match outcome {
            Ok(data) => summarize(&data),
            Err(error) => NodeResult {
                result_type: None,
                series: 0,
                samples: Vec::new(),
                error: Some(error.to_string()),
            },
        });
    }
    let mut next = 0;
    assign(root, &mut results, &mut next);
}

/// Pre-order list of the expressions to evaluate; `None` for literals.
fn collect(node: &ExplainNode, out: &mut Vec<Option<String>>) {
    let evaluable = !matches!(node.kind, "number" | "string" | "extension");
    out.push(evaluable.then(|| node.expr.clone()));
    for child in &node.children {
        collect(child, out);
    }
}

/// Hands results back in the same pre-order [`collect`] produced them.
fn assign(node: &mut ExplainNode, results: &mut [Option<NodeResult>], next: &mut usize) {
    if let Some(slot) = results.get_mut(*next) {
        node.result = slot.take();
    }
    *next += 1;
    for child in &mut node.children {
        assign(child, results, next);
    }
}

fn summarize(data: &Value) -> NodeResult {
    let result_type = data
        .get("resultType")
        .and_then(Value::as_str)
        .map(str::to_string);
    let result = data.get("result");
    let (series, samples) = match (result_type.as_deref(), result) {
        (Some("scalar") | Some("string"), Some(value)) => (1, vec![value.clone()]),
        (_, Some(Value::Array(items))) => (
            items.len(),
            items.iter().take(MAX_SAMPLES).cloned().collect(),
        ),
        _ => (0, Vec::new()),
    };
    NodeResult {
        result_type,
        series,
        samples,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_tree_without_parentheses() {
        let root = tree(
            "histogram_quantile(0.95, sum by (le) (rate(octofhir_http_request_duration_seconds_bucket[5m])))",
        )
        .unwrap();
        assert_eq!(root.kind, "function");
        assert_eq!(root.label, "histogram_quantile");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].kind, "number");
        let sum = &root.children[1];
        assert_eq!(
            (sum.kind, sum.label.as_str()),
            ("aggregation", "sum by (le)")
        );
        let rate = &sum.children[0];
        assert_eq!(rate.label, "rate");
        assert_eq!(rate.children[0].kind, "range-selector");
        assert_eq!(
            rate.children[0].label,
            "octofhir_http_request_duration_seconds_bucket"
        );
    }

    #[test]
    fn binary_operations_have_both_sides() {
        let root = tree("sum(rate(a[5m])) / sum(rate(b[5m]))").unwrap();
        assert_eq!((root.kind, root.label.as_str()), ("binary", "/"));
        assert_eq!(root.children.len(), 2);
        assert!(root.children[1].expr.contains('b'));
    }

    #[test]
    fn collects_every_node_once_and_assigns_in_order() {
        let mut root = tree("sum(rate(a[5m])) / 2").unwrap();
        let mut exprs = Vec::new();
        collect(&root, &mut exprs);
        assert_eq!(exprs.len(), 5);
        assert_eq!(exprs[4], None, "the literal is not evaluated");
        let mut results: Vec<Option<NodeResult>> = (0..5)
            .map(|i| {
                Some(NodeResult {
                    result_type: None,
                    series: i,
                    samples: Vec::new(),
                    error: None,
                })
            })
            .collect();
        let mut next = 0;
        assign(&mut root, &mut results, &mut next);
        assert_eq!(root.result.as_ref().unwrap().series, 0);
        assert_eq!(
            root.children[0].children[0].result.as_ref().unwrap().series,
            2
        );
        assert_eq!(root.children[1].result.as_ref().unwrap().series, 4);
    }

    #[test]
    fn rejects_invalid_queries() {
        assert!(tree("sum(").is_err());
    }
}
