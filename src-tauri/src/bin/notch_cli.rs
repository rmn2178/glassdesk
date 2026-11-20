use serde_json::json;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  notch-cli notify --agent <name> --status <working|needs_input|done> --title <text> [--body <text>] [--accent <#hex>]");
        eprintln!("  notch-cli tile --id <id> --label <label> --value <val> [--progress <0.0..1.0>]");
        std::process::exit(1);
    }

    let sub = args[1].as_str();
    let get_flag = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let client = reqwest::Client::new();
        match sub {
            "notify" => {
                let payload = json!({
                    "agent": get_flag("--agent").unwrap_or_else(|| "custom-agent".to_string()),
                    "status": get_flag("--status").unwrap_or_else(|| "working".to_string()),
                    "title": get_flag("--title").unwrap_or_else(|| "Agent Update".to_string()),
                    "body": get_flag("--body").unwrap_or_default(),
                    "accent": get_flag("--accent").unwrap_or_else(|| "#ffffff".to_string()),
                });
                match client
                    .post("http://127.0.0.1:47823/notify")
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => println!("Notification sent to NotchGlass."),
                    Ok(r) => eprintln!("NotchGlass returned HTTP {}", r.status()),
                    Err(e) => eprintln!("Could not reach NotchGlass on 127.0.0.1:47823: {}", e),
                }
            }
            "tile" => {
                let progress = get_flag("--progress").and_then(|v| v.parse::<f32>().ok());
                let payload = json!({
                    "id": get_flag("--id").unwrap_or_else(|| "tile-1".to_string()),
                    "label": get_flag("--label").unwrap_or_else(|| "Task".to_string()),
                    "value": get_flag("--value").unwrap_or_else(|| "Running".to_string()),
                    "progress": progress,
                });
                match client
                    .post("http://127.0.0.1:47823/tile")
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => println!("Live tile updated."),
                    Ok(r) => eprintln!("NotchGlass returned HTTP {}", r.status()),
                    Err(e) => eprintln!("Could not reach NotchGlass on 127.0.0.1:47823: {}", e),
                }
            }
            _ => {
                eprintln!("Unknown command '{}'. Use 'notify' or 'tile'.", sub);
                std::process::exit(1);
            }
        }
    });
}
