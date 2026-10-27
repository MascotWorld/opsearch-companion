use std::{env, fs, path::PathBuf};

fn main() {
    let icon_path = "assets/brand/opsearch-icon-v1.png";
    println!("cargo:rerun-if-changed={icon_path}");

    let image = image::ImageReader::open(icon_path)
        .expect("open OPSearch icon PNG")
        .decode()
        .expect("decode OPSearch icon PNG")
        .resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode Windows icon PNG");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let png_bytes = png.into_inner();
    fs::write(out_dir.join("opsearch-icon-v1.png"), &png_bytes)
        .expect("write resized OPSearch icon PNG");
    let ico_path = out_dir.join("opsearch-icon-v1.ico");
    fs::write(&ico_path, png_ico(&png_bytes)).expect("write Windows icon resource");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon(ico_path.to_str().expect("Windows icon path"))
            .compile()
            .expect("embed OPSearch Windows application icon");
    }
}

// ICO permits an embedded PNG payload for 256×256 and larger icon frames.
fn png_ico(png: &[u8]) -> Vec<u8> {
    let mut ico = Vec::with_capacity(22 + png.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22_u32.to_le_bytes());
    ico.extend_from_slice(png);
    ico
}
