fn main() {
    println!("cargo:rerun-if-changed=assets/muud-view.ico");
    // Embed the icon and version info into muud-view.exe.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/muud-view.ico")
            .set("ProductName", "muud view")
            .set("FileDescription", "muud view - image viewer")
            .set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
