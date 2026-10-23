use std::process::Command;

fn main() {
    // Get git commit hash
    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
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
        .args(["show", "-s", "--format=%cI", "HEAD"])
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

    // Cargo runs this script in the crate directory, which has no .git.
    // Resolve Git's actual paths, including linked worktrees and packed refs.
    println!("cargo:rerun-if-changed=build.rs");
    for name in ["HEAD", "refs", "packed-refs"] {
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--git-path", name])
            .output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout);
            let path = path.trim();
            if std::path::Path::new(path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }

    // Re-run if UI dist changes
    println!("cargo:rerun-if-changed=../../ui/dist");
}
