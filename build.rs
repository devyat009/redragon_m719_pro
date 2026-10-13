fn main() {
    println!("cargo:rerun-if-changed=assets/logo.png");

    #[cfg(windows)]
    {
        // Generate or ensure assets/icon.ico is produced from assets/logo.png
        if let Ok(img) = image::open("assets/logo.png") {
            let resized = img.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
            if let Ok(mut out) = std::fs::File::create("assets/icon.ico") {
                let _ = resized.write_to(&mut out, image::ImageFormat::Ico);
            }
        }

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Redragon INVADER PRO Suite");
        res.set("FileDescription", "Redragon INVADER PRO (M719RGB-PRO) Suite");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resource: {}", e);
        }
    }
}
