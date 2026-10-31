//! Live Reddit CLI commands routed through the Tauri/WKWebView event loop.

use crate::browser::Browser;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tauri::AppHandle;

pub enum Command {
    Listing {
        subreddit: String,
        sort: String,
        period: String,
        pages: usize,
    },
    Search {
        query: String,
        subreddit: Option<String>,
        sort: String,
        period: String,
        smart: bool,
    },
}

pub struct Request {
    pub data_dir: Option<PathBuf>,
    pub command: Command,
}

fn exit_code(success: bool) -> i32 {
    if success {
        0
    } else {
        1
    }
}

fn error_output(command: &str, requested: serde_json::Value, error: &str) -> serde_json::Value {
    json!({"ok":false,"command":command,"requested":requested,"error":error})
}

fn take_value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    let value = args
        .get(*index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{option} requires a value"))?;
    *index += 2;
    Ok(value.clone())
}

pub fn parse(args: &[String]) -> Result<Option<Request>, String> {
    let mut remaining = Vec::new();
    let mut data_dir = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--data-dir" {
            if data_dir.is_some() {
                return Err("--data-dir may be specified once".into());
            }
            data_dir = Some(PathBuf::from(take_value(args, &mut index, "--data-dir")?));
        } else {
            remaining.push(args[index].clone());
            index += 1;
        }
    }
    let Some(verb) = remaining.first().map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(verb, "fetch-reddit" | "search-reddit") {
        return Ok(None);
    }
    let mut args = remaining;
    let verb = args.remove(0);
    let positional = args
        .first()
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{verb} requires a subreddit or query"))?;
    let mut sort = if verb == "fetch-reddit" {
        "hot".to_owned()
    } else {
        "relevance".to_owned()
    };
    let mut period = if verb == "fetch-reddit" {
        "day".to_owned()
    } else {
        "all".to_owned()
    };
    let mut subreddit = None;
    let mut smart = true;
    let mut pages = 1usize;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--sort" => sort = take_value(&args, &mut index, "--sort")?,
            "--period" => period = take_value(&args, &mut index, "--period")?,
            "--pages" if verb == "fetch-reddit" => {
                pages = take_value(&args, &mut index, "--pages")?.parse().map_err(|_| "--pages must be between 1 and 20")?;
                if !(1..=20).contains(&pages) { return Err("--pages must be between 1 and 20".into()); }
            }
            "--subreddit" if verb == "search-reddit" => {
                subreddit = Some(take_value(&args, &mut index, "--subreddit")?);
            }
            "--smart" if verb == "search-reddit" => {
                smart = true;
                index += 1;
            }
            "--no-smart" if verb == "search-reddit" => {
                smart = false;
                index += 1;
            }
            option => return Err(format!("Unknown option for {verb}: {option}")),
        }
    }
    if !matches!(
        period.as_str(),
        "hour" | "day" | "week" | "month" | "year" | "all"
    ) {
        return Err("--period must be hour, day, week, month, year, or all".into());
    }
    let command = if verb == "fetch-reddit" {
        if !matches!(sort.as_str(), "hot" | "new" | "top" | "rising") {
            return Err("--sort must be hot, new, top, or rising".into());
        }
        Command::Listing {
            subreddit: positional,
            sort,
            period,
            pages,
        }
    } else {
        if !matches!(sort.as_str(), "relevance" | "new" | "top" | "comments") {
            return Err("--sort must be relevance, new, top, or comments".into());
        }
        Command::Search {
            query: positional,
            subreddit,
            sort,
            period,
            smart,
        }
    };
    Ok(Some(Request { data_dir, command }))
}

pub async fn run(app: AppHandle, browser: Arc<Browser>, request: Request) {
    let (verb, requested, result) = match request.command {
        Command::Listing {
            subreddit,
            sort,
            period,
            pages,
        } => {
            let result = crate::fetch_subreddit_pages_operation(&browser, &subreddit, &sort, &period, pages).await;
            (
                "fetch-reddit",
                json!({"subreddit":subreddit,"sort":sort,"period":period,"pages":pages}),
                result,
            )
        }
        Command::Search {
            query,
            subreddit,
            sort,
            period,
            smart,
        } => {
            let result = crate::search_reddit_operation(
                &browser,
                None,
                &query,
                subreddit.as_deref(),
                &sort,
                &period,
                smart,
            )
            .await;
            (
                "search-reddit",
                json!({"query":query,"subreddit":subreddit,"sort":sort,"period":period,"smart":smart}),
                result,
            )
        }
    };
    match result {
        Ok(snapshot) => {
            let threads = snapshot.threads.len();
            let output = json!({"ok":true,"command":verb,"requested":requested,"thread_count":threads,"result":snapshot});
            println!(
                "{}",
                serde_json::to_string(&output).unwrap_or_else(|_| "{}".into())
            );
            app.exit(exit_code(true));
        }
        Err(error) => {
            println!("{}", error_output(verb, requested, &error));
            app.exit(exit_code(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{error_output, exit_code, parse, Command};
    use serde_json::json;

    #[test]
    fn live_listing_cli_keeps_defaults_and_data_directory() {
        let args = [
            "fetch-reddit".into(),
            "soccer".into(),
            "--data-dir".into(),
            "/tmp/hrungnir-cli".into(),
        ];
        let request = parse(&args).unwrap().unwrap();
        assert_eq!(
            request.data_dir.as_deref().unwrap(),
            std::path::Path::new("/tmp/hrungnir-cli")
        );
        assert!(
            matches!(request.command, Command::Listing { subreddit, sort, period, pages } if subreddit == "soccer" && sort == "hot" && period == "day" && pages == 1)
        );
    }

    #[test]
    fn live_listing_cli_accepts_leading_data_directory_and_options() {
        let args = [
            "--data-dir".into(),
            "/tmp/hrungnir-cli".into(),
            "fetch-reddit".into(),
            "all".into(),
            "--sort".into(),
            "top".into(),
            "--period".into(),
            "week".into(),
        ];
        let request = parse(&args).unwrap().unwrap();
        assert_eq!(
            request.data_dir.as_deref().unwrap(),
            std::path::Path::new("/tmp/hrungnir-cli")
        );
        assert!(
            matches!(request.command, Command::Listing { subreddit, sort, period, pages } if subreddit == "all" && sort == "top" && period == "week" && pages == 1)
        );
    }

    #[test]
    fn live_listing_cli_page_count_is_bounded_for_paging_checks() {
        let args = ["fetch-reddit".into(), "soccer".into(), "--pages".into(), "2".into()];
        let request = parse(&args).unwrap().unwrap();
        assert!(matches!(request.command, Command::Listing { pages: 2, .. }));
        for count in ["0", "21", "many"] {
            let args = ["fetch-reddit".into(), "soccer".into(), "--pages".into(), count.into()];
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn live_search_cli_preserves_query_scope_options_and_smart_choice() {
        let args = [
            "search-reddit".into(),
            "blue team goal".into(),
            "--subreddit".into(),
            "soccer".into(),
            "--sort".into(),
            "new".into(),
            "--period".into(),
            "week".into(),
            "--no-smart".into(),
        ];
        let request = parse(&args).unwrap().unwrap();
        assert!(
            matches!(request.command, Command::Search { query, subreddit, sort, period, smart } if query == "blue team goal" && subreddit.as_deref() == Some("soccer") && sort == "new" && period == "week" && !smart)
        );
    }

    #[test]
    fn local_cli_search_does_not_dispatch_to_live_acquisition() {
        let args = [
            "--data-dir".into(),
            "/tmp/local-cache".into(),
            "search".into(),
            "rust".into(),
        ];
        assert!(parse(&args).unwrap().is_none());
    }

    #[test]
    fn live_cli_rejects_invalid_inputs_and_emits_structured_errors() {
        let missing = ["fetch-reddit".into()];
        assert!(parse(&missing).is_err());
        let duplicate = [
            "fetch-reddit".into(),
            "soccer".into(),
            "--data-dir".into(),
            "/tmp/a".into(),
            "--data-dir".into(),
            "/tmp/b".into(),
        ];
        assert!(parse(&duplicate).is_err());
        let invalid_sort = [
            "fetch-reddit".into(),
            "soccer".into(),
            "--sort".into(),
            "relevance".into(),
        ];
        assert!(parse(&invalid_sort).is_err());
        let output = error_output(
            "fetch-reddit",
            json!({"subreddit":"soccer"}),
            "native acquisition failed",
        );
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"], "native acquisition failed");
        assert_eq!(exit_code(false), 1);
        assert_eq!(exit_code(true), 0);
    }
}
