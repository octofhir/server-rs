fn main() {
    #[cfg(feature = "core")]
    {
        let reference = octofhir_core::parse_reference("Patient/123", None).unwrap();
        assert_eq!(reference, octofhir_core::parse_reference("Patient/123", None).unwrap());
        assert!(octofhir_core::validate_id("patient-123").is_ok());
    }
    #[cfg(feature = "storage")]
    {
        let params = octofhir_storage::SearchParams::new().with_param("_id", "123");
        assert_eq!(params.parameters["_id"], ["123"]);
    }
    #[cfg(feature = "search")]
    {
        use octofhir_search::{SearchParameterRegistry, build_native_ir_query_from_params, parse_query_string, register_common_parameters};
        let registry = SearchParameterRegistry::new();
        register_common_parameters(&registry);
        let params = parse_query_string("_id=patient-123&_count=5", 10, 100);
        let query = build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap().builder.build().unwrap();
        assert!(query.sql.contains("patient"));
        assert!(query.sql.contains("$1"));
        assert!(!query.sql.contains("patient-123"));
        assert!(!query.params.is_empty());
    }
    #[cfg(feature = "graphql")]
    {
        assert!(matches!(octofhir_graphql::GraphQLContextBuilder::new().build(), Err(octofhir_graphql::context::ContextBuilderError::MissingField("storage"))));
        let operation = octofhir_graphql::resolvers::access::map_graphql_to_fhir_operation("PatientCreate").unwrap();
        assert_eq!(operation, octofhir_graphql::authorization::FhirOperation::Create);
    }

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
