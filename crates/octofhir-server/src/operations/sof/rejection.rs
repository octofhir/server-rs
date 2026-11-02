//! Rejections of the SQL on FHIR operations as the specification words them:
//! the status, the `issue.code` and the `expression` naming the parameter at
//! fault (`$sql-run` / `$sql-export` "Error Handling", Common Operation
//! Behavior "Status code for a value that cannot be resolved").

use axum::http::StatusCode;
use octofhir_api::{ApiError, OperationOutcomeIssue};

use crate::operations::OperationError;

/// A rejection with one issue.
pub(crate) fn rejected(
    status: StatusCode,
    code: &'static str,
    expression: Option<&str>,
    message: impl Into<String>,
) -> OperationError {
    OperationError::Api(ApiError::issues(
        status,
        vec![OperationOutcomeIssue::error(code, message, expression)],
    ))
}

/// `error` as the issue the operation answers with, its `expression` set to
/// `expression`: a 400 is `invalid` (`not-supported` when the operation does
/// not support what was asked), a 404 `not-found`, a 422 `invalid`. Other
/// errors (authorization, server failures) pass through unchanged.
pub(crate) fn at(error: OperationError, expression: &str) -> OperationError {
    match issue_of(&error, Some(expression)) {
        Some((status, issue)) => OperationError::Api(ApiError::issues(status, vec![issue])),
        None => error,
    }
}

/// `error` with `expression` only when it is a 422 (the subject cannot be
/// processed); anything else unchanged.
pub(crate) fn at_subject_if_unprocessable(error: OperationError) -> OperationError {
    match &error {
        OperationError::Api(ApiError::UnprocessableEntity { .. }) => at(error, "subject"),
        _ => error,
    }
}

/// Two failures reported together with the status of the first (the more
/// fundamental one, e.g. an unresolvable subject before an unresolvable
/// filter).
pub(crate) fn together(first: OperationError, second: OperationError) -> OperationError {
    let issues = |error: &OperationError| -> Vec<OperationOutcomeIssue> {
        match error {
            OperationError::Api(ApiError::Issues { issues, .. }) => issues.clone(),
            other => issue_of(other, None)
                .map(|(_, issue)| vec![issue])
                .unwrap_or_default(),
        }
    };
    let status = match &first {
        OperationError::Api(api) => api.status_code(),
        other => match issue_of(other, None) {
            Some((status, _)) => status,
            None => return first,
        },
    };
    let mut all = issues(&first);
    all.extend(issues(&second));
    OperationError::Api(ApiError::issues(status, all))
}

/// Several failures as one rejection, each issue's `expression` its own
/// (e.g. `subject[2]`): the status is the most fundamental one (404, then
/// 422, then 400). A failure that is not a rejection (authorization, a server
/// error) is returned as it is.
pub(crate) fn all_together(mut problems: Vec<(String, OperationError)>) -> OperationError {
    if problems.len() == 1 {
        let (expression, error) = problems.remove(0);
        return at(error, &expression);
    }
    let mut issues = Vec::with_capacity(problems.len());
    let mut statuses = Vec::with_capacity(problems.len());
    for (expression, error) in problems {
        match issue_of(&error, Some(&expression)) {
            Some((status, issue)) => {
                statuses.push(status);
                issues.push(issue);
            }
            None => return error,
        }
    }
    let status = [
        StatusCode::NOT_FOUND,
        StatusCode::UNPROCESSABLE_ENTITY,
        StatusCode::BAD_REQUEST,
    ]
    .into_iter()
    .find(|candidate| statuses.contains(candidate))
    .unwrap_or(StatusCode::BAD_REQUEST);
    OperationError::Api(ApiError::issues(status, issues))
}

fn issue_of(
    error: &OperationError,
    expression: Option<&str>,
) -> Option<(StatusCode, OperationOutcomeIssue)> {
    let (status, code, message) = match error {
        OperationError::InvalidParameters(message) => {
            (StatusCode::BAD_REQUEST, "invalid", message.clone())
        }
        OperationError::NotSupported(message) => {
            (StatusCode::BAD_REQUEST, "not-supported", message.clone())
        }
        OperationError::NotFound(message) => (StatusCode::NOT_FOUND, "not-found", message.clone()),
        OperationError::Api(ApiError::UnprocessableEntity { message, .. }) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid", message.clone())
        }
        OperationError::Api(ApiError::Issues { status, issues, .. }) => {
            let mut issue = issues.first()?.clone();
            if let Some(expression) = expression {
                issue.expression = vec![expression.to_owned()];
            }
            return Some((*status, issue));
        }
        _ => return None,
    };
    Some((
        status,
        OperationOutcomeIssue::error(code, message, expression),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(error: OperationError) -> (StatusCode, serde_json::Value) {
        match error {
            OperationError::Api(api) => (
                api.status_code(),
                serde_json::to_value(api.to_operation_outcome()).unwrap(),
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejections_carry_code_and_expression() {
        let (status, body) = outcome(at(OperationError::NotSupported("no".into()), "_format"));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["issue"][0]["code"], "not-supported");
        assert_eq!(body["issue"][0]["expression"][0], "_format");

        let (status, body) = outcome(together(
            at(OperationError::NotFound("subject gone".into()), "subject"),
            rejected(
                StatusCode::BAD_REQUEST,
                "not-found",
                Some("patient"),
                "patient gone",
            ),
        ));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["issue"].as_array().unwrap().len(), 2);
        assert_eq!(body["issue"][1]["expression"][0], "patient");

        let forbidden = OperationError::Api(ApiError::forbidden("no"));
        assert!(matches!(
            at(forbidden, "subject"),
            OperationError::Api(ApiError::Forbidden(_))
        ));
    }
}
