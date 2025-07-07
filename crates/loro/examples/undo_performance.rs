//! Demonstrates performance issues with undo system for large content operations
//! 
//! Run with: cargo run --example undo_performance --release

use loro::{LoroDoc, UndoManager};
use std::time::Instant;

fn benchmark_delete_operation(content_size: usize, with_undo: bool) -> std::time::Duration {
    let doc = LoroDoc::new();
    let _undo = if with_undo {
        Some(UndoManager::new(&doc))
    } else {
        None
    };
    
    let text = doc.get_text("text");
    let content = "x".repeat(content_size);
    
    // Insert content
    text.insert(0, &content).unwrap();
    doc.commit();
    
    // Time the delete operation
    let start = Instant::now();
    text.delete(0, content.len()).unwrap();
    doc.commit();
    
    start.elapsed()
}

fn main() {
    println!("Undo System Performance Impact on Large Content Operations");
    println!("=========================================================\n");
    
    println!("This example demonstrates that the undo system has significant");
    println!("performance overhead for large content operations because it");
    println!("eagerly copies deleted content into memory.\n");
    
    let sizes = vec![
        (100, "100B"),
        (500, "500B"),
        (1_000, "1KB"),
        (5_000, "5KB"),
        (10_000, "10KB"),
    ];
    
    println!("{:<10} | {:>15} | {:>15} | {:>10} | {:>8}", 
             "Size", "Without Undo", "With Undo", "Overhead", "Slowdown");
    println!("{:-<10}-+-{:-<15}-+-{:-<15}-+-{:-<10}-+-{:-<8}", "", "", "", "", "");
    
    for (size, label) in sizes {
        // Run multiple times and take average
        let runs = 5;
        let mut time_without = std::time::Duration::ZERO;
        let mut time_with = std::time::Duration::ZERO;
        
        for _ in 0..runs {
            time_without += benchmark_delete_operation(size, false);
            time_with += benchmark_delete_operation(size, true);
        }
        
        time_without /= runs;
        time_with /= runs;
        
        let overhead = time_with.saturating_sub(time_without);
        let slowdown = time_with.as_secs_f64() / time_without.as_secs_f64();
        
        println!("{:<10} | {:>15.2?} | {:>15.2?} | {:>10.2?} | {:>7.1}x",
                 label, time_without, time_with, overhead, slowdown);
    }
    
    println!("\nKey Observation:");
    println!("The performance overhead is most significant for small-to-medium content sizes.");
    println!("This is because the undo system copies all deleted content immediately,");
    println!("even if undo is never used. The overhead is especially noticeable for");
    println!("operations in the 100B-1KB range, which are common in text editing.");
}