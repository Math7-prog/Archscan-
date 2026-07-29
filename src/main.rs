use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use ignore::WalkBuilder;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser as TsParser};

#[derive(Parser)]
#[command(name = "archscan")]
#[command(about = "Architectural health linter for JS/TS codebases", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a project directory for architectural issues
    Analyze {
        /// Path to the project root
        path: PathBuf,
        /// Output format: text | json
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Debug, Clone, Serialize)]
struct FileNode {
    path: String,
}

#[derive(Debug, Serialize)]
struct CycleReport {
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AnalysisReport {
    files_scanned: usize,
    edges: usize,
    circular_dependencies: Vec<CycleReport>,
    health_score: u32,
}

fn is_supported_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx")
    )
}

/// Walks the project respecting .gitignore and collects supported source files.
fn collect_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_supported_file(path) {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

/// Extracts the raw import/require specifiers (e.g. "./foo", "../bar") from a source file.
fn extract_imports(source: &str, is_tsx: bool) -> Result<Vec<String>> {
    let mut parser = TsParser::new();
    let language = if is_tsx {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    };
    parser
        .set_language(language)
        .context("failed to load tree-sitter language")?;

    let tree = parser
        .parse(source, None)
        .context("failed to parse source file")?;

    let mut specifiers = Vec::new();
    walk_for_imports(tree.root_node(), source.as_bytes(), &mut specifiers);
    Ok(specifiers)
}

fn walk_for_imports(node: Node, source: &[u8], out: &mut Vec<String>) {
    match node.kind() {
        "import_statement" | "export_statement" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "string" {
                        if let Some(spec) = string_node_content(child, source) {
                            out.push(spec);
                        }
                    }
                }
            }
        }
        "call_expression" => {
            if let Some(func) = node.child(0) {
                let func_text = func.utf8_text(source).unwrap_or("");
                if func_text == "require" || func_text == "import" {
                    if let Some(args) = node.child_by_field_name("arguments") {
                        for i in 0..args.child_count() {
                            if let Some(child) = args.child(i) {
                                if child.kind() == "string" {
                                    if let Some(spec) = string_node_content(child, source) {
                                        out.push(spec);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_for_imports(child, source, out);
        }
    }
}

fn string_node_content(node: Node, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?;
    let trimmed = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    Some(trimmed.to_string())
}

/// Resolves a relative import specifier to an actual file in the project, if possible.
fn resolve_import(from_file: &Path, spec: &str, root: &Path) -> Option<PathBuf> {
    if !(spec.starts_with('.') || spec.starts_with('/')) {
        return None; // external package (node_modules) — not tracked
    }

    let base = if spec.starts_with('/') {
        root.to_path_buf()
    } else {
        from_file.parent()?.to_path_buf()
    };

    let candidate = base.join(spec.trim_start_matches('/'));
    let extensions = ["", ".ts", ".tsx", ".js", ".jsx"];
    let index_names = ["index.ts", "index.tsx", "index.js", "index.jsx"];

    for ext in extensions {
        let p = if ext.is_empty() {
            candidate.clone()
        } else {
            PathBuf::from(format!("{}{}", candidate.display(), ext))
        };
        if p.is_file() {
            return p.canonicalize().ok();
        }
    }

    if candidate.is_dir() {
        for idx in index_names {
            let p = candidate.join(idx);
            if p.is_file() {
                return p.canonicalize().ok();
            }
        }
    }

    None
}

fn run_analyze(path: &Path, format: &str) -> Result<()> {
    let root = path
        .canonicalize()
        .with_context(|| format!("path not found: {}", path.display()))?;

    let files = collect_files(&root)?;
    if files.is_empty() {
        println!(
            "{}",
            "No supported source files (.ts/.tsx/.js/.jsx) found.".yellow()
        );
        return Ok(());
    }

    let mut graph: DiGraph<FileNode, ()> = DiGraph::new();
    let mut index_of: HashMap<PathBuf, NodeIndex> = HashMap::new();

    for f in &files {
        let idx = graph.add_node(FileNode {
            path: rel(&root, f),
        });
        index_of.insert(f.clone(), idx);
    }

    let mut edge_count = 0usize;

    for f in &files {
        let source = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let is_tsx = matches!(
            f.extension().and_then(|e| e.to_str()),
            Some("tsx") | Some("jsx")
        );
        let specs = match extract_imports(&source, is_tsx) {
            Ok(s) => s,
            Err(_) => continue,
        };

        for spec in specs {
            if let Some(resolved) = resolve_import(f, &spec, &root) {
                if let (Some(&from_idx), Some(&to_idx)) =
                    (index_of.get(f), index_of.get(&resolved))
                {
                    if from_idx != to_idx {
                        graph.add_edge(from_idx, to_idx, ());
                        edge_count += 1;
                    }
                }
            }
        }
    }

    let sccs = tarjan_scc(&graph);
    let mut cycles: Vec<CycleReport> = sccs
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| CycleReport {
            files: scc.iter().map(|&i| graph[i].path.clone()).collect(),
        })
        .collect();
    cycles.sort_by(|a, b| b.files.len().cmp(&a.files.len()));

    let health_score = compute_health_score(files.len(), &cycles);

    let report = AnalysisReport {
        files_scanned: files.len(),
        edges: edge_count,
        circular_dependencies: cycles,
        health_score,
    };

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        _ => print_text_report(&report),
    }

    Ok(())
}

fn compute_health_score(files_scanned: usize, cycles: &[CycleReport]) -> u32 {
    if files_scanned == 0 {
        return 100;
    }
    let files_in_cycles: usize = cycles.iter().map(|c| c.files.len()).sum();
    let penalty = ((files_in_cycles as f64 / files_scanned as f64) * 100.0).min(100.0);
    (100.0 - penalty).max(0.0) as u32
}

fn rel(root: &Path, f: &Path) -> String {
    f.strip_prefix(root)
        .unwrap_or(f)
        .to_string_lossy()
        .to_string()
}

fn print_text_report(report: &AnalysisReport) {
    println!(
        "{} {} files scanned, {} import edges",
        "archscan".bold().cyan(),
        report.files_scanned,
        report.edges
    );
    println!();

    if report.circular_dependencies.is_empty() {
        println!("{}", "✓ No circular dependencies found.".green());
    } else {
        println!(
            "{} {}",
            "✗".red().bold(),
            format!(
                "{} circular dependency group(s) found:",
                report.circular_dependencies.len()
            )
            .red()
        );
        for (i, cycle) in report.circular_dependencies.iter().enumerate() {
            println!(
                "\n  {} ({} files):",
                format!("Cycle #{}", i + 1).bold(),
                cycle.files.len()
            );
            for f in &cycle.files {
                println!("    - {}", f);
            }
        }
    }

    println!();
    let score_colored = match report.health_score {
        90..=100 => format!("{}/100", report.health_score).green().bold(),
        70..=89 => format!("{}/100", report.health_score).yellow().bold(),
        _ => format!("{}/100", report.health_score).red().bold(),
    };
    println!("{} {}", "Health Score:".bold(), score_colored);
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Analyze { path, format } => run_analyze(path, format)?,
    }
    Ok(())
}
