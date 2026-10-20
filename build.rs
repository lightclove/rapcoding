fn main() {
    // libvosk import library for linking on Windows (see lib/vosk/).
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-search=native=lib/vosk");
        println!("cargo:rerun-if-changed=lib/vosk/libvosk.lib");

        // Embed the app icon (taskbar/start menu) as a Windows resource.
        println!("cargo:rerun-if-changed=assets/rapcoding.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/rapcoding.ico");
        res.compile().expect("embed icon resource");
    }
}
