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

        let subject = template
            .subject
            .as_ref()
            .map(|s| self.render_string(s, data));
        let body = self.render_string(&template.body, data);
        let html_body = template
            .html_body
            .as_ref()
            .map(|s| self.render_string(s, data));

        Ok(RenderedContent {
            subject,
            body,
            html_body,
        })
    }

    fn render_string(&self, template: &str, data: &HashMap<String, serde_json::Value>) -> String {
        let mut result = template.to_string();

        for (key, value) in data {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                _ => value.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }

        result
    }
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
