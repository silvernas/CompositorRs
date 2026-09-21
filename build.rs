use std::path::Path;

fn main() {
    // Reuse the original Compositor C pixel-algorithm kernel. These eight files are pure C
    // (stdlib/math only) and process premultiplied RGBA8 buffers, so they compile unchanged.
    let c_dir = Path::new("c");
    let sources = [
        "BrushPixels",
        "HealPixels",
        "LevelsPixels",
        "WandPixels",
        "NoisePixels",
        "LensPixels",
        "ContentFill",
        "AdjustPixels",
    ];

    let mut build = cc::Build::new();
    build.include(c_dir).opt_level(2);
    // MSVC doesn't expose M_PI from <math.h> unless _USE_MATH_DEFINES is set, and
    // treats some source files as the system code page (936) unless told UTF-8.
    if cfg!(target_env = "msvc") {
        build.flag("/D_USE_MATH_DEFINES").flag("/utf-8");
    } else {
        build.flag("-D_USE_MATH_DEFINES");
    }
    for name in sources {
        build.file(c_dir.join(format!("{name}.c")));
    }
    build.compile("compositor_pixels");

    println!("cargo:rerun-if-changed=c");
    println!("cargo:rerun-if-changed=assets/compositor.ico");

    // Windows resources: version info, app metadata and (when present) an icon.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "Compositor 图像编辑器");
        res.set("ProductName", "Compositor");
        res.set("LegalCopyright", "MIT License");
        let icon = Path::new("assets/compositor.ico");
        if icon.exists() {
            res.set_icon(icon.to_str().unwrap());
        }
        if let Err(e) = res.compile() {
            println!("cargo:warning=winres compile failed: {e}");
        }
    }
}
