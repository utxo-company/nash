//! Module compilation orchestration.
//!
//! Coordinates parallel compilation of modules respecting dependency order.

use std::collections::{BTreeMap, HashMap};
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

    /// Warnings collected during canonicalization.
    pub warnings: Vec<String>,
}

impl BuildResult {
    /// Check if the build was completely successful.
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// Compile all modules in the dependency graph.
///
/// Modules are compiled level-by-level. Sources for each level are fetched
/// in parallel, then compiled. Interfaces from earlier levels are passed to
/// later levels for import resolution.
///
/// Within-level compilation is currently sequential because `Bump` is `!Sync`
/// and `Context<'a>` ties the arena lifetime to the interface lifetime.
/// Real parallelism would require either splitting those lifetimes or using
/// per-module arenas with a post-level interface extraction step.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    // Shared arena for interfaces that must outlive individual module compilations.
    let shared_bump = Bump::new();
    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut interfaces: BTreeMap<&str, nash_can::Interface<'_>> = BTreeMap::new();
    let mut all_warnings: Vec<String> = Vec::new();
    let levels = graph.levels();

    for level in levels {
        // Pre-fetch all sources for this level in parallel
        let sources = fetch_sources(&db, &level).await;

        // Compile each module (sequential — see doc comment above)
        for (uri, source) in sources {
            let result = compile_module(
                &uri,
                &source,
                &shared_bump,
                &mut interfaces,
                &mut all_warnings,
            );
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
        warnings: all_warnings,
    }
}

/// Fetch source content for all modules in a level, in parallel.
async fn fetch_sources(
    db: &Arc<Mutex<Database>>,
    uris: &[&Url],
) -> Vec<(Url, Result<String, String>)> {
    let mut set = JoinSet::new();

    for &uri in uris {
        let uri = uri.clone();
        let db = db.clone();
        set.spawn(async move {
            let source = {
                let mut db = db.lock().await;
                db.source(&uri).await.map(|s| s.to_string())
            };
            (uri, source.map_err(|e| e.to_string()))
        });
    }

    let mut results = Vec::with_capacity(uris.len());
    while let Some(res) = set.join_next().await {
        if let Ok(r) = res {
            results.push(r);
        }
    }
    results
}

/// Compile a single module: parse, canonicalize, extract interface.
fn compile_module<'a>(
    _uri: &Url,
    source: &Result<String, String>,
    shared_bump: &'a Bump,
    interfaces: &mut BTreeMap<&'a str, nash_can::Interface<'a>>,
    warnings: &mut Vec<String>,
) -> ModuleResult {
    let source = match source {
        Ok(s) => s,
        Err(e) => {
            return ModuleResult::Failed { message: e.clone() };
        }
    };

    // Allocate source into shared bump so it outlives the parse
    let src: &str = shared_bump.alloc_str(source);
    let mut parser = nash_parse::Parser::new(shared_bump, src.as_bytes());

    let module = match parser.module() {
        Ok(m) => m,
        Err(e) => {
            return ModuleResult::Failed {
                message: format!("{:?}", e),
            };
        }
    };

    // Build canonicalization context with interfaces from already-compiled modules
    let context = nash_can::Context {
        package: None,
        interfaces: if interfaces.is_empty() {
            None
        } else {
            Some(&*shared_bump.alloc(interfaces.clone()))
        },
    };

    match nash_can::canonicalize(shared_bump, context, &module) {
        Ok(can_result) => {
            for w in &can_result.warnings {
                warnings.push(format!("{:?}", w));
            }

            // Extract interface and store it for downstream modules
            let interface = nash_can::from_module(shared_bump, &can_result.module);
            let module_name: &str = shared_bump.alloc_str(can_result.module.name.name);
            interfaces.insert(module_name, interface);

            let decl_count = count_decls(can_result.module.decls);
            ModuleResult::Success { decl_count }
        }
        Err(errors) => ModuleResult::Failed {
            message: format!("{:?}", errors),
        },
    }
}

fn count_decls(decls: &nash_ast::Decls<'_>) -> usize {
    match decls {
        nash_ast::Decls::Declare { next, .. } => 1 + count_decls(next),
        nash_ast::Decls::DeclareRec {
            following, next, ..
        } => 1 + following.len() + count_decls(next),
        nash_ast::Decls::Empty => 0,
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
        let modules = vec![uri];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 1);
        assert_eq!(result.success, 1);
        assert!(result.is_success());
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
        let modules = vec![uri];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 1);
        assert_eq!(result.failed, 1);
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

main = Utils.helper
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
