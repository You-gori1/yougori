fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("broker.manifest");
        println!("cargo:rustc-link-arg-bin=yougori-vault=/MANIFEST:EMBED");
        // Suppress the linker's default asInvoker fragment; our manifest
        // explicitly requires administrator protection for this binary only.
        println!("cargo:rustc-link-arg-bin=yougori-vault=/MANIFESTUAC:NO");
        println!(
            "cargo:rustc-link-arg-bin=yougori-vault=/MANIFESTINPUT:{}",
            manifest.display()
        );
        println!("cargo:rerun-if-changed=broker.manifest");
    }
}
