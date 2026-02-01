//! Module compilation orchestration.
//!
//! Coordinates parallel compilation of modules respecting dependency order.

use std::collections::HashMap;
use std::sync::Arc;

use bumpalo::Bump;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use url::Url;

use crate::database::Database;
use crate::error::DriverError;
use crate::graph::DepGraph;

/// Result of compiling a single module.
#[derive(Debug)]
pub enum ModuleResult {
    /// Module compiled successfully.
    Success {
        /// Number of declarations in the module.
        decl_count: usize,
    },
    /// Module failed to compile.
    Failed {
        /// Parse or other error message.
        message: String,
    },
}

/// Result of a full build.
#[derive(Debug)]
pub struct BuildResult {
    /// Results for each module.
    pub modules: HashMap<Url, ModuleResult>,

    /// Total number of modules processed.
    pub total: usize,

    /// Number of successful compilations.
    pub success: usize,

    /// Number of failed compilations.
    pub failed: usize,
}

impl BuildResult {
    /// Check if the build was completely successful.
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// Compile all modules in the dependency graph.
///
/// Modules are compiled in parallel within each dependency level.
/// A level N can be compiled once all levels < N have completed.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let levels = graph.levels();

    for level in levels {
        // Compile all modules at this level in parallel
        let level_results = compile_level(db.clone(), level).await;

        for (uri, result) in level_results {
            results.insert(uri, result);
        }
    }

    let total = results.len();
    let success = results
        .values()
        .filter(|r| matches!(r, ModuleResult::Success { .. }))
        .count();
    let failed = total - success;

    BuildResult {
        modules: results,
        total,
        success,
        failed,
    }
}

/// Compile all modules at a single dependency level in parallel.
async fn compile_level(db: Arc<Mutex<Database>>, modules: Vec<&Url>) -> Vec<(Url, ModuleResult)> {
    let mut set = JoinSet::new();

    for uri in modules {
        let uri = uri.clone();
        let db = db.clone();

        set.spawn(async move {
            let result = compile_module(&db, &uri).await;
            (uri, result)
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok((uri, result)) => results.push((uri, result)),
            Err(e) => {
                // Task panicked - should not happen in normal operation
                eprintln!("Task panicked: {:?}", e);
            }
        }
    }

    results
}

/// Compile a single module.
async fn compile_module(db: &Arc<Mutex<Database>>, uri: &Url) -> ModuleResult {
    // Get source content
    let source = {
        let mut db = db.lock().await;
        match db.source(uri).await {
            Ok(s) => s.to_string(),
            Err(e) => {
                return ModuleResult::Failed {
                    message: e.to_string(),
                };
            }
        }
    };

    // Parse the module
    let bump = Bump::new();
    let src = bump.alloc_str(&source);
    let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());

    match parser.module() {
        Ok(module) => {
            // Count declarations
            let decl_count = module.values.len() + module.unions.len() + module.aliases.len();

            ModuleResult::Success { decl_count }
        }
        Err(e) => ModuleResult::Failed {
            message: format!("{:?}", e),
        },
    }
}

/// Build a dependency graph from parsed modules.
///
/// This is a simplified implementation that parses modules to extract imports.
/// For a full implementation, we would parse just the header/imports.
pub async fn build_graph(
    db: Arc<Mutex<Database>>,
    modules: &[Url],
) -> Result<DepGraph, DriverError> {
    let mut graph = DepGraph::new();

    for uri in modules {
        // Parse module to get imports
        let source = {
            let mut db = db.lock().await;
            db.source(uri).await?.to_string()
        };

        let imports = extract_imports(&source, uri, modules);
        graph.add_module(uri.clone(), imports);
    }

    graph.compute_order()?;
    Ok(graph)
}

/// Extract import URIs from source code.
///
/// This is a simplified implementation - in production we'd use the parser.
fn extract_imports(source: &str, current: &Url, known_modules: &[Url]) -> Vec<Url> {
    let mut imports = Vec::new();

    // Parse to get imports
    let bump = Bump::new();
    let src = bump.alloc_str(source);
    let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());

    if let Ok(module) = parser.module() {
        for import in module.imports {
            let import_name = import.import.value;

            // Try to resolve import to a known module
            if let Some(uri) = resolve_import(import_name, current, known_modules) {
                imports.push(uri);
            }
        }
    }

    imports
}

/// Resolve an import name to a module URI.
///
/// This is a simplified implementation. Full resolution would handle:
/// - Package dependencies
/// - Source directory structure
/// - Module naming conventions
fn resolve_import(name: &str, _current: &Url, known_modules: &[Url]) -> Option<Url> {
    // Convert module name to file path pattern
    // e.g., "Json.Decode" -> "Json/Decode.nash"
    let path_pattern = format!("{}.nash", name.replace('.', "/"));

    // Find matching module
    known_modules
        .iter()
        .find(|uri| uri.path().ends_with(&path_pattern))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::InMemorySource;

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
    }

    #[tokio::test]
    async fn test_compile_single_module() {
        let mem = InMemorySource::new();
        let uri = url("Main.nash");
        mem.insert(
            uri.clone(),
            r#"
module Main exposing (..)

main = 42
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let result = compile_module(&db, &uri).await;

        match result {
            ModuleResult::Success { decl_count } => {
                assert_eq!(decl_count, 1); // main is one value declaration
            }
            ModuleResult::Failed { message } => {
                panic!("Expected success, got failure: {}", message);
            }
        }
    }

    #[tokio::test]
    async fn test_compile_invalid_module() {
        let mem = InMemorySource::new();
        let uri = url("Bad.nash");
        mem.insert(
            uri.clone(),
            "this is not valid nash syntax {{{{".to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let result = compile_module(&db, &uri).await;

        assert!(matches!(result, ModuleResult::Failed { .. }));
    }

    #[tokio::test]
    async fn test_build_simple_project() {
        let mem = InMemorySource::new();

        // Create two modules
        mem.insert(
            url("Utils.nash"),
            r#"
module Utils exposing (..)

helper = 1
"#
            .to_string(),
        );

        mem.insert(
            url("Main.nash"),
            r#"
module Main exposing (..)

import Utils

main = 42
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("Utils.nash"), url("Main.nash")];

        // Build graph
        let graph = build_graph(db.clone(), &modules).await.unwrap();

        // Build project
        let result = build(db, &graph).await;

        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }
}
