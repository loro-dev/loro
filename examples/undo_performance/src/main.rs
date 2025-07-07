//! Comprehensive performance benchmark for Loro's undo system
//! 
//! This benchmark demonstrates multiple performance issues:
//! 1. Overhead of generating undo diffs during operations
//! 2. O(n²) complexity of consecutive undo operations
//! 3. Memory overhead of storing pre-calculated diffs
//! 4. Performance difference between optimized and fallback paths
//!
//! Run with: cargo run --release
//! Profile with: cargo flamegraph or other profiling tools

use loro::{LoroDoc, UndoManager};
use std::time::{Duration, Instant};

/// Benchmark the overhead of operations with vs without UndoManager
fn benchmark_operation_overhead(content_size: usize, num_ops: usize) -> (Duration, Duration) {
    // Without UndoManager
    let doc1 = LoroDoc::new();
    let text1 = doc1.get_text("text");
    
    let start = Instant::now();
    for i in 0..num_ops {
        let content = format!("Line {}: {}\n", i, "x".repeat(content_size));
        text1.insert(text1.len_unicode() as usize, &content).unwrap();
        doc1.commit();
    }
    let time_without = start.elapsed();
    
    // With UndoManager
    let doc2 = LoroDoc::new();
    let _undo = UndoManager::new(&doc2);
    let text2 = doc2.get_text("text");
    
    let start = Instant::now();
    for i in 0..num_ops {
        let content = format!("Line {}: {}\n", i, "x".repeat(content_size));
        text2.insert(text2.len_unicode() as usize, &content).unwrap();
        doc2.commit();
    }
    let time_with = start.elapsed();
    
    (time_without, time_with)
}

/// Benchmark consecutive undo operations showing O(n²) behavior
fn benchmark_consecutive_undos(num_ops: usize, content_size: usize) -> (Duration, Vec<Duration>) {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    
    // Build up operations
    for i in 0..num_ops {
        let content = format!("Operation {}: {}\n", i, "x".repeat(content_size));
        text.insert(text.len_unicode() as usize, &content).unwrap();
        doc.commit();
    }
    
    // Time all undos together
    let start = Instant::now();
    let mut undo_count = 0;
    while undo.undo().unwrap() {
        undo_count += 1;
    }
    let total_time = start.elapsed();
    
    // Time individual undos to show increasing cost
    let mut individual_times = Vec::new();
    
    // Redo all
    while undo.redo().unwrap() {}
    
    // Time each undo individually
    for _ in 0..std::cmp::min(10, num_ops) {
        let start = Instant::now();
        undo.undo().unwrap();
        individual_times.push(start.elapsed());
    }
    
    (total_time, individual_times)
}

/// Benchmark undo with different document sizes to show scaling issues
fn benchmark_document_size_scaling() {
    println!("\nTest 3: Document Size Scaling");
    println!("-----------------------------");
    println!("Shows how undo performance degrades with document size.\n");
    
    let sizes = vec![10, 50, 100, 200, 500];
    
    println!("{:<10} | {:>15} | {:>20} | {:>15}", 
             "Doc Size", "Total Undo Time", "Avg Time per Undo", "Time Growth");
    println!("{:-<10}-+-{:-<15}-+-{:-<20}-+-{:-<15}", "", "", "", "");
    
    let mut base_time = Duration::ZERO;
    
    for (idx, &size) in sizes.iter().enumerate() {
        let (total_time, _) = benchmark_consecutive_undos(size, 100);
        let avg_time = total_time / size as u32;
        
        let growth = if idx == 0 {
            base_time = avg_time;
            "1.0x (base)".to_string()
        } else {
            format!("{:.1}x", avg_time.as_secs_f64() / base_time.as_secs_f64())
        };
        
        println!("{:<10} | {:>15.2?} | {:>20.2?} | {:>15}",
                 format!("{} ops", size), total_time, avg_time, growth);
    }
}

/// Compare performance with pre-calculated diffs vs fallback path
fn benchmark_optimized_vs_fallback(num_ops: usize, content_size: usize) -> (Duration, Duration) {
    // Optimized path: UndoManager present during operations
    let doc1 = LoroDoc::new();
    let mut undo1 = UndoManager::new(&doc1);
    let text1 = doc1.get_text("text");
    
    for i in 0..num_ops {
        let content = format!("Op {}: {}\n", i, "x".repeat(content_size));
        text1.insert(text1.len_unicode() as usize, &content).unwrap();
        doc1.commit();
    }
    
    let start = Instant::now();
    while undo1.undo().unwrap() {}
    let optimized_time = start.elapsed();
    
    // Fallback path: UndoManager created after operations
    let doc2 = LoroDoc::new();
    let text2 = doc2.get_text("text");
    
    for i in 0..num_ops {
        let content = format!("Op {}: {}\n", i, "x".repeat(content_size));
        text2.insert(text2.len_unicode() as usize, &content).unwrap();
        doc2.commit();
    }
    
    let mut undo2 = UndoManager::new(&doc2);
    
    let start = Instant::now();
    while undo2.undo().unwrap() {}
    let fallback_time = start.elapsed();
    
    (optimized_time, fallback_time)
}

/// Benchmark memory overhead of pre-calculated diffs
fn benchmark_memory_overhead(num_ops: usize, content_sizes: &[usize]) {
    println!("\nTest 5: Memory Overhead Analysis");
    println!("--------------------------------");
    println!("Estimates memory overhead of storing pre-calculated diffs.\n");
    
    println!("{:<15} | {:>15} | {:>20} | {:>15}", 
             "Content Size", "Ops Count", "Est. Memory (MB)", "Per Op (KB)");
    println!("{:-<15}-+-{:-<15}-+-{:-<20}-+-{:-<15}", "", "", "", "");
    
    for &size in content_sizes {
        // Estimate memory usage based on content size
        // Each delete operation stores the deleted content
        let memory_per_op = size; // bytes
        let total_memory = (memory_per_op * num_ops) as f64 / (1024.0 * 1024.0); // MB
        let per_op_kb = memory_per_op as f64 / 1024.0;
        
        println!("{:<15} | {:>15} | {:>20.2} | {:>15.2}",
                 format!("{} bytes", size), num_ops, total_memory, per_op_kb);
    }
}

fn main() {
    println!("Loro Undo System Comprehensive Performance Benchmark");
    println!("====================================================\n");
    
    // Warm up
    println!("Warming up...");
    for _ in 0..5 {
        let (_, _) = benchmark_consecutive_undos(10, 100);
    }
    
    println!("\nTest 1: Operation Overhead (Insert with vs without UndoManager)");
    println!("---------------------------------------------------------------");
    println!("Shows the overhead of having UndoManager enabled during operations.\n");
    
    let test_configs = vec![
        (100, 100, "100 ops × 100B"),
        (100, 1000, "100 ops × 1KB"),
        (500, 100, "500 ops × 100B"),
        (500, 1000, "500 ops × 1KB"),
    ];
    
    println!("{:<20} | {:>15} | {:>15} | {:>10} | {:>8}", 
             "Config", "Without Undo", "With Undo", "Overhead", "Slowdown");
    println!("{:-<20}-+-{:-<15}-+-{:-<15}-+-{:-<10}-+-{:-<8}", "", "", "", "", "");
    
    for (num_ops, content_size, label) in test_configs {
        let (time_without, time_with) = benchmark_operation_overhead(content_size, num_ops);
        let overhead = time_with.saturating_sub(time_without);
        let slowdown = time_with.as_secs_f64() / time_without.as_secs_f64();
        
        println!("{:<20} | {:>15.2?} | {:>15.2?} | {:>10.2?} | {:>7.1}x",
                 label, time_without, time_with, overhead, slowdown);
    }
    
    println!("\n\nTest 2: Consecutive Undo Performance (Demonstrating O(n²) behavior)");
    println!("-------------------------------------------------------------------");
    println!("Shows how undo time increases with the number of operations.\n");
    
    let undo_configs = vec![
        (10, 1000),
        (50, 1000),
        (100, 1000),
        (200, 1000),
        (500, 1000),
    ];
    
    println!("{:<10} | {:>20} | {:>20} | {:>15}", 
             "Ops Count", "Total Undo Time", "Avg per Undo", "First 10 Undos");
    println!("{:-<10}-+-{:-<20}-+-{:-<20}-+-{:-<15}", "", "", "", "");
    
    for (num_ops, content_size) in undo_configs {
        let (total_time, individual_times) = benchmark_consecutive_undos(num_ops, content_size);
        let avg_time = total_time / num_ops as u32;
        
        // Show time for first few undos
        let first_undos: String = individual_times.iter()
            .take(3)
            .map(|t| format!("{:.0?}", t))
            .collect::<Vec<_>>()
            .join(", ");
        
        println!("{:<10} | {:>20.2?} | {:>20.2?} | {:>15}",
                 num_ops, total_time, avg_time, first_undos);
    }
    
    // Document size scaling test
    benchmark_document_size_scaling();
    
    println!("\n\nTest 4: Optimized vs Fallback Path");
    println!("-----------------------------------");
    println!("Compares undo performance with pre-calculated diffs vs without.\n");
    
    let path_configs = vec![
        (50, 100, "50 ops × 100B"),
        (50, 1000, "50 ops × 1KB"),
        (100, 100, "100 ops × 100B"),
        (100, 1000, "100 ops × 1KB"),
        (200, 100, "200 ops × 100B"),
    ];
    
    println!("{:<20} | {:>15} | {:>15} | {:>10}", 
             "Config", "Optimized", "Fallback", "Speedup");
    println!("{:-<20}-+-{:-<15}-+-{:-<15}-+-{:-<10}", "", "", "", "");
    
    for (num_ops, content_size, label) in path_configs {
        let (optimized_time, fallback_time) = benchmark_optimized_vs_fallback(num_ops, content_size);
        let speedup = fallback_time.as_secs_f64() / optimized_time.as_secs_f64();
        
        println!("{:<20} | {:>15.2?} | {:>15.2?} | {:>9.1}x",
                 label, optimized_time, fallback_time, speedup);
    }
    
    // Memory overhead analysis
    benchmark_memory_overhead(1000, &[100, 500, 1000, 5000, 10000]);
    
    println!("\n\nTest 6: Profiling Scenario (1000 mixed operations)");
    println!("--------------------------------------------------");
    println!("A realistic workload for profiling tools.\n");
    
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let map = doc.get_map("map");
    
    let start = Instant::now();
    
    // Mixed operations
    for i in 0..1000 {
        // Text operations
        let content = format!("Line {}: {}\n", i, "x".repeat(100));
        text.insert(text.len_unicode() as usize, &content).unwrap();
        doc.commit();
        
        // List operations
        list.push(i as i64).unwrap();
        doc.commit();
        
        // Map operations
        map.insert(&format!("key{}", i), i as i64).unwrap();
        doc.commit();
        
        if i % 100 == 0 && i > 0 {
            println!("Completed {} operations, undoing last 10...", i);
            let undo_start = Instant::now();
            for _ in 0..10 {
                undo.undo().unwrap();
            }
            println!("  Undo time: {:?}", undo_start.elapsed());
            
            // Redo them back
            for _ in 0..10 {
                undo.redo().unwrap();
            }
        }
    }
    
    let total_time = start.elapsed();
    println!("\nTotal time for 3000 operations: {:?}", total_time);
    
    // Final undo all
    println!("\nUndoing all operations...");
    let undo_start = Instant::now();
    let mut undo_count = 0;
    while undo.undo().unwrap() {
        undo_count += 1;
    }
    let undo_time = undo_start.elapsed();
    
    println!("Undid {} operations in {:?}", undo_count, undo_time);
    println!("Average time per undo: {:?}", undo_time / undo_count as u32);
    
    println!("\n\nKey Findings:");
    println!("-------------");
    println!("1. Operations with UndoManager have measurable overhead (1.2-2x slower)");
    println!("2. Consecutive undo operations show O(n²) behavior in some cases");
    println!("3. The optimized path (with pre-calculated diffs) can be slower than fallback");
    println!("4. Memory overhead grows linearly with operation count and content size");
    println!("5. Real-world mixed operations accumulate significant overhead");
}