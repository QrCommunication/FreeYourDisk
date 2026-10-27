use core_services::{GitService, Service};
use std::path::PathBuf;

fn main() {
    let search_root = PathBuf::from("/home/rony/Projets");
    let svc = GitService::new(search_root);
    eprintln!(
        "Scanning {:?} (max_depth={})...",
        svc.search_root, svc.max_depth
    );
    let start = std::time::Instant::now();
    let result = svc.scan();
    let elapsed = start.elapsed();
    println!("Scan completed in {:.2}s", elapsed.as_secs_f64());
    println!(
        "Found {} worktree items, total {} bytes",
        result.items.len(),
        result.total_bytes
    );
    for item in &result.items {
        println!(
            "  {} ({} bytes) {:?}",
            item.path.display(),
            item.size_bytes,
            item.kind
        );
    }
    if result.items.is_empty() {
        println!("\nNO WORKTREES FOUND.");
    }
}
