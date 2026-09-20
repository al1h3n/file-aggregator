use anyhow::Result;
use eframe::egui;
use std::path::PathBuf;

use crate::aggregator::{aggregate_files_with_prompt};
use crate::security::{validate_path, is_executable_file, strip_windows_prefix};
use image;

/// Truncate path to max_len, keeping start and end with "..." in middle
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    
    // Find last separator position
    let separators = ['\\', '/'];
    let last_sep = path.rfind(|c| separators.contains(&c));
    
    if let Some(sep_pos) = last_sep {
        let filename = &path[sep_pos..];
        let prefix_len = max_len.saturating_sub(filename.len() + 5); // 5 for "\...\"
        
        if prefix_len > 10 {
            // Find first separator after prefix
            let prefix = &path[..prefix_len];
            if let Some(first_sep) = prefix.rfind(|c| separators.contains(&c)) {
                return format!("{}...{}", &path[..first_sep + 1], filename);
            }
        }
    }
    
    // Fallback: simple truncation
    format!("{}...{}", &path[..max_len / 2], &path[path.len() - max_len / 2..])
}

pub fn run_gui() -> Result<()> {
    let icon_data = include_bytes!("../icon.png");
    let icon_image = image::load_from_memory(icon_data)
        .unwrap_or_else(|_| image::DynamicImage::new_rgba8(128, 128))
        .to_rgba8();
    let (width, height) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width,
        height,
    };
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        ..Default::default()
    };
    
    eframe::run_native(
        "File Aggregator",
        options,
        Box::new(|_cc| Ok(Box::new(FileAggregatorApp::default()))),
    ).map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

#[derive(Default)]
struct FileAggregatorApp {
    files: Vec<PathBuf>,
    error_message: Option<String>,
    success_message: Option<String>,
    custom_prompt: String,
    executable_warning: Option<(PathBuf, String)>, // (path, file_type)
    pending_file: Option<PathBuf>,
    message_timer: Option<std::time::Instant>,
}

impl eframe::App for FileAggregatorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("File Aggregator");
            ui.add_space(10.0);
            
            // File list area
            ui.label("Files to aggregate:");
            ui.add_space(5.0);
            
            egui::ScrollArea::vertical()
                .id_salt("file_list")
                .max_height(300.0)
                .show(ui, |ui| {
                    let mut to_remove = None;
                    
                    // Grid layout: 3 columns (icon, path, remove button)
                    egui::Grid::new("file_grid")
                        .num_columns(3)
                        .spacing([10.0, 8.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for (idx, file) in self.files.iter().enumerate() {
                                // File icon based on extension
                                let ext = file.extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                let icon = match ext.as_str() {
                                    "rs" => "RS",
                                    "py" => "PY",
                                    "js" | "ts" => "JS",
                                    "md" => "MD",
                                    "txt" => "TXT",
                                    "json" | "yaml" | "toml" | "xml" => "CFG",
                                    "exe" | "dll" | "bin" | "elf" => "EXE",
                                    "sh" | "bat" | "ps1" | "cmd" => "SH",
                                    "zip" | "tar" | "gz" | "7z" | "rar" => "ZIP",
                                    "jpg" | "png" | "gif" | "svg" | "webp" => "IMG",
                                    "pdf" => "PDF",
                                    "doc" | "docx" => "DOC",
                                    "xls" | "xlsx" => "XLS",
                                    "ppt" | "pptx" => "PPT",
                                    _ => "FILE",
                                };
                                
                                ui.label(icon);
                                
                                // Truncated path: "C:\Users\...\file.txt"
                                let full_path = strip_windows_prefix(&file.display().to_string());
                                let truncated = truncate_path(&full_path, 60);
                                ui.label(truncated);
                                
                                if ui.button("❌").clicked() {
                                    to_remove = Some(idx);
                                }
                                
                                ui.end_row();
                            }
                        });
                    
                    if let Some(idx) = to_remove {
                        self.files.remove(idx);
                    }
                    
                    if self.files.is_empty() {
                        ui.label("No files added yet. Drag and drop files here or click 'Add Files'.");
                    }
                });
            
            ui.add_space(10.0);
            
            // Custom prompt input
            ui.label("Custom Instructions (Optional):");
            ui.add_space(5.0);
            
            egui::ScrollArea::vertical()
                .id_salt("custom_prompt")
                .max_height(150.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.custom_prompt)
                            .hint_text("Example: \"Focus on error handling\" or \"Explain async patterns\"")
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                    );
                });
            
            ui.add_space(10.0);
            
            // Buttons
            ui.horizontal(|ui| {
                if ui.button("Add Files").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        self.add_files(paths);
                    }
                }
                
                if ui.button("Clear All").clicked() {
                    self.files.clear();
                    self.error_message = None;
                    self.success_message = None;
                    self.message_timer = None;
                }
                
                if ui.button("Copy to Clipboard").clicked() {
                    self.process_and_copy();
                }
                
                if ui.button("Save to File").clicked() {
                    self.process_and_save();
                }
            });
            
            ui.add_space(10.0);
            
            // Status messages with auto-dismiss
            if let Some(timer) = self.message_timer {
                if timer.elapsed().as_secs() > 10 {
                    self.success_message = None;
                    self.message_timer = None;
                }
            }
            
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, format!("❌ {}", error));
            }
            
            if let Some(success) = &self.success_message {
                ui.colored_label(egui::Color32::GREEN, format!("✅ {}", success));
                
                // Request repaint for timer update
                ui.ctx().request_repaint();
            }
            
            // Executable warning dialog
            if let Some((path, file_type)) = &self.executable_warning.clone() {
                egui::Window::new("Security Warning")
                    .collapsible(false)
                    .resizable(false)
                    .show(ui.ctx(), |ui| {
                        ui.heading("Executable File Detected");
                        ui.add_space(10.0);
                        
                        ui.label("Executable files may contain harmful code.");
                        ui.label("This tool will include the file content in output.");
                        ui.add_space(5.0);
                        
                        ui.label(format!("File: {}", strip_windows_prefix(&path.display().to_string())));
                        ui.label(format!("Type: {}", file_type));
                        ui.add_space(10.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.executable_warning = None;
                                self.pending_file = None;
                            }
                            
                            if ui.button("I understand the risk, add anyway").clicked() {
                                if let Some(pending) = self.pending_file.take() {
                                    self.files.push(pending);
                                    self.success_message = Some("Added 1 executable file".to_string());
                                    self.message_timer = Some(std::time::Instant::now());
                                }
                                self.executable_warning = None;
                            }
                        });
                    });
            }
            
            // Handle drag and drop
            ui.input(|i| {
                if !i.raw.dropped_files.is_empty() {
                    // Catch panics during path extraction
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let paths: Vec<PathBuf> = i.raw.dropped_files
                            .iter()
                            .map(|f| f.path().to_path_buf())
                            .collect();
                        paths
                    }));
                    
                    match result {
                        Ok(paths) => {
                            if !paths.is_empty() {
                                self.add_files(paths);
                            }
                        }
                        Err(_) => {
                            self.error_message = Some("Panic during drag-and-drop file extraction".to_string());
                        }
                    }
                }
            });
        });
    }
}

impl FileAggregatorApp {
    fn add_files(&mut self, paths: Vec<PathBuf>) {
        self.error_message = None;
        self.success_message = None;
        self.message_timer = None;
        
        let mut added = 0;
        
        for path in paths {
            // Catch panics during file processing
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match validate_path(&path) {
                    Ok(validated) => {
                        if self.files.contains(&validated) {
                            return Ok(None);
                        }
                        
                        // Check if executable
                        match is_executable_file(&validated) {
                            Ok((true, file_type)) => {
                                Ok(Some((validated, file_type, true)))
                            }
                            Ok((false, _)) => {
                                Ok(Some((validated, String::new(), false)))
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }
            }));
            
            match result {
                Ok(Ok(Some((validated, file_type, is_exec)))) => {
                    if is_exec {
                        self.executable_warning = Some((validated.clone(), file_type));
                        self.pending_file = Some(validated);
                        return;
                    } else {
                        self.files.push(validated);
                        added += 1;
                    }
                }
                Ok(Ok(None)) => {
                    // Duplicate file, skip
                }
                Ok(Err(e)) => {
                    self.error_message = Some(format!("Failed to add {}: {}", 
                        strip_windows_prefix(&path.display().to_string()), e));
                    return;
                }
                Err(_) => {
                    self.error_message = Some(format!("Panic while processing {}", 
                        strip_windows_prefix(&path.display().to_string())));
                    return;
                }
            }
        }
        
        if added > 0 {
            self.success_message = Some(format!("Added {} file(s)", added));
            self.message_timer = Some(std::time::Instant::now());
        }
    }
    
    fn process_and_copy(&mut self) {
        self.error_message = None;
        self.success_message = None;
        self.message_timer = None;
        
        if self.files.is_empty() {
            self.error_message = Some("No files to process".to_string());
            return;
        }
        
        let prompt = if self.custom_prompt.trim().is_empty() {
            None
        } else {
            Some(self.custom_prompt.as_str())
        };
        
        match aggregate_files_with_prompt(&self.files, prompt) {
            Ok(content) => {
                // Check size limit
                const MAX_CLIPBOARD_SIZE: usize = 10 * 1024 * 1024;
                if content.len() > MAX_CLIPBOARD_SIZE {
                    self.error_message = Some(format!(
                        "Content too large for clipboard ({} MB). Save to file instead.",
                        content.len() / 1024 / 1024
                    ));
                    return;
                }
                
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        match clipboard.set_text(&crate::security::strip_control_chars(&content)) {
                            Ok(_) => {
                                self.success_message = Some(format!(
                                    "Copied to clipboard ({} bytes)",
                                    content.len()
                                ));
                                self.message_timer = Some(std::time::Instant::now());
                            }
                            Err(e) => {
                                self.error_message = Some(format!("Failed to copy: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to access clipboard: {}", e));
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to aggregate files: {}", e));
            }
        }
    }
    
    fn process_and_save(&mut self) {
        self.error_message = None;
        self.success_message = None;
        self.message_timer = None;
        
        if self.files.is_empty() {
            self.error_message = Some("No files to process".to_string());
            return;
        }
        
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name("aggregated.md")
            .save_file()
        {
            let prompt = if self.custom_prompt.trim().is_empty() {
                None
            } else {
                Some(self.custom_prompt.as_str())
            };
            
            match aggregate_files_with_prompt(&self.files, prompt) {
                Ok(content) => {
                    match std::fs::write(&path, crate::security::strip_control_chars(&content)) {
                        Ok(_) => {
                            // Set secure permissions on Unix
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                if let Ok(metadata) = std::fs::metadata(&path) {
                                    let mut perms = metadata.permissions();
                                    perms.set_mode(0o600);
                                    let _ = std::fs::set_permissions(&path, perms);
                                }
                            }
                            
                            self.success_message = Some(format!("Saved to {}", strip_windows_prefix(&path.display().to_string())));
                            self.message_timer = Some(std::time::Instant::now());
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Failed to save: {}", e));
                        }
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to aggregate files: {}", e));
                }
            }
        }
    }
}
