//! HTML templates for OAuth authorize flow.
//!
//! Server-rendered HTML templates for login, consent, and error pages
//! that match the existing UI design (glassmorphism, Indigo/Blue primary colors).

/// Shared CSS styles for all OAuth pages.
const SHARED_STYLES: &str = r#"
:root {
    /* Brand colors */
    --brand-primary: #3b3fe3;
    --brand-primary-light: #5e85ff;
    --brand-fire: #ff4d3d;
    --brand-fire-bg: rgba(255, 77, 61, 0.15);

    /* Surface colors (dark mode) */
    --surface-1: #0d0e1a;
    --surface-2: #141629;
    --surface-3: #1c1f40;

    /* Glass effect */
    --glass-bg: rgba(20, 22, 41, 0.8);
    --glass-border: rgba(255, 255, 255, 0.08);
    --glass-blur: 16px;

    /* Text colors */
    --text-primary: #f8f9fe;
    --text-secondary: #adb5bd;
    --text-dimmed: #6c757d;

    /* Border */
    --border-subtle: rgba(255, 255, 255, 0.08);

    /* Spacing */
    --radius-md: 6px;
    --radius-lg: 8px;
    --radius-xl: 12px;
}

* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: radial-gradient(circle at top left, #1c1f40, #0d0e1a);
    min-height: 100vh;
    display: flex;
    justify-content: center;
    align-items: center;
    color: var(--text-primary);
    line-height: 1.5;
}

.container {
    width: 100%;
    max-width: 420px;
    padding: 1rem;
}

.logo-section {
    text-align: center;
    margin-bottom: 1.5rem;
}

.logo-section img {
    height: 120px;
    width: auto;
    margin-bottom: 0.5rem;
}

.logo-section .subtitle {
    font-size: 0.875rem;
    color: var(--text-dimmed);
}

.card {
    background: var(--glass-bg);
    backdrop-filter: blur(var(--glass-blur));
    border: 1px solid var(--glass-border);
    border-radius: var(--radius-xl);
    padding: 1.5rem;
}

.card-title {
    font-family: "Outfit", "Inter", sans-serif;
    font-size: 1.25rem;
    font-weight: 600;
    margin-bottom: 1rem;
    color: var(--text-primary);
}

.form-group {
    margin-bottom: 1rem;
}

.form-label {
    display: block;
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 0.25rem;
}

.form-input {
    width: 100%;
    padding: 0.625rem 0.75rem;
    background: var(--surface-2);
    border: 1px solid var(--glass-border);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: 0.875rem;
    transition: border-color 0.15s ease;
}

.form-input:focus {
    outline: none;
    border-color: var(--brand-primary);
}

.form-input::placeholder {
    color: var(--text-dimmed);
}

.btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    padding: 0.625rem 1rem;
    border: none;
    border-radius: var(--radius-md);
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
}

.btn-primary {
    background: var(--brand-primary);
    color: white;
}

.btn-primary:hover {
    background: var(--brand-primary-light);
    transform: translateY(-1px);
}

.btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
    transform: none;
}

.btn-danger {
    background: transparent;
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
}

.btn-danger:hover {
    border-color: var(--brand-fire);
    color: var(--brand-fire);
}

.btn-group {
    display: flex;
    gap: 0.75rem;
    margin-top: 1rem;
}

.btn-group .btn {
    flex: 1;
}

.alert {
    padding: 0.75rem 1rem;
    border-radius: var(--radius-md);
    font-size: 0.875rem;
    margin-bottom: 1rem;
}

.alert-error {
    background: var(--brand-fire-bg);
    border: 1px solid var(--brand-fire);
    color: var(--brand-fire);
}

.hint {
    font-size: 0.75rem;
    color: var(--text-dimmed);
    text-align: center;
    margin-top: 1rem;
}

/* Consent-specific styles */
.client-info {
    background: var(--surface-2);
    border-radius: var(--radius-lg);
    padding: 1rem;
    margin-bottom: 1rem;
}

.client-name {
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 0.25rem;
}

.client-redirect {
    font-size: 0.75rem;
    color: var(--text-dimmed);
    word-break: break-all;
}

.scope-list {
    list-style: none;
    margin: 1rem 0;
}

.scope-item {
    display: flex;
    align-items: flex-start;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border-subtle);
}

.scope-item:last-child {
    border-bottom: none;
}

.scope-icon {
    width: 20px;
    height: 20px;
    margin-right: 0.75rem;
    flex-shrink: 0;
    color: var(--brand-primary-light);
}

.scope-name {
    font-weight: 500;
    color: var(--text-primary);
}

.scope-description {
    font-size: 0.75rem;
    color: var(--text-secondary);
}

.warning-box {
    background: rgba(168, 132, 120, 0.15);
    border: 1px solid #a88478;
    border-radius: var(--radius-md);
    padding: 0.75rem 1rem;
    font-size: 0.75rem;
    color: #a88478;
    margin-bottom: 1rem;
}

/* Error page styles */
.error-icon {
    width: 64px;
    height: 64px;
    color: var(--brand-fire);
    margin-bottom: 1rem;
}

.error-title {
    font-family: "Outfit", "Inter", sans-serif;
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 0.5rem;
}

.error-description {
    color: var(--text-secondary);
    font-size: 0.875rem;
    margin-bottom: 1.5rem;
}

.error-code {
    font-family: "JetBrains Mono", "SF Mono", monospace;
    font-size: 0.75rem;
    color: var(--text-dimmed);
    background: var(--surface-2);
    padding: 0.25rem 0.5rem;
    border-radius: var(--radius-md);
}
"#;

/// Logo SVG (uses CSS variables for colors)
const LOGO_SVG_120: &str = "<svg width=\"120\" height=\"120\" viewBox=\"0 0 120 120\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\">\
    <circle cx=\"60\" cy=\"60\" r=\"50\" fill=\"var(--brand-primary)\" fill-opacity=\"0.2\"/>\
    <circle cx=\"60\" cy=\"60\" r=\"30\" stroke=\"url(#logo-gradient)\" stroke-width=\"3\"/>\
    <defs>\
        <linearGradient id=\"logo-gradient\" x1=\"0\" y1=\"0\" x2=\"120\" y2=\"120\" gradientUnits=\"userSpaceOnUse\">\
            <stop stop-color=\"var(--brand-primary)\"/>\
            <stop offset=\"1\" stop-color=\"var(--brand-fire)\"/>\
        </linearGradient>\
    </defs>\
</svg>";

/// Logo SVG (smaller version)
const LOGO_SVG_80: &str = "<svg width=\"80\" height=\"80\" viewBox=\"0 0 120 120\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\">\
    <circle cx=\"60\" cy=\"60\" r=\"50\" fill=\"var(--brand-primary)\" fill-opacity=\"0.2\"/>\
    <circle cx=\"60\" cy=\"60\" r=\"30\" stroke=\"url(#logo-gradient-consent)\" stroke-width=\"3\"/>\
    <defs>\
        <linearGradient id=\"logo-gradient-consent\" x1=\"0\" y1=\"0\" x2=\"120\" y2=\"120\" gradientUnits=\"userSpaceOnUse\">\
            <stop stop-color=\"var(--brand-primary)\"/>\
            <stop offset=\"1\" stop-color=\"var(--brand-fire)\"/>\
        </linearGradient>\
    </defs>\
</svg>";

/// Check icon SVG for scope items
const CHECK_ICON_SVG: &str = r#"<svg class="scope-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <path d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"/>
</svg>"#;

/// Warning icon SVG for error page
const WARNING_ICON_SVG: &str = r#"<svg class="error-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="margin: 0 auto 1rem;">
    <path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z"/>
</svg>"#;

/// Base HTML template wrapper.
fn html_page(title: &str, content: &str) -> String {
    let mut html = String::with_capacity(content.len() + 2000);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("    <meta charset=\"UTF-8\">\n");
    html.push_str(
        "    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n",
    );
    html.push_str("    <title>");
    html.push_str(&html_escape(title));
    html.push_str(" - OctoFHIR</title>\n");
    html.push_str("    <link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n");
    html.push_str("    <link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n");
    html.push_str("    <link href=\"https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=Outfit:wght@500;600&display=swap\" rel=\"stylesheet\">\n");
    html.push_str("    <style>");
    html.push_str(SHARED_STYLES);
    html.push_str("</style>\n</head>\n<body>\n    <div class=\"container\">\n");
    html.push_str(content);
    html.push_str("\n    </div>\n</body>\n</html>");
    html
}

/// Renders the login form.
///
/// # Arguments
///
/// * `client_name` - Display name of the OAuth client
/// * `session_id` - Session ID for the hidden form field
/// * `error` - Optional error message to display
pub fn render_login_form(client_name: &str, session_id: &str, error: Option<&str>) -> String {
    let mut content = String::with_capacity(4096);

    // Logo section
    content.push_str("<div class=\"logo-section\">\n");
    content.push_str(LOGO_SVG_120);
    content.push_str("\n<div class=\"subtitle\">Sign in to continue</div>\n</div>\n\n");

    // Card
    content.push_str("<div class=\"card\">\n");
    content.push_str("<div class=\"card-title\">Sign in to ");
    content.push_str(&html_escape(client_name));
    content.push_str("</div>\n\n");

    // Error message if any
    if let Some(e) = error {
        content.push_str("<div class=\"alert alert-error\">");
        content.push_str(&html_escape(e));
        content.push_str("</div>\n\n");
    }

    // Form
    content.push_str("<form method=\"POST\">\n");
    content.push_str("<input type=\"hidden\" name=\"action\" value=\"login\">\n");
    content.push_str("<input type=\"hidden\" name=\"session_id\" value=\"");
    content.push_str(&html_escape(session_id));
    content.push_str("\">\n\n");

    // Username field
    content.push_str("<div class=\"form-group\">\n");
    content.push_str("<label class=\"form-label\" for=\"username\">Username</label>\n");
    content
        .push_str("<input type=\"text\" id=\"username\" name=\"username\" class=\"form-input\" ");
    content.push_str("placeholder=\"Enter your username\" required autocomplete=\"username\">\n");
    content.push_str("</div>\n\n");

    // Password field
    content.push_str("<div class=\"form-group\">\n");
    content.push_str("<label class=\"form-label\" for=\"password\">Password</label>\n");
    content.push_str(
        "<input type=\"password\" id=\"password\" name=\"password\" class=\"form-input\" ",
    );
    content.push_str(
        "placeholder=\"Enter your password\" required autocomplete=\"current-password\">\n",
    );
    content.push_str("</div>\n\n");

    // Submit button
    content.push_str("<button type=\"submit\" class=\"btn btn-primary\">Sign in</button>\n");
    content.push_str("</form>\n\n");

    // Hint
    content.push_str("<div class=\"hint\">Use your server credentials to continue</div>\n");
    content.push_str("</div>");

    html_page("Sign In", &content)
}

/// Scope information for consent display.
pub struct ScopeInfo {
    pub name: String,
    pub description: String,
}

/// Get human-readable scope information.
pub fn get_scope_info(scope: &str) -> ScopeInfo {
    match scope {
        "openid" => ScopeInfo {
            name: "OpenID Connect".to_string(),
            description: "Verify your identity".to_string(),
        },
        "profile" => ScopeInfo {
            name: "Profile".to_string(),
            description: "Access your basic profile information".to_string(),
        },
        "fhirUser" => ScopeInfo {
            name: "FHIR User".to_string(),
            description: "Access your FHIR user identity".to_string(),
        },
        "launch" => ScopeInfo {
            name: "Launch Context".to_string(),
            description: "Receive the EHR launch context".to_string(),
        },
        "launch/patient" => ScopeInfo {
            name: "Patient Context".to_string(),
            description: "Access the current patient context".to_string(),
        },
        "launch/encounter" => ScopeInfo {
            name: "Encounter Context".to_string(),
            description: "Access the current encounter context".to_string(),
        },
        "offline_access" => ScopeInfo {
            name: "Offline Access".to_string(),
            description: "Maintain access when you're not using the app".to_string(),
        },
        s if s.starts_with("patient/") => {
            let resource = s.strip_prefix("patient/").unwrap_or(s);
            let (resource_type, action) = if let Some((r, a)) = resource.rsplit_once('.') {
                (r, a)
            } else {
                (resource, "*")
            };
            let action_desc = match action {
                "read" | "r" => "read",
                "write" | "w" => "write",
                "c" => "create",
                "u" => "update",
                "d" => "delete",
                "s" => "search",
                "*" | "rs" | "cruds" => "full access to",
                _ => action,
            };
            ScopeInfo {
                name: format!("Patient {}", resource_type),
                description: format!(
                    "Allow {} {} data in the patient context",
                    action_desc, resource_type
                ),
            }
        }
        s if s.starts_with("user/") => {
            let resource = s.strip_prefix("user/").unwrap_or(s);
            let (resource_type, action) = if let Some((r, a)) = resource.rsplit_once('.') {
                (r, a)
            } else {
                (resource, "*")
            };
            let action_desc = match action {
                "read" | "r" => "read",
                "write" | "w" => "write",
                "c" => "create",
                "u" => "update",
                "d" => "delete",
                "s" => "search",
                "*" | "rs" | "cruds" => "full access to",
                _ => action,
            };
            ScopeInfo {
                name: format!("User {}", resource_type),
                description: format!(
                    "Allow {} {} data based on your permissions",
                    action_desc, resource_type
                ),
            }
        }
        s if s.starts_with("system/") => {
            let resource = s.strip_prefix("system/").unwrap_or(s);
            ScopeInfo {
                name: format!("System {}", resource),
                description: "System-level access".to_string(),
            }
        }
        other => ScopeInfo {
            name: other.to_string(),
            description: format!("Access to {}", other),
        },
    }
}

/// Renders the consent form.
///
/// # Arguments
///
/// * `client_name` - Display name of the OAuth client
/// * `client_uri` - Client redirect URI for display
/// * `scopes` - List of requested scopes
/// * `session_id` - Session ID for the hidden form field
pub fn render_consent_form(
    client_name: &str,
    client_uri: &str,
    scopes: &[&str],
    session_id: &str,
) -> String {
    let mut content = String::with_capacity(4096);

    // Logo section
    content.push_str("<div class=\"logo-section\">\n");
    content.push_str(LOGO_SVG_80);
    content.push_str("\n</div>\n\n");

    // Card
    content.push_str("<div class=\"card\">\n");
    content.push_str("<div class=\"card-title\">Authorize Access</div>\n\n");

    // Client info
    content.push_str("<div class=\"client-info\">\n");
    content.push_str("<div class=\"client-name\">");
    content.push_str(&html_escape(client_name));
    content.push_str("</div>\n");
    content.push_str("<div class=\"client-redirect\">");
    content.push_str(&html_escape(client_uri));
    content.push_str("</div>\n</div>\n\n");

    // Permission text
    content.push_str("<p style=\"font-size: 0.875rem; color: var(--text-secondary); margin-bottom: 0.75rem;\">\n");
    content.push_str("This application is requesting permission to:\n</p>\n\n");

    // Scope list
    content.push_str("<ul class=\"scope-list\">\n");
    for scope in scopes {
        let info = get_scope_info(scope);
        content.push_str("<li class=\"scope-item\">\n");
        content.push_str(CHECK_ICON_SVG);
        content.push_str("\n<div>\n<div class=\"scope-name\">");
        content.push_str(&html_escape(&info.name));
        content.push_str("</div>\n<div class=\"scope-description\">");
        content.push_str(&html_escape(&info.description));
        content.push_str("</div>\n</div>\n</li>\n");
    }
    content.push_str("</ul>\n\n");

    // Warning box
    content.push_str("<div class=\"warning-box\">\n");
    content.push_str(
        "By authorizing, you allow this application to access your data as described above.\n",
    );
    content.push_str("</div>\n\n");

    // Form with buttons
    content.push_str("<form method=\"POST\">\n");
    content.push_str("<input type=\"hidden\" name=\"session_id\" value=\"");
    content.push_str(&html_escape(session_id));
    content.push_str("\">\n\n");
    content.push_str("<div class=\"btn-group\">\n");
    content.push_str("<button type=\"submit\" name=\"action\" value=\"deny\" class=\"btn btn-danger\">Deny</button>\n");
    content.push_str("<button type=\"submit\" name=\"action\" value=\"authorize\" class=\"btn btn-primary\">Authorize</button>\n");
    content.push_str("</div>\n</form>\n");
    content.push_str("</div>");

    html_page("Authorize", &content)
}

/// Renders an error page (used when redirect_uri is invalid).
///
/// # Arguments
///
/// * `error_code` - OAuth error code (e.g., "invalid_request")
/// * `error_description` - Human-readable error description
pub fn render_error_page(error_code: &str, error_description: &str) -> String {
    let mut content = String::with_capacity(1024);

    content.push_str("<div class=\"card\" style=\"text-align: center;\">\n");
    content.push_str(WARNING_ICON_SVG);
    content.push_str("\n\n<div class=\"error-title\">Authorization Error</div>\n");
    content.push_str("<div class=\"error-description\">");
    content.push_str(&html_escape(error_description));
    content.push_str("</div>\n\n");
    content.push_str("<div class=\"error-code\">");
    content.push_str(&html_escape(error_code));
    content.push_str("</div>\n</div>");

    html_page("Error", &content)
}

/// Patient info for the patient picker.
pub struct PatientInfo {
    /// FHIR resource ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Date of birth (optional).
    pub birth_date: Option<String>,
}

/// Renders the patient picker form for standalone launch.
///
/// # Arguments
///
/// * `patients` - List of available patients
/// * `session_id` - Session ID for the hidden form field
pub fn render_patient_picker(patients: &[PatientInfo], session_id: &str) -> String {
    let mut content = String::with_capacity(4096);

    // Logo section
    content.push_str("<div class=\"logo-section\">\n");
    content.push_str(LOGO_SVG_80);
    content.push_str("\n</div>\n\n");

    // Card
    content.push_str("<div class=\"card\">\n");
    content.push_str("<div class=\"card-title\">Select Patient</div>\n\n");

    content.push_str(
        "<p style=\"font-size: 0.875rem; color: var(--text-secondary); margin-bottom: 1rem;\">\n",
    );
    content.push_str("Choose the patient context for this application:\n</p>\n\n");

    // Form
    content.push_str("<form method=\"POST\">\n");
    content.push_str("<input type=\"hidden\" name=\"action\" value=\"select_patient\">\n");
    content.push_str("<input type=\"hidden\" name=\"session_id\" value=\"");
    content.push_str(&html_escape(session_id));
    content.push_str("\">\n\n");

    // Patient list as radio buttons
    content.push_str("<div style=\"margin-bottom: 1rem;\">\n");
    for (i, patient) in patients.iter().enumerate() {
        let checked = if i == 0 { " checked" } else { "" };
        content.push_str("<label style=\"display: flex; align-items: center; padding: 0.75rem; background: var(--surface-2); border-radius: var(--radius-lg); margin-bottom: 0.5rem; cursor: pointer; border: 1px solid var(--border-subtle); transition: border-color 0.15s;\">\n");
        content.push_str("<input type=\"radio\" name=\"patient_id\" value=\"");
        content.push_str(&html_escape(&patient.id));
        content.push_str("\"");
        content.push_str(checked);
        content
            .push_str(" style=\"margin-right: 0.75rem; accent-color: var(--brand-primary);\">\n");
        content.push_str("<div>\n<div style=\"font-weight: 500; color: var(--text-primary);\">");
        content.push_str(&html_escape(&patient.name));
        content.push_str("</div>\n");
        if let Some(dob) = &patient.birth_date {
            content.push_str("<div style=\"font-size: 0.75rem; color: var(--text-dimmed);\">DOB: ");
            content.push_str(&html_escape(dob));
            content.push_str("</div>\n");
        }
        content.push_str("<div style=\"font-size: 0.75rem; color: var(--text-dimmed);\">ID: ");
        content.push_str(&html_escape(&patient.id));
        content.push_str("</div>\n</div>\n</label>\n");
    }
    content.push_str("</div>\n\n");

    // Submit button
    content.push_str("<button type=\"submit\" class=\"btn btn-primary\">Continue</button>\n");
    content.push_str("</form>\n");
    content.push_str("</div>");

    html_page("Select Patient", &content)
}

/// Simple HTML escaping to prevent XSS.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_login_form_with_error() {
        let html = render_login_form("Test App", "session-123", Some("Invalid credentials"));
        assert!(html.contains("Invalid credentials"));
        assert!(html.contains("alert-error"));
    }

    #[test]
    fn test_render_consent_form() {
        let html = render_consent_form(
            "Test App",
            "https://example.com/callback",
            &["openid", "patient/*.read"],
            "session-456",
        );
        assert!(html.contains("Test App"));
        assert!(html.contains("session-456"));
        assert!(html.contains("Authorize"));
        assert!(html.contains("OpenID Connect"));
    }

    #[test]
    fn test_render_error_page() {
        let html = render_error_page("invalid_request", "Missing required parameter");
        assert!(html.contains("invalid_request"));
        assert!(html.contains("Missing required parameter"));
        assert!(html.contains("Authorization Error"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_get_scope_info() {
        let info = get_scope_info("openid");
        assert_eq!(info.name, "OpenID Connect");

        let info = get_scope_info("patient/Observation.read");
        assert!(info.name.contains("Observation"));
        assert!(info.description.contains("read"));
    }
}
