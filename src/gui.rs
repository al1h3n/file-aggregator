use anyhow::Result;
use eframe::egui;
use std::path::PathBuf;

use crate::aggregator::aggregate_files;
use crate::security::validate_path;

pub fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_drag_and_drop(true),
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
}

impl eframe::App for FileAggregatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("File Aggregator");
            ui.add_space(10.0);
            
            // File list area
            ui.label("Files to aggregate:");
            ui.add_space(5.0);
            
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    let mut to_remove = None;
                    
                    for (idx, file) in self.files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(file.display().to_string());
                            if ui.button("❌").clicked() {
                                to_remove = Some(idx);
                            }
                        });
                    }
                    
                    if let Some(idx) = to_remove {
                        self.files.remove(idx);
                    }
                    
                    if self.files.is_empty() {
                        ui.label("No files added yet. Drag and drop files here or click 'Add Files'.");
                    }
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
                }
                
                if ui.button("Copy to Clipboard").clicked() {
                    self.process_and_copy();
                }
                
                if ui.button("Save to File").clicked() {
                    self.process_and_save();
                }
            });
            
            ui.add_space(10.0);
            
            // Status messages
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, format!("❌ {}", error));
            }
            
            if let Some(success) = &self.success_message {
                ui.colored_label(egui::Color32::GREEN, format!("✅ {}", success));
            }
            
            // Handle drag and drop
            ctx.input(|i| {
                if !i.raw.dropped_files.is_empty() {
                    let paths: Vec<PathBuf> = i.raw.dropped_files
                        .iter()
                        .filter_map(|f| f.path.clone())
                        .collect();
                    
                    self.add_files(paths);
                }
            });
        });
    }
}

impl FileAggregatorApp {
    fn add_files(&mut self, paths: Vec<PathBuf>) {
        self.error_message = None;
        self.success_message = None;
        
        let count = paths.len();
        
        for path in paths {
            match validate_path(&path) {
                Ok(validated) => {
                    if !self.files.contains(&validated) {
                        self.files.push(validated);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to add {}: {}", path.display(), e));
                    return;
                }
            }
        }
        
        self.success_message = Some(format!("Added {} file(s)", count));
    }
    
    fn process_and_copy(&mut self) {
        self.error_message = None;
        self.success_message = None;
        
        if self.files.is_empty() {
            self.error_message = Some("No files to process".to_string());
            return;
        }
        
        match aggregate_files(&self.files) {
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
        
        if self.files.is_empty() {
            self.error_message = Some("No files to process".to_string());
            return;
        }
        
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name("aggregated.md")
            .save_file()
        {
            match aggregate_files(&self.files) {
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
                            
                            self.success_message = Some(format!("Saved to {}", path.display()));
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
