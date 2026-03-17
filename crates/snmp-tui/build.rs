fn main() {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let build_number = std::env::var("BUILD_NUMBER").unwrap_or_else(|_| "dev".to_string());
    println!(
        "cargo:rustc-env=SNMP_TUI_VERSION={} ({})",
        pkg_version, build_number
    );
    // Re-run if BUILD_NUMBER changes
    println!("cargo:rerun-if-env-changed=BUILD_NUMBER");
}
