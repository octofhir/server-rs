//! TypeScript to JavaScript transpiler.
//!
//! This module provides TypeScript transpilation using SWC (Speedy Web Compiler).
//! It strips type annotations and transforms TypeScript-specific syntax to JavaScript.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_jsc::transpile_typescript;
//!
//! let ts_code = r#"
//!     const patient: Patient = event.resource;
//!     fhir.create<Task>({ resourceType: 'Task', status: 'requested' });
//! "#;
//!
//! let js_code = transpile_typescript(ts_code)?;
//! ```

use swc_common::{sync::Lrc, FileName, Globals, Mark, SourceMap, GLOBALS};
use swc_ecma_ast::{EsVersion, Program};
use swc_ecma_codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_base::{fixer::fixer, resolver};
use swc_ecma_transforms_typescript::strip;
use swc_ecma_visit::VisitMutWith;
use thiserror::Error;

/// Error type for transpilation failures.
#[derive(Error, Debug)]
pub enum TranspileError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Transform error: {0}")]
    Transform(String),

    #[error("Codegen error: {0}")]
    Codegen(String),
}

/// Result of TypeScript transpilation.
#[derive(Debug)]
pub struct TranspileResult {
    /// The transpiled JavaScript code.
    pub code: String,
    /// Source map (if generated).
    pub source_map: Option<String>,
}

/// Transpile TypeScript code to JavaScript.
///
/// This function:
/// - Parses TypeScript code
/// - Strips type annotations
/// - Transforms TypeScript-specific syntax (enums, namespaces, etc.)
/// - Generates JavaScript ES2020 output
///
/// # Arguments
///
/// * `source` - TypeScript source code
///
/// # Returns
///
/// Transpiled JavaScript code or an error.
pub fn transpile_typescript(source: &str) -> Result<TranspileResult, TranspileError> {
    transpile_typescript_with_options(source, TranspileOptions::default())
}

/// Options for TypeScript transpilation.
#[derive(Debug, Clone)]
pub struct TranspileOptions {
    /// Target ECMAScript version. Default: ES2020.
    pub target: EsVersion,
    /// Generate source map. Default: false.
    pub source_map: bool,
    /// File name for error messages. Default: "automation.ts".
    pub filename: String,
}

impl Default for TranspileOptions {
    fn default() -> Self {
        Self {
            target: EsVersion::Es2020,
            source_map: false,
            filename: "automation.ts".to_string(),
        }
    }
}

/// Transpile TypeScript code to JavaScript with custom options.
pub fn transpile_typescript_with_options(
    source: &str,
    options: TranspileOptions,
) -> Result<TranspileResult, TranspileError> {
    // Create source map
    let cm: Lrc<SourceMap> = Default::default();

    // Create source file
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom(options.filename.clone())),
        source.to_string(),
    );

    // TypeScript parser configuration
    let syntax = Syntax::Typescript(TsSyntax {
        tsx: false,
        decorators: true,
        dts: false,
        no_early_errors: false,
        disallow_ambiguous_jsx_like: false,
    });

    // Create lexer and parser
    let lexer = Lexer::new(syntax, options.target, StringInput::from(&*fm), None);

    let mut parser = Parser::new_from(lexer);

    // Parse the module
    let module = parser.parse_module().map_err(|e| {
        TranspileError::Parse(format!("Failed to parse TypeScript: {:?}", e.kind()))
    })?;

    // Check for parse errors
    for _e in parser.take_errors() {
        // Errors are collected but we continue
    }

    // Transform the AST - wrap in Program for Pass trait
    let mut program = Program::Module(module);

    GLOBALS.set(&Globals::default(), || {
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();

        // Apply resolver transform using VisitMut
        program.visit_mut_with(&mut resolver(unresolved_mark, top_level_mark, true));

        // Strip TypeScript types using mutate
        program.mutate(&mut strip(unresolved_mark, top_level_mark));

        // Apply fixer transform
        program.visit_mut_with(&mut fixer(None));
    });

    // Extract module back from Program
    let module = match program {
        Program::Module(m) => m,
        Program::Script(_) => {
            return Err(TranspileError::Transform(
                "Expected module, got script".to_string(),
            ));
        }
    };

    // Generate JavaScript code
    let mut buf = vec![];
    let mut src_map_buf = vec![];

    {
        let writer = JsWriter::new(
            cm.clone(),
            "\n",
            &mut buf,
            if options.source_map {
                Some(&mut src_map_buf)
            } else {
                None
            },
        );

        let codegen_config = CodegenConfig::default()
            .with_target(options.target)
            .with_ascii_only(false)
            .with_minify(false)
            .with_omit_last_semi(false);

        let mut emitter = Emitter {
            cfg: codegen_config,
            cm: cm.clone(),
            comments: None,
            wr: writer,
        };

        emitter
            .emit_module(&module)
            .map_err(|e| TranspileError::Codegen(format!("Failed to emit code: {}", e)))?;
    }

    let code = String::from_utf8(buf)
        .map_err(|e| TranspileError::Codegen(format!("Invalid UTF-8 output: {}", e)))?;

    let source_map = if options.source_map && !src_map_buf.is_empty() {
        let mut map_buf = vec![];
        cm.build_source_map(
            &src_map_buf,
            None,
            swc_common::source_map::DefaultSourceMapGenConfig,
        )
        .to_writer(&mut map_buf)
        .ok();
        String::from_utf8(map_buf).ok()
    } else {
        None
    };

    Ok(TranspileResult { code, source_map })
}

/// Check if code appears to be TypeScript (has type annotations).
///
/// This is a heuristic check - it looks for common TypeScript patterns.
pub fn is_typescript(source: &str) -> bool {
    // Look for common TypeScript patterns
    let ts_patterns = [
        ": string",
        ": number",
        ": boolean",
        ": any",
        ": void",
        ": null",
        ": undefined",
        ": never",
        ": unknown",
        ": object",
        "interface ",
        "type ",
        "enum ",
        "as const",
        "as string",
        "as number",
        "<T>",
        "<T,",
        ": T",
        "readonly ",
        "private ",
        "protected ",
        "public ",
        "implements ",
        "extends ",
        "namespace ",
        "declare ",
        "?:", // Optional property
        "!:", // Non-null assertion in property
    ];

    for pattern in ts_patterns {
        if source.contains(pattern) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_simple_typescript() {
        let ts_code = r#"
            const name: string = "test";
            const count: number = 42;
            console.log(name, count);
        "#;

        let result = transpile_typescript(ts_code).expect("transpile failed");

        // Should not contain type annotations
        assert!(!result.code.contains(": string"));
        assert!(!result.code.contains(": number"));

        // Should contain variable declarations
        assert!(result.code.contains("const name"));
        assert!(result.code.contains("const count"));
    }

    #[test]
    fn test_transpile_interface_and_type() {
        let ts_code = r#"
            interface Patient {
                id: string;
                name: string;
            }

            type Status = "active" | "inactive";

            const patient: Patient = { id: "123", name: "John" };
            const status: Status = "active";
        "#;

        let result = transpile_typescript(ts_code).expect("transpile failed");

        // Interfaces and types should be stripped
        assert!(!result.code.contains("interface Patient"));
        assert!(!result.code.contains("type Status"));

        // Object literals should remain
        assert!(result.code.contains("const patient"));
    }

    #[test]
    fn test_transpile_generics() {
        let ts_code = r#"
            function identity<T>(value: T): T {
                return value;
            }

            const result = identity<string>("hello");
        "#;

        let result = transpile_typescript(ts_code).expect("transpile failed");

        // Generic type parameter should be stripped
        assert!(!result.code.contains("<T>"));
        assert!(!result.code.contains("<string>"));

        // Function should remain
        assert!(result.code.contains("function identity"));
    }

    #[test]
    fn test_transpile_automation_script() {
        let ts_code = r#"
            // Automation script with TypeScript
            const patient: Patient = event.resource as Patient;

            if (patient.active) {
                const task = fhir.create<Task>({
                    resourceType: 'Task',
                    status: 'requested',
                    intent: 'order',
                    description: `Welcome ${patient.name?.[0]?.family}`
                });

                console.log('Created task:', task.id);
                return { taskId: task.id };
            }
        "#;

        let result = transpile_typescript(ts_code).expect("transpile failed");

        // Type annotations should be stripped
        assert!(!result.code.contains(": Patient"));
        assert!(!result.code.contains("as Patient"));
        assert!(!result.code.contains("<Task>"));

        // Logic should remain
        assert!(result.code.contains("event.resource"));
        assert!(result.code.contains("fhir.create"));
    }

    #[test]
    fn test_is_typescript_detection() {
        // TypeScript code
        assert!(is_typescript("const x: string = 'hello';"));
        assert!(is_typescript("interface Foo { bar: number }"));
        assert!(is_typescript("type Status = 'active' | 'inactive';"));
        assert!(is_typescript("function foo<T>(x: T): T { return x; }"));
        assert!(is_typescript("const arr: readonly string[] = [];"));

        // JavaScript code
        assert!(!is_typescript("const x = 'hello';"));
        assert!(!is_typescript("function foo(x) { return x; }"));
        assert!(!is_typescript("const obj = { bar: 42 };"));
    }

    #[test]
    fn test_transpile_preserves_javascript() {
        let js_code = r#"
            const name = "test";
            const count = 42;
            console.log(name, count);
        "#;

        let result = transpile_typescript(js_code).expect("transpile failed");

        // JavaScript should pass through mostly unchanged
        assert!(result.code.contains("const name"));
        assert!(result.code.contains("const count"));
    }

    #[test]
    fn test_transpile_error_handling() {
        let invalid_ts = r#"
            const x: = "invalid syntax";
        "#;

        let result = transpile_typescript(invalid_ts);
        assert!(result.is_err());
    }
}
