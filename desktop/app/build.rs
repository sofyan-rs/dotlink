fn main() {
    // Windows: the icon of the .exe, which GPUI also loads (resource 1) for the window, the
    // taskbar and Alt-Tab.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/dotlink.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/dotlink.ico", "1");
        res.set("ProductName", "Dotlink");
        res.set("FileDescription", "Dotlink");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
}
