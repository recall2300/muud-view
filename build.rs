// File types and their icon resource ids (shared with the app).
include!("src/platform/file_types.rs");

fn main() {
    println!("cargo:rerun-if-changed=assets/muud-view.ico");
    println!("cargo:rerun-if-changed=src/platform/file_types.rs");
    for t in FILE_TYPES {
        println!("cargo:rerun-if-changed=assets/icons/{}.ico", t.icon);
    }
    // Embed the app icon, one icon per file type, and version info.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/muud-view.ico")
            .set("ProductName", "muud view")
            .set("FileDescription", "muud view - image viewer")
            .set("LegalCopyright", "MIT License");
        for t in FILE_TYPES {
            res.set_icon_with_id(
                &format!("assets/icons/{}.ico", t.icon),
                &t.icon_id.to_string(),
            );
        }
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
