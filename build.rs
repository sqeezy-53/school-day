fn main() {
    println!("cargo:rerun-if-changed=packaging/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let output = out_dir.join("school-day-resource.o");
    let compiler =
        std::env::var_os("WINDRES").unwrap_or_else(|| "x86_64-w64-mingw32-windres".into());
    let version = env!("CARGO_PKG_VERSION");
    let version_parts = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    std::fs::write(
        out_dir.join("school-day-version.h"),
        format!(
            "#define APP_VERSION_TEXT \"{version}\\0\"\n#define APP_VERSION_NUM {version_parts}\n"
        ),
    )
    .expect("Не удалось записать версию для ресурсов Windows");
    let status = std::process::Command::new(compiler)
        .args([
            "--codepage=65001",
            "--input=packaging/windows/school-day.rc",
        ])
        .arg("--include-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg(&output)
        .arg("--output-format=coff")
        .status()
        .expect("Для значка Windows нужен windres из MinGW");
    assert!(status.success(), "Не удалось собрать ресурсы Windows");
    println!("cargo:rustc-link-arg={}", output.display());
}
