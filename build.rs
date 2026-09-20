#[cfg(windows)]
fn main() {
    use std::fs::File;
    use std::io::BufWriter;
    
    if std::path::Path::new("icon.png").exists() {
        // Use image crate to load PNG (handles any format correctly)
        let img = image::open("icon.png").unwrap();
        let rgba_img = img.to_rgba8();
        let (width, height) = rgba_img.dimensions();
        
        let icon_image = ico::IconImage::from_rgba_data(
            width,
            height,
            rgba_img.into_raw(),
        );
        
        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
        icon_dir.add_entry(ico::IconDirEntry::encode(&icon_image).unwrap());
        
        let ico_file = File::create("icon.ico").unwrap();
        icon_dir.write(BufWriter::new(ico_file)).unwrap();
        
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.compile().unwrap();
    }
}

#[cfg(not(windows))]
fn main() {
    // No-op on non-Windows platforms
}
