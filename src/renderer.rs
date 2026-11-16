use adw::prelude::*;

const CAIRO: &str = "cairo";

/// Apply the renderer before creating the GTK application or acquiring any data locks.
pub fn prepare() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::process::CommandExt;

        let configured = std::env::var_os("GSK_RENDERER");
        let forced_glx = std::env::var_os("__GLX_VENDOR_LIBRARY_NAME").is_some();
        let theme_override = cfg!(target_os = "linux") && std::env::var_os("GTK_THEME").is_some();
        if configured.as_deref() != Some(OsStr::new(CAIRO)) || forced_glx || theme_override {
            let mut process = std::process::Command::new(std::env::current_exe()?);
            process
                .args(std::env::args_os().skip(1))
                .env("GSK_RENDERER", CAIRO)
                // libGLX may load the forced vendor before main, so clean up at exec.
                .env_remove("__GLX_VENDOR_LIBRARY_NAME");
            if theme_override {
                process.env_remove("GTK_THEME");
            }
            return Err(process.exec());
        }
    }

    #[cfg(windows)]
    // Windows supports thread-safe environment mutation. GTK has not initialized yet.
    unsafe {
        std::env::set_var("GSK_RENDERER", CAIRO);
    }

    Ok(())
}

pub fn record_actual(window: &adw::ApplicationWindow) {
    if let Some(renderer) = window.renderer() {
        eprintln!("Графический рендерер: {}", renderer.type_().name());
    }
}
