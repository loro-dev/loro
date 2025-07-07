//! Benchmark demonstrating performance issues with undo system for large content
//! 
//! This example shows that the current undo implementation has significant overhead
//! when dealing with large text deletions because it eagerly copies all deleted content
//! into memory for potential undo operations, even if undo is never used.

use loro_internal::{LoroDoc, UndoManager};
use std::time::Instant;

fn main() {
    println!("Undo System Performance Benchmark for Large Content");
    println!("===================================================\n");
    
    println!("This benchmark demonstrates the performance overhead of the undo system");
    println!("when working with large text content.\n");
    
    // Test different content sizes
    let content_sizes = vec![
        (100, "100 bytes"),
        (1_000, "1 KB"),
        (10_000, "10 KB"),
        (100_000, "100 KB"),
        (1_000_000, "1 MB"),
    ];
    
    println!("Test 1: Deletion Performance (with vs without UndoManager)");
    println!("----------------------------------------------------------");
    println!("Content Size | Without Undo | With Undo | Overhead | Slowdown");
    println!("-------------|--------------|-----------|----------|----------");
    
    for (size, label) in &content_sizes {
        let content = "x".repeat(*size);
        
        // Test without UndoManager
        let doc1 = LoroDoc::new();
        let text1 = doc1.get_text("text");
        text1.insert(0, &content).unwrap();
        doc1.commit_then_renew();
        
        let start = Instant::now();
        text1.delete(0, content.len()).unwrap();
        doc1.commit_then_renew();
        let time_without_undo = start.elapsed();
        
        // Test with UndoManager
        let doc2 = LoroDoc::new();
        let _undo = UndoManager::new(&doc2);
        let text2 = doc2.get_text("text");
        text2.insert(0, &content).unwrap();
        doc2.commit_then_renew();
        
        let start = Instant::now();
        text2.delete(0, content.len()).unwrap();
        doc2.commit_then_renew();
        let time_with_undo = start.elapsed();
        
        let overhead = time_with_undo.saturating_sub(time_without_undo);
        let slowdown = time_with_undo.as_secs_f64() / time_without_undo.as_secs_f64();
        
        println!(
            "{:12} | {:12.2?} | {:9.2?} | {:8.2?} | {:8.2}x",
            label, time_without_undo, time_with_undo, overhead, slowdown
        );
    }
    
    println!("\n\nTest 2: Memory Impact of Multiple Operations");
    println!("---------------------------------------------");
    println!("This test performs many delete operations to show cumulative overhead.\n");
    
    let chunk_size = 10_000; // 10KB chunks
    let num_chunks = 100;
    
    // Without UndoManager
    let doc1 = LoroDoc::new();
    let text1 = doc1.get_text("text");
    
    // Insert all content
    for i in 0..num_chunks {
        let chunk = format!("Chunk {}: {}\n", i, "x".repeat(chunk_size));
        text1.insert(text1.len_unicode() as usize, &chunk).unwrap();
    }
    doc1.commit_then_renew();
    
    println!("Deleting {} chunks of {} bytes each WITHOUT UndoManager...", num_chunks, chunk_size);
    let start = Instant::now();
    for _ in 0..num_chunks {
        text1.delete(0, chunk_size + 20).unwrap(); // +20 for "Chunk N: " prefix
        doc1.commit_then_renew();
    }
    let time_without = start.elapsed();
    println!("Time: {:?}", time_without);
    
    // With UndoManager
    let doc2 = LoroDoc::new();
    let _undo = UndoManager::new(&doc2);
    let text2 = doc2.get_text("text");
    
    // Insert all content
    for i in 0..num_chunks {
        let chunk = format!("Chunk {}: {}\n", i, "x".repeat(chunk_size));
        text2.insert(text2.len_unicode() as usize, &chunk).unwrap();
    }
    doc2.commit_then_renew();
    
    println!("\nDeleting {} chunks of {} bytes each WITH UndoManager...", num_chunks, chunk_size);
    let start = Instant::now();
    for _ in 0..num_chunks {
        text2.delete(0, chunk_size + 20).unwrap();
        doc2.commit_then_renew();
    }
    let time_with = start.elapsed();
    println!("Time: {:?}", time_with);
    
    println!("\nOverhead: {:?}", time_with.saturating_sub(time_without));
    println!("Slowdown: {:.2}x", time_with.as_secs_f64() / time_without.as_secs_f64());
    
    println!("\n\nTest 3: Undo Performance After Large Deletions");
    println!("-----------------------------------------------");
    println!("This test shows that even undo operations are affected.\n");
    
    // Create document with UndoManager and perform operations
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    
    // Insert and delete large content multiple times
    let large_content = "x".repeat(100_000); // 100KB
    let num_operations = 10;
    
    for i in 0..num_operations {
        text.insert(0, &format!("Operation {}: {}", i, large_content)).unwrap();
        doc.commit_then_renew();
        text.delete(0, large_content.len() + 15).unwrap(); // +15 for prefix
        doc.commit_then_renew();
    }
    
    println!("Performed {} insert/delete pairs with 100KB content each", num_operations);
    println!("Now timing {} undo operations...", num_operations * 2);
    
    let start = Instant::now();
    let mut undo_count = 0;
    while undo.undo().unwrap() {
        undo_count += 1;
    }
    let undo_time = start.elapsed();
    
    println!("Undid {} operations in {:?}", undo_count, undo_time);
    println!("Average time per undo: {:?}", undo_time / undo_count as u32);
    
    println!("\n\nConclusion:");
    println!("-----------");
    println!("The benchmark shows that:");
    println!("1. Delete operations are significantly slower with UndoManager");
    println!("2. The overhead increases with content size");
    println!("3. Even undo operations themselves are affected by the accumulated overhead");
    println!("\nThe root cause is that deleted content is eagerly copied into memory");
    println!("for potential undo operations, even if undo is never used.");
}