fn main() {
    println!("cargo:rerun-if-changed=packaging/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("school-day-resource.o");
    let compiler =
        std::env::var_os("WINDRES").unwrap_or_else(|| "x86_64-w64-mingw32-windres".into());
    let status = std::process::Command::new(compiler)
        .args([
            "--codepage=65001",
            "--input=packaging/windows/school-day.rc",
        ])
        .arg("--output")
        .arg(&output)
        .arg("--output-format=coff")
        .status()
        .expect("Для значка Windows нужен windres из MinGW");
    assert!(status.success(), "Не удалось собрать ресурсы Windows");
    println!("cargo:rustc-link-arg={}", output.display());
}
