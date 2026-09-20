use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::security::{read_file_safe, validate_path};

/// File entry with validated path and content
#[derive(Clone)]
struct FileEntry {
    canonical: PathBuf,
    extension: String,
    content: String,
    size: u64,
    index: usize,
}

/// Aggregate multiple files into prompt format
pub fn aggregate_files(paths: &[PathBuf]) -> Result<String> {
    aggregate_files_with_prompt(paths, None)
}

/// Aggregate multiple files with optional custom prompt
pub fn aggregate_files_with_prompt(paths: &[PathBuf], custom_prompt: Option<&str>) -> Result<String> {
    // Total size limit: 200MB
    const MAX_TOTAL_SIZE: u64 = 200 * 1024 * 1024;
    
    // Phase 1: Parallel validation and reading using stdlib threads
    let entries = if paths.len() > 1 {
        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = vec![];
        
        for (idx, path) in paths.iter().enumerate() {
            let path = path.clone();
            let _results = Arc::clone(&results);
            
            let handle = thread::spawn(move || -> Result<FileEntry> {
                // Validate path (defense in depth)
                let canonical = validate_path(&path)?;
                
                // Check file metadata
                let metadata = std::fs::metadata(&canonical)
                    .context("Failed to read file metadata")?;
                
                let size = metadata.len();
                
                // Get extension
                let extension = canonical
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt")
                    .to_string();
                
                // Read file content safely
                let content = read_file_safe(&canonical)?;
                
                Ok(FileEntry {
                    canonical,
                    extension,
                    content,
                    size,
                    index: idx,
                })
            });
            
            handles.push((idx, handle));
        }
        
        // Collect results
        for (idx, handle) in handles {
            match handle.join() {
                Ok(result) => {
                    let entry = result?;
                    results.lock().unwrap().push(entry);
                }
                Err(_) => anyhow::bail!("Thread panicked processing file at index {}", idx),
            }
        }
        
        let mut entries = Arc::try_unwrap(results)
            .map(|mutex| mutex.into_inner().unwrap())
            .unwrap_or_else(|arc| arc.lock().unwrap().clone());
        
        // Sort by original index to preserve order
        entries.sort_by_key(|e| e.index);
        entries
    } else {
        // Single file, no threading overhead
        paths.iter().enumerate().map(|(idx, path)| {
            let canonical = validate_path(path)?;
            let metadata = std::fs::metadata(&canonical)
                .context("Failed to read file metadata")?;
            let size = metadata.len();
            let extension = canonical
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("txt")
                .to_string();
            let content = read_file_safe(&canonical)?;
            
            Ok(FileEntry {
                canonical,
                extension,
                content,
                size,
                index: idx,
            })
        }).collect::<Result<Vec<_>>>()?
    };
    
    // Phase 2: Sequential aggregation with size check
    let mut total_size: u64 = 0;
    let mut output = String::from("BEGINNING\nRead following file and answer question given:\n");
    
    if let Some(prompt) = custom_prompt {
        if !prompt.trim().is_empty() {
            output.push_str(prompt);
            output.push_str("\n\n");
        }
    } else {
        output.push_str("QUESTION\n\n");
    }
    
    for entry in entries {
        total_size += entry.size;
        if total_size > MAX_TOTAL_SIZE {
            anyhow::bail!(
                "Total size exceeds limit: {} MB (max: 200 MB)",
                total_size / 1024 / 1024
            );
        }
        
        // Format: ```extension path=FULL_PATH
        output.push_str(&format!(
            "```{} path={}\n{}\n```\n\n",
            entry.extension,
            entry.canonical.display(),
            entry.content
        ));
    }
    
    output.push_str("END");
    
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    
    #[test]
    fn test_aggregate_empty() {
        let result = aggregate_files(&[]);
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("BEGINNING"));
        assert!(content.contains("END"));
    }
}
