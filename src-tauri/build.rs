fn main() {
    println!("cargo:rustc-check-cfg=cfg(yougori_source_runtime)");
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    // Icons are compiled into the executable, including development builds.
    // Rebuild native resources whenever the generated app icons change.
    println!("cargo:rerun-if-changed=icons");
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml"));
        let attributes = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attributes).expect("failed to prepare Yougori desktop build");
    } else {
        // Development runs use the checked-in runtime directly. Copying it over
        // target/debug can fail on Windows when an existing QEMU maps a file.
        // Preserve the CLI's other overrides (especially devUrl and features).
        let mut config: serde_json::Value = std::env::var("TAURI_CONFIG")
            .map(|value| serde_json::from_str(&value).expect("invalid TAURI_CONFIG JSON"))
            .unwrap_or_else(|_| serde_json::json!({}));
        config["bundle"]["resources"] = serde_json::json!([]);
        std::env::set_var("TAURI_CONFIG", config.to_string());
        println!("cargo:rustc-cfg=yougori_source_runtime");
        // The linker embeds the manifest for every development executable,
        // including the library's WebView2 unit-test harness. Do not also put
        // it in Tauri's resource.lib: that duplicates MANIFEST #1 in the app.
        // Tauri still supplies the icon and version resources.
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let attributes = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attributes).expect("failed to prepare Yougori desktop build");
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            // Single manifest owner for both the app and test executables.
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml").display());
        }
    }
}
