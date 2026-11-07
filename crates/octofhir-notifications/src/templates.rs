use std::collections::HashMap;

use crate::adapters::RenderedContent;
use crate::error::NotificationError;

/// Simple template renderer using {{variable}} syntax
pub struct TemplateRenderer {
    templates: HashMap<String, Template>,
}

#[derive(Debug, Clone)]
pub struct Template {
    pub id: String,
    pub subject: Option<String>,
    pub body: String,
    pub html_body: Option<String>,
}

impl TemplateRenderer {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn register(&mut self, template: Template) {
        self.templates.insert(template.id.clone(), template);
    }

    pub fn get(&self, template_id: &str) -> Option<&Template> {
        self.templates.get(template_id)
    }

    pub fn render(
        &self,
        template_id: &str,
        data: &HashMap<String, serde_json::Value>,
    ) -> Result<RenderedContent, NotificationError> {
        let template = self
            .templates
            .get(template_id)
            .ok_or(NotificationError::TemplateNotFound(template_id.to_string()))?;
        Ok(render_template(template, data))
    }
}

/// `template` with `{{name}}` replaced by the values of `data`. In an HTML body the values are
/// escaped, so data cannot add markup.
pub fn render_template(
    template: &Template,
    data: &HashMap<String, serde_json::Value>,
) -> RenderedContent {
    let escaped: HashMap<String, serde_json::Value> = data
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                serde_json::Value::String(escape_html(&text(value))),
            )
        })
        .collect();
    RenderedContent {
        subject: template.subject.as_ref().map(|s| render_string(s, data)),
        body: render_string(&template.body, data),
        html_body: template
            .html_body
            .as_ref()
            .map(|s| render_string(s, &escaped)),
    }
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => value.to_string(),
    }
}

impl Template {
    /// A `NotificationTemplate` resource as a template; `None` when it is inactive or has no
    /// body. `contentType` `text/html` makes the body the HTML one.
    pub fn from_resource(id: &str, resource: &serde_json::Value) -> Option<Self> {
        if resource["active"] == serde_json::Value::Bool(false) {
            return None;
        }
        let body = resource["body"].as_str()?.to_owned();
        let html = resource["contentType"].as_str() == Some("text/html");
        Some(Self {
            id: id.to_owned(),
            subject: resource["subject"].as_str().map(str::to_owned),
            html_body: html.then(|| body.clone()),
            body,
        })
    }
}

fn render_string(template: &str, data: &HashMap<String, serde_json::Value>) -> String {
    let mut result = template.to_string();
    for (key, value) in data {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, &text(value));
    }
    result
}

impl Default for TemplateRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_cannot_add_markup_to_an_html_body() {
        let template = Template {
            id: "t".to_owned(),
            subject: Some("Hi {{name}}".to_owned()),
            body: "Hi {{name}}".to_owned(),
            html_body: Some("<p>Hi {{name}}</p>".to_owned()),
        };
        let data = HashMap::from([("name".to_owned(), serde_json::json!("<b>Eve</b>"))]);
        let rendered = render_template(&template, &data);
        assert_eq!(rendered.body, "Hi <b>Eve</b>");
        assert_eq!(
            rendered.html_body.as_deref(),
            Some("<p>Hi &lt;b&gt;Eve&lt;/b&gt;</p>")
        );
    }

    #[test]
    fn test_render_template() {
        let mut renderer = TemplateRenderer::new();
        renderer.register(Template {
            id: "test".to_string(),
            subject: Some("Hello {{name}}".to_string()),
            body: "Your appointment is on {{date}}".to_string(),
            html_body: None,
        });

        let mut data = HashMap::new();
        data.insert("name".to_string(), serde_json::json!("John"));
        data.insert("date".to_string(), serde_json::json!("2024-01-15"));

        let result = renderer.render("test", &data).unwrap();
        assert_eq!(result.subject.unwrap(), "Hello John");
        assert_eq!(result.body, "Your appointment is on 2024-01-15");
    }

    #[test]
    fn test_render_with_numbers() {
        let mut renderer = TemplateRenderer::new();
        renderer.register(Template {
            id: "test".to_string(),
            subject: None,
            body: "You have {{count}} messages".to_string(),
            html_body: None,
        });

        let mut data = HashMap::new();
        data.insert("count".to_string(), serde_json::json!(5));

        let result = renderer.render("test", &data).unwrap();
        assert_eq!(result.body, "You have 5 messages");
    }

    #[test]
    fn test_template_not_found() {
        let renderer = TemplateRenderer::new();
        let data = HashMap::new();

        let result = renderer.render("nonexistent", &data);
        assert!(matches!(
            result,
            Err(NotificationError::TemplateNotFound(_))
        ));
    }
}
