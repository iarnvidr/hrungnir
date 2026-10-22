//! Explicitly confirmed, user-level SSL trust recovery. Never invoked at startup.

#[cfg(target_os = "macos")]
pub async fn install(ca: &reddit_offline::ca::Ca) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("User home directory is unavailable")?;
    let path = reddit_offline::ca::Ca::cert_path(ca.dir());
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Recovery requires the app's regular public CA file".into());
    }
    let pem = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if pem != ca.cert_pem {
        return Err("Public CA file changed; restart before enabling trust recovery".into());
    }
    let plan = reddit_offline::trust_policy::install_plan(
        &pem,
        ca.cert_der(),
        &path,
        std::path::Path::new(&home),
    )?;
    let mut command = tokio::process::Command::new(plan.program);
    command.args(plan.args).kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(120), command.output())
        .await
        .map_err(|_| "User SSL trust recovery timed out")?
        .map_err(|error| format!("Could not start user SSL trust recovery: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "User SSL trust recovery failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
                .trim()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub async fn install(_ca: &reddit_offline::ca::Ca) -> Result<(), String> {
    Err("User SSL trust recovery is only supported on macOS".into())
}
