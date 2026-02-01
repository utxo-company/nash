//! Nash compiler CLI.

use std::path::PathBuf;
use std::sync::Arc;

use miette::{IntoDiagnostic, Result};
use nash_driver::{Database, FileSystemSource, Project, build, build_graph};
use tokio::sync::Mutex;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("check") => {
            let path = args.get(2).map(PathBuf::from).unwrap_or_else(|| ".".into());
            run_check(path)
        }
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some("version") | Some("--version") | Some("-v") => {
            print_version();
            Ok(())
        }
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
            eprintln!();
            print_help();
            std::process::exit(1);
        }
        None => {
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    eprintln!("nashc - Nash compiler");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    nashc <COMMAND> [OPTIONS]");
    eprintln!();
    eprintln!("COMMANDS:");
    eprintln!("    check [PATH]    Check a Nash project for errors");
    eprintln!("    help            Show this help message");
    eprintln!("    version         Show version information");
}

fn print_version() {
    eprintln!("nashc {}", env!("CARGO_PKG_VERSION"));
}

#[tokio::main]
async fn run_check_async(path: PathBuf) -> Result<()> {
    // Load project
    eprintln!("Loading project from {:?}...", path);
    let project = Project::load(&path).await.into_diagnostic()?;

    eprintln!("Project root: {:?}", project.root);
    eprintln!("Members: {}", project.members.len());

    // Create database with filesystem source
    let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));

    // Discover modules
    eprintln!("Discovering modules...");
    let modules = project
        .discover_modules(&*db.lock().await)
        .await
        .into_diagnostic()?;

    eprintln!("Found {} modules", modules.len());

    if modules.is_empty() {
        eprintln!("No Nash source files found.");
        return Ok(());
    }

    // Build dependency graph
    eprintln!("Building dependency graph...");
    let graph = build_graph(db.clone(), &modules).await.into_diagnostic()?;

    eprintln!("Dependency order: {} modules", graph.order.len());

    // Compile
    eprintln!("Compiling...");
    let result = build(db, &graph).await;

    // Report results
    eprintln!();
    if result.is_success() {
        eprintln!(
            "Success! Compiled {} modules ({} declarations)",
            result.total,
            result
                .modules
                .values()
                .filter_map(|r| match r {
                    nash_driver::ModuleResult::Success { decl_count } => Some(decl_count),
                    _ => None,
                })
                .sum::<usize>()
        );
        Ok(())
    } else {
        eprintln!("Compilation failed.");
        eprintln!("  {} succeeded", result.success);
        eprintln!("  {} failed", result.failed);

        // Show failures
        for (uri, module_result) in &result.modules {
            if let nash_driver::ModuleResult::Failed { message } = module_result {
                eprintln!();
                eprintln!("Error in {}:", uri.path());
                eprintln!("  {}", message);
            }
        }

        std::process::exit(1);
    }
}

fn run_check(path: PathBuf) -> Result<()> {
    run_check_async(path)
}
