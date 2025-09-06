use std::process::Command;

fn main() {
    // Get git commit hash
    if let Ok(output) = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output() 
    {
        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("cargo:rustc-env=GIT_COMMIT={}", commit);
        } else {
            println!("cargo:rustc-env=GIT_COMMIT=unknown");
        }
    } else {
        println!("cargo:rustc-env=GIT_COMMIT=unknown");
    }

    // Get git commit timestamp
    if let Ok(output) = Command::new("git")
        .args(&["show", "-s", "--format=%cI", "HEAD"])
        .output()
    {
        if output.status.success() {
            let timestamp = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("cargo:rustc-env=GIT_COMMIT_TIMESTAMP={}", timestamp);
        } else {
            println!("cargo:rustc-env=GIT_COMMIT_TIMESTAMP=unknown");
        }
    } else {
        println!("cargo:rustc-env=GIT_COMMIT_TIMESTAMP=unknown");
    }

    // Re-run the build script if git changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    // Re-run if UI dist changes
    println!("cargo:rerun-if-changed=../../ui/dist");
}