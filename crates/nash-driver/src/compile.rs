//! Module compilation orchestration.
//!
//! Currently each module is parsed and canonicalized independently.
//! Cross-module compilation is intentionally absent: interfaces only
//! exist for type-solved modules (Elm's `Interface.fromModule` takes the
//! solver's annotations), so wiring imports back up is blocked on
//! `nash-constrain`/`nash-solve`. Once those land, the pipeline becomes
//! parse -> canonicalize -> constrain -> solve -> interface, compiled in
//! dependency order.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// Holds the output of compiling a single module.
struct CompileOutput {
    uri: Url,
    result: ModuleResult,
    warnings: Vec<String>,
}

/// Parse and canonicalize all modules.
///
/// The async part only fetches sources; the CPU-bound compilation runs on
/// tokio's blocking pool (`spawn_blocking`) so no executor worker is ever
/// stalled. Modules are independent (see module docs), so they all compile
/// in parallel on a bounded pool of worker threads.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    let modules: Vec<&Url> = graph.levels().into_iter().flatten().collect();
    let sources = fetch_sources(&db, &modules).await;

    tokio::task::spawn_blocking(move || build_sync(sources))
        .await
        .expect("compile task panicked")
}

fn build_sync(sources: Vec<(Url, Result<String, String>)>) -> BuildResult {
    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut all_warnings: Vec<String> = Vec::new();

    for output in compile_all(&sources) {
        all_warnings.extend(output.warnings);
        results.insert(output.uri, output.result);
    }

    let total = results.len();
    let success = results
        .values()
        .filter(|r| matches!(r, ModuleResult::Success { .. }))
        .count();

    BuildResult {
        modules: results,
        total,
        success,
        failed: total - success,
        warnings: all_warnings,
    }
}

/// Compile all modules on a bounded pool of scoped threads.
///
/// Workers pull the next module index from a shared counter, so at most
/// `available_parallelism` OS threads exist regardless of project size.
/// Outputs are re-sorted by index to keep results deterministic.
fn compile_all(sources: &[(Url, Result<String, String>)]) -> Vec<CompileOutput> {
    if sources.is_empty() {
        return Vec::new();
    }

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(sources.len());
    let next = AtomicUsize::new(0);

    let mut indexed: Vec<(usize, CompileOutput)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..worker_count)
            .map(|_| {
                let next = &next;
                scope.spawn(move || {
                    let mut outputs = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((uri, src)) = sources.get(index) else {
                            break;
                        };
                        outputs.push((index, compile_module(uri, src)));
                    }
                    outputs
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("compile worker panicked"))
            .collect()
    });

    indexed.sort_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, output)| output).collect()
}

/// Fetch source content for all modules, in parallel.
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

/// Parse + canonicalize a single module in its own arena.
fn compile_module(uri: &Url, source: &Result<String, String>) -> CompileOutput {
    let source = match source {
        Ok(s) => s,
        Err(e) => {
            return CompileOutput {
                uri: uri.clone(),
                result: ModuleResult::Failed { message: e.clone() },
                warnings: vec![],
            };
        }
    };

    let bump = Bump::new();
    let src: &str = bump.alloc_str(source);
    let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());

    let (result, warnings) = match parser.module() {
        Err(e) => (
            ModuleResult::Failed {
                message: format!("{:?}", e),
            },
            vec![],
        ),
        Ok(module) => {
            let context = nash_can::Context {
                package: None,
                interfaces: None,
            };

            match nash_can::canonicalize(&bump, context, &module) {
                Ok(can_result) => {
                    let warnings: Vec<String> = can_result
                        .warnings
                        .iter()
                        .map(|w| format!("{:?}", w))
                        .collect();
                    let decl_count = count_decls(can_result.module.decls);
                    (ModuleResult::Success { decl_count }, warnings)
                }
                Err(errors) => (
                    ModuleResult::Failed {
                        message: format!("{:?}", errors),
                    },
                    vec![],
                ),
            }
        }
    };

    CompileOutput {
        uri: uri.clone(),
        result,
        warnings,
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
    async fn test_imports_blocked_until_solver_exists() {
        let mem = InMemorySource::new();

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

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        // Utils compiles standalone; Main's import cannot resolve until
        // interfaces exist, which requires the type solver.
        assert_eq!(result.total, 2);
        assert_eq!(result.success, 1);
        assert_eq!(result.failed, 1);
    }
}
