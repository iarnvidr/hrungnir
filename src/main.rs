use reddit_offline::{Cache, Result, Thread};
use std::io::Read;
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--cli") {
        let tools = std::env::var_os("HRUNGNIR_MEDIA_TOOLS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("bin")
            });
        return reddit_offline::cli::run(args[1..].to_vec(), tools);
    }
    let usage = "usage: reddit-offline DB import FIXTURE BUDGET_BYTES | list | read ID | position ID ANCHOR | restore | prune BEFORE_UNIX_SECONDS | doctor | diagnostics export FILE.zip";
    if args.len() < 2 {
        return Err(usage.into());
    }
    if let ("diagnostics", [action, output]) = (args[1].as_str(), &args[2..]) {
        if action == "export" {
            let root = std::path::Path::new(&args[0])
                .parent()
                .unwrap_or(std::path::Path::new("."));
            println!(
                "{}",
                reddit_offline::diagnostics::export(root, std::path::Path::new(output), None)?
            );
            return Ok(());
        }
    }
    let mut cache = Cache::open(&args[0])?;
    match (args[1].as_str(), &args[2..]) {
        ("import", [file, budget]) => {
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err("fixture exceeds 4 MiB import limit".into());
            }
            let thread: Thread = serde_json::from_slice(&bytes)?;
            cache.import(&thread, budget.parse()?)?;
            println!(
                "{}",
                serde_json::json!({"imported":thread.id,"source":"synthetic fixture"})
            );
        }
        ("list", []) => println!("{}", serde_json::to_string(&cache.list()?)?),
        ("read", [id]) => println!(
            "{}",
            serde_json::to_string_pretty(
                &cache
                    .read(id)?
                    .ok_or("thread not cached; network fallback disabled")?
            )?
        ),
        ("position", [id, anchor]) => cache.save_position(id, anchor)?,
        ("restore", []) => println!("{}", serde_json::to_string(&cache.position()?)?),
        ("prune", [before]) => println!(
            "{}",
            serde_json::json!({"deleted":cache.prune(before.parse()?)?})
        ),
        ("doctor", []) => println!("{}", cache.diagnostics()?),
        _ => return Err(usage.into()),
    }
    Ok(())
}
