fn main() {
    // libvosk import library for linking on Windows (see lib/vosk/).
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-search=native=lib/vosk");
        println!("cargo:rerun-if-changed=lib/vosk/libvosk.lib");
    }
}
