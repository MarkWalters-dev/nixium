/// Query the crates.io API for information about a Rust crate.
pub async fn call(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let crate_name = match args.get("crate_name").and_then(|v| v.as_str()) {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            return super::McpCallResponse {
                content: "Missing required argument: crate_name".to_string(),
                is_error: true,
            }
        }
    };

    let url = format!("https://crates.io/api/v1/crates/{}", crate_name);

    let result = client
        .get(&url)
        .header("User-Agent", "nixium-ide/1.0")
        .send()
        .await;

    match result {
        Err(e) => super::McpCallResponse {
            content: format!("Failed to reach crates.io API: {e}"),
            is_error: true,
        },
        Ok(resp) => {
            if resp.status().as_u16() == 404 {
                return super::McpCallResponse {
                    content: format!("Crate `{crate_name}` not found on crates.io."),
                    is_error: true,
                };
            }
            if !resp.status().is_success() {
                return super::McpCallResponse {
                    content: format!("crates.io returned HTTP {}", resp.status()),
                    is_error: true,
                };
            }
            match resp.json::<serde_json::Value>().await {
                Err(e) => super::McpCallResponse {
                    content: format!("Failed to parse crates.io response: {e}"),
                    is_error: true,
                },
                Ok(json) => {
                    let krate = &json["crate"];
                    let name = krate["name"].as_str().unwrap_or(&crate_name);
                    let version = krate["newest_version"].as_str().unwrap_or("unknown");
                    let description = krate["description"].as_str().unwrap_or("No description.");
                    let downloads = krate["downloads"].as_u64().unwrap_or(0);
                    let homepage = krate["homepage"].as_str().unwrap_or("");
                    let repository = krate["repository"].as_str().unwrap_or("");
                    let documentation = krate["documentation"].as_str().unwrap_or("");

                    let doc_link = if !documentation.is_empty() {
                        documentation.to_string()
                    } else {
                        format!("https://docs.rs/{name}")
                    };

                    let mut output = format!(
                        "## {name} v{version}\n\n\
                         {description}\n\n\
                         Downloads : {downloads}\n\
                         Docs      : {doc_link}\n",
                    );

                    if !repository.is_empty() {
                        output.push_str(&format!("Repository: {repository}\n"));
                    }
                    if !homepage.is_empty() {
                        output.push_str(&format!("Homepage  : {homepage}\n"));
                    }

                    output.push_str(&format!(
                        "\nAdd to Cargo.toml:\n```toml\n{name} = \"{version}\"\n```"
                    ));

                    super::McpCallResponse {
                        content: output,
                        is_error: false,
                    }
                }
            }
        }
    }
}

pub const README: &str = r#"
# Rust Crate Lookup (crates.io)

Fetches metadata for a Rust crate from [crates.io](https://crates.io/).

## What it returns

| Field | Description |
|---|---|
| Name & Version | Latest published version |
| Description | Short crate description |
| Downloads | Total all-time download count |
| Docs | Link to docs.rs documentation |
| Repository | Source repository URL (if provided) |
| Cargo.toml snippet | Ready-to-paste dependency line |

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `crate_name` | `string` | ✅ | The exact crate name to look up |

## Example usage

Ask the AI:
> *"What is the latest version of tokio?"*
> *"Tell me about the serde crate."*
> *"How do I add axum to my project?"*

The AI will call this tool and include the crate details in its reply.
"#;
