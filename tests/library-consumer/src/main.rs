fn main() {
    #[cfg(feature = "cql")]
    {
        use octofhir_cql_service::library_cache::{CompiledLibrary, LibraryCache};
        let cache = LibraryCache::new(2);
        let source = "library PatientCheck version '1'\ndefine Ready: true";
        cache.put(std::sync::Arc::new(CompiledLibrary {
            url: "https://example.org/Library/PatientCheck".into(),
            version: "1".into(),
            elm: serde_json::json!({"library": {"identifier": {"id": "PatientCheck", "version": "1"}}}),
            cql_source: source.into(),
        }));
        assert_eq!(
            cache
                .get("https://example.org/Library/PatientCheck", "1")
                .unwrap()
                .cql_source,
            source
        );
        assert!(
            cache
                .get("https://example.org/Library/PatientCheck", "2")
                .is_none()
        );
    }

    #[cfg(feature = "api")]
    {
        use octofhir_api::{ApiError, RawJson};
        let source =
            r#"{"resourceType":"Observation","valueQuantity":{"value":0.1234567890123456789}}"#;
        let raw = RawJson::from_string(source);
        assert_eq!(serde_json::to_string(&raw).unwrap(), source);
        assert_eq!(
            raw.get_str_field("resourceType").as_deref(),
            Some("Observation")
        );
        let error = ApiError::not_found("Пациент");
        assert_eq!(error.status_code().as_u16(), 404);
        assert_eq!(error.to_operation_outcome().issue[0].code, "not-found");
    }
    #[cfg(feature = "config")]
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            use octofhir_config::{ConfigCategory, ConfigurationManager, Secrets};
            let manager = ConfigurationManager::builder()
                .with_secrets(Secrets::new([7; 32], "consumer"))
                .build()
                .await
                .unwrap();
            manager
                .set_config(
                    ConfigCategory::Server,
                    "consumer",
                    serde_json::json!("Пациент"),
                    None,
                    false,
                    None,
                )
                .await
                .unwrap();
            assert_eq!(
                manager.get_category(ConfigCategory::Server).await.unwrap()["consumer"],
                "Пациент"
            );
            let secrets = manager.secrets().unwrap();
            let value = secrets.encrypt("日本語").await.unwrap();
            assert_eq!(secrets.decrypt(&value).await.unwrap(), "日本語");
            assert!(
                !manager
                    .delete_config(ConfigCategory::Server, "consumer")
                    .await
                    .unwrap()
            );
        });
    #[cfg(feature = "notifications")]
    {
        use octofhir_notifications::{Template, TemplateRenderer};
        let mut renderer = TemplateRenderer::new();
        renderer.register(Template {
            id: "welcome".into(),
            subject: None,
            body: "Hello {{name}}".into(),
            html_body: None,
        });
        let data = std::collections::HashMap::from([(
            "name".into(),
            serde_json::json!("Пациент — 日本語"),
        )]);
        assert_eq!(
            renderer.render("welcome", &data).unwrap().body,
            "Hello Пациент — 日本語"
        );
        assert!(renderer.render("missing", &data).is_err());
    }
}
