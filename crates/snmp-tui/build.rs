use std::process::Command;

fn main() {
    let pkg_version = env!("CARGO_PKG_VERSION");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o.stdout) } else { None })
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "local".to_string());

    println!(
        "cargo:rustc-env=SNMP_TUI_VERSION={} ({})",
        pkg_version, git_hash
    );
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
