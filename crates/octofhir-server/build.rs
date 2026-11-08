use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../ui/dist");
    println!("cargo:rerun-if-env-changed=OCTOFHIR_EMBED_GIT_INFO");

    // CI and local edit loops can omit provenance to avoid rebuilding the
    // server solely because HEAD changed. Normal and release builds retain it.
    if std::env::var("OCTOFHIR_EMBED_GIT_INFO").is_ok_and(|value| value == "0") {
        println!("cargo:rustc-env=GIT_COMMIT=unknown");
        println!("cargo:rustc-env=GIT_COMMIT_TIMESTAMP=unknown");
        return;
    }

    println!("cargo:rerun-if-env-changed=OCTOFHIR_BUILD_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=OCTOFHIR_BUILD_GIT_TIMESTAMP");
    // Container build contexts omit .git; release automation supplies the exact source identity.
    if let Ok(commit) = std::env::var("OCTOFHIR_BUILD_GIT_COMMIT")
        && commit.len() == 40
        && commit.bytes().all(|c| c.is_ascii_hexdigit())
    {
        println!("cargo:rustc-env=GIT_COMMIT={commit}");
        let timestamp =
            std::env::var("OCTOFHIR_BUILD_GIT_TIMESTAMP").unwrap_or_else(|_| "unknown".into());
        assert!(!timestamp.contains(['\r', '\n']), "Invalid build timestamp");
        println!("cargo:rustc-env=GIT_COMMIT_TIMESTAMP={timestamp}");
        return;
    }

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
}
