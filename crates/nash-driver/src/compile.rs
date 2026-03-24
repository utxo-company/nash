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

/// Holds the output of compiling a single module.
///
/// The `_bump` field keeps the per-module arena alive so that `interface`
/// (which was transmuted from the module bump's lifetime to `'static`)
/// remains valid until the output is consumed by the main thread.
struct CompileOutput {
    uri: Url,
    result: ModuleResult,
    interface: Option<(String, nash_can::Interface<'static>)>,
    warnings: Vec<String>,
    _bump: Bump,
}

/// Compile all modules in the dependency graph.
///
/// Within each level, modules compile in parallel via `std::thread::scope`.
/// Each thread gets its own `Bump`. After the level joins, interfaces are
/// deep-copied into a shared `Bump` that outlives all module arenas.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    let shared_bump = Bump::new();
    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut interfaces: BTreeMap<&str, nash_can::Interface<'_>> = BTreeMap::new();
    let mut all_warnings: Vec<String> = Vec::new();

    for level in graph.levels() {
        let sources = fetch_sources(&db, &level).await;

        // Parallel: each thread gets its own Bump, reads &interfaces
        let level_outputs: Vec<CompileOutput> = std::thread::scope(|s| {
            sources
                .iter()
                .map(|(uri, src)| {
                    let ifaces = &interfaces;
                    s.spawn(move || compile_module_threaded(uri, src, ifaces))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect()
        });

        // Sequential: deep-copy interfaces into shared bump, drop module bumps
        for output in level_outputs {
            if let Some((ref name, ref iface)) = output.interface {
                let copied = nash_can::deep_copy_interface(&shared_bump, iface);
                let name: &str = shared_bump.alloc_str(name);
                interfaces.insert(name, copied);
            }
            all_warnings.extend(output.warnings);
            results.insert(output.uri, output.result);
        }
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

/// Parse + canonicalize a single module in a per-module arena.
///
/// The interface is transmuted to `'static` so it can escape the block
/// where the arena is borrowed. The arena is kept alive as `_bump` in
/// the returned `CompileOutput`.
fn compile_module_threaded(
    uri: &Url,
    source: &Result<String, String>,
    interfaces: &BTreeMap<&str, nash_can::Interface<'_>>,
) -> CompileOutput {
    let source = match source {
        Ok(s) => s,
        Err(e) => {
            return CompileOutput {
                uri: uri.clone(),
                result: ModuleResult::Failed { message: e.clone() },
                interface: None,
                warnings: vec![],
                _bump: Bump::new(),
            };
        }
    };

    let bump = Bump::new();

    // Inner block limits borrow scope of `bump`. After the block, only
    // the transmuted `Interface<'static>` survives, and `bump` is free
    // to move into `CompileOutput._bump`.
    let (result, interface, warnings) = {
        let src: &str = bump.alloc_str(source);
        let mut parser = nash_parse::Parser::new(&bump, src.as_bytes());

        match parser.module() {
            Err(e) => (
                ModuleResult::Failed {
                    message: format!("{:?}", e),
                },
                None,
                vec![],
            ),
            Ok(module) => {
                let context = nash_can::Context {
                    package: None,
                    interfaces: if interfaces.is_empty() {
                        None
                    } else {
                        Some(&*bump.alloc(interfaces.clone()))
                    },
                };

                match nash_can::canonicalize(&bump, context, &module) {
                    Ok(can_result) => {
                        let warnings: Vec<String> = can_result
                            .warnings
                            .iter()
                            .map(|w| format!("{:?}", w))
                            .collect();
                        let interface = nash_can::from_module(&bump, &can_result.module);
                        let module_name = can_result.module.name.name.to_string();
                        let decl_count = count_decls(can_result.module.decls);

                        // SAFETY: `interface` borrows from `bump`. We transmute the
                        // lifetime to `'static` so the value can escape this block.
                        // The bump is kept alive as `_bump` in CompileOutput, so all
                        // pointers inside the interface remain valid until the output
                        // is consumed and dropped.
                        let interface: nash_can::Interface<'static> =
                            unsafe { std::mem::transmute(interface) };

                        (
                            ModuleResult::Success { decl_count },
                            Some((module_name, interface)),
                            warnings,
                        )
                    }
                    Err(errors) => (
                        ModuleResult::Failed {
                            message: format!("{:?}", errors),
                        },
                        None,
                        vec![],
                    ),
                }
            }
        }
    };

    CompileOutput {
        uri: uri.clone(),
        result,
        interface,
        warnings,
        _bump: bump,
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
