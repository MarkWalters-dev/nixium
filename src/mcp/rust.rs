// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, super::McpCallResponse> {
    match args.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s.trim()),
        _ => Err(super::McpCallResponse {
            content: format!("Missing required argument: {key}"),
            is_error: true,
        }),
    }
}

async fn crates_get(
    url: &str,
    client: &reqwest::Client,
) -> Result<serde_json::Value, super::McpCallResponse> {
    let resp = client
        .get(url)
        .header("User-Agent", "nixium-ide/1.0")
        .send()
        .await
        .map_err(|e| super::McpCallResponse {
            content: format!("Failed to reach crates.io: {e}"),
            is_error: true,
        })?;

    if !resp.status().is_success() {
        return Err(super::McpCallResponse {
            content: format!("crates.io returned HTTP {}", resp.status()),
            is_error: true,
        });
    }

    resp.json::<serde_json::Value>().await.map_err(|e| super::McpCallResponse {
        content: format!("Failed to parse crates.io response: {e}"),
        is_error: true,
    })
}

/// Strip HTML tags and decode common entities from a string.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn remove_tag_blocks(html: &str, tag: &str) -> String {
    let open = format!("<{tag");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.to_lowercase().find(&open) {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        if let Some(end) = after.to_lowercase().find(&close) {
            rest = &after[end + close.len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tool 1: Crate Lookup
// ---------------------------------------------------------------------------

/// Query the crates.io API for information about a Rust crate.
pub async fn call_lookup(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let crate_name = match require_str(args, "crate_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };

    let url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let json = match crates_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let krate = &json["crate"];
    let name = krate["name"].as_str().unwrap_or(&crate_name);
    let version = krate["newest_version"].as_str().unwrap_or("unknown");
    let description = krate["description"].as_str().unwrap_or("No description.");
    let downloads = krate["downloads"].as_u64().unwrap_or(0);
    let repository = krate["repository"].as_str().unwrap_or("");
    let homepage = krate["homepage"].as_str().unwrap_or("");
    let documentation = krate["documentation"].as_str().unwrap_or("");
    let doc_link = if !documentation.is_empty() {
        documentation.to_string()
    } else {
        format!("https://docs.rs/{name}")
    };

    let mut output = format!(
        "## {name} v{version}\n\n{description}\n\nDownloads : {downloads}\nDocs      : {doc_link}\n"
    );
    if !repository.is_empty() { output.push_str(&format!("Repository: {repository}\n")); }
    if !homepage.is_empty()   { output.push_str(&format!("Homepage  : {homepage}\n")); }
    output.push_str(&format!("\nAdd to Cargo.toml:\n```toml\n{name} = \"{version}\"\n```"));

    super::McpCallResponse { content: output, is_error: false }
}

pub const README_LOOKUP: &str = r#"
# Rust Crate Lookup (crates.io)

Fetches metadata for a Rust crate from [crates.io](https://crates.io/).

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `crate_name` | `string` | ✅ | The exact crate name to look up |

## Example usage
> *"What is the latest version of tokio?"*
> *"How do I add axum to my project?"*
"#;

// ---------------------------------------------------------------------------
// Tool 2: Crate Search
// ---------------------------------------------------------------------------

pub async fn call_search(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let query = match require_str(args, "query") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };
    let per_page = args.get("per_page").and_then(|v| v.as_u64()).unwrap_or(5).min(10);

    let url = format!(
        "https://crates.io/api/v1/crates?q={}&per_page={per_page}",
        urlencoding_simple(&query)
    );
    let json = match crates_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let crates = match json["crates"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return super::McpCallResponse {
            content: format!("No crates found for query: {query}"),
            is_error: false,
        },
    };

    let mut output = format!("## crates.io search: \"{query}\"\n\n");
    for krate in crates {
        let name = krate["name"].as_str().unwrap_or("unknown");
        let version = krate["newest_version"].as_str().unwrap_or("?");
        let desc = krate["description"].as_str().unwrap_or("No description.");
        let downloads = krate["downloads"].as_u64().unwrap_or(0);
        output.push_str(&format!(
            "### {name} v{version}\n{desc}\nDownloads: {downloads} | https://crates.io/crates/{name}\n\n"
        ));
    }

    super::McpCallResponse { content: output.trim_end().to_string(), is_error: false }
}

pub const README_SEARCH: &str = r#"
# Rust Crate Search (crates.io)

Searches crates.io by keyword and returns a list of matching crates.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `query` | `string` | ✅ | Search keywords |
| `per_page` | `number` | ❌ | Results to return (1–10, default 5) |

## Example usage
> *"Find crates for async HTTP clients."*
> *"What crates exist for JSON parsing in Rust?"*
"#;

// ---------------------------------------------------------------------------
// Tool 3: Rustc Error Lookup
// ---------------------------------------------------------------------------

pub async fn call_error(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let code = match require_str(args, "error_code") {
        Ok(v) => v.to_uppercase(),
        Err(e) => return e,
    };

    if !code.starts_with('E') || code.len() != 5 || !code[1..].chars().all(|c| c.is_ascii_digit()) {
        return super::McpCallResponse {
            content: format!("`{code}` is not a valid Rust error code. Expected format: E0001–E9999"),
            is_error: true,
        };
    }

    let url = format!("https://doc.rust-lang.org/error_codes/{code}.html");
    let resp = match client.get(&url).header("User-Agent", "nixium-ide/1.0").send().await {
        Err(e) => return super::McpCallResponse {
            content: format!("Failed to reach doc.rust-lang.org: {e}"),
            is_error: true,
        },
        Ok(r) => r,
    };

    if resp.status().as_u16() == 404 {
        return super::McpCallResponse {
            content: format!("No documentation found for error `{code}`. It may be unlisted or internal."),
            is_error: false,
        };
    }
    if !resp.status().is_success() {
        return super::McpCallResponse {
            content: format!("doc.rust-lang.org returned HTTP {}", resp.status()),
            is_error: true,
        };
    }

    let html = match resp.text().await {
        Err(e) => return super::McpCallResponse {
            content: format!("Failed to read response body: {e}"),
            is_error: true,
        },
        Ok(t) => t,
    };

    let body = if let Some(start) = html.find("<main") {
        let inner_start = html[start..].find('>').map(|i| start + i + 1).unwrap_or(start);
        let end = html.find("</main>").unwrap_or(html.len());
        html[inner_start..end].to_string()
    } else {
        html.clone()
    };

    let cleaned = remove_tag_blocks(&body, "script");
    let cleaned = remove_tag_blocks(&cleaned, "style");
    let text = strip_html(&cleaned);

    let lines: Vec<&str> = text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let content = lines.join("\n");
    let truncated = if content.chars().count() > 3000 {
        format!("{}\n\n[…truncated — full docs: {url}]", &content[..3000])
    } else {
        format!("{content}\n\nFull docs: {url}")
    };

    super::McpCallResponse { content: truncated, is_error: false }
}

pub const README_ERROR: &str = r#"
# Rustc Error Lookup

Fetches the official Rust compiler error explanation from [doc.rust-lang.org](https://doc.rust-lang.org/error_codes/).

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `error_code` | `string` | ✅ | Rust error code (e.g. `E0308`) |

## Example usage
> *"What does Rust error E0308 mean?"*
> *"Explain compiler error E0502."*
"#;

// ---------------------------------------------------------------------------
// Tool 4: Crate Dependencies
// ---------------------------------------------------------------------------

pub async fn call_deps(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let crate_name = match require_str(args, "crate_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };

    let version = if let Some(v) = args.get("version").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
        v.to_string()
    } else {
        let meta_url = format!("https://crates.io/api/v1/crates/{crate_name}");
        match crates_get(&meta_url, client).await {
            Ok(json) => json["crate"]["newest_version"].as_str().unwrap_or("latest").to_string(),
            Err(e) => return e,
        }
    };

    let url = format!("https://crates.io/api/v1/crates/{crate_name}/{version}/dependencies");
    let json = match crates_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let deps = match json["dependencies"].as_array() {
        Some(arr) => arr,
        None => return super::McpCallResponse {
            content: format!("No dependency data found for {crate_name} v{version}."),
            is_error: false,
        },
    };

    let mut normal: Vec<String> = Vec::new();
    let mut dev: Vec<String>    = Vec::new();
    let mut build: Vec<String>  = Vec::new();

    for dep in deps {
        let name = dep["crate_id"].as_str().unwrap_or("unknown");
        let req  = dep["req"].as_str().unwrap_or("*");
        let kind = dep["kind"].as_str().unwrap_or("normal");
        let optional = dep["optional"].as_bool().unwrap_or(false);
        let entry = if optional { format!("{name} {req} (optional)") } else { format!("{name} {req}") };
        match kind {
            "dev"   => dev.push(entry),
            "build" => build.push(entry),
            _       => normal.push(entry),
        }
    }

    let mut output = format!("## Dependencies of {crate_name} v{version}\n\n");
    if normal.is_empty() {
        output.push_str("No runtime dependencies.\n");
    } else {
        output.push_str(&format!("### Runtime ({} deps)\n", normal.len()));
        for d in &normal { output.push_str(&format!("- {d}\n")); }
    }
    if !dev.is_empty() {
        output.push_str(&format!("\n### Dev ({} deps)\n", dev.len()));
        for d in &dev { output.push_str(&format!("- {d}\n")); }
    }
    if !build.is_empty() {
        output.push_str(&format!("\n### Build ({} deps)\n", build.len()));
        for d in &build { output.push_str(&format!("- {d}\n")); }
    }

    super::McpCallResponse { content: output.trim_end().to_string(), is_error: false }
}

pub const README_DEPS: &str = r#"
# Crate Dependencies

Fetches the dependency list for a specific crate version from crates.io, split by runtime, dev, and build dependencies.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `crate_name` | `string` | ✅ | Crate name |
| `version` | `string` | ❌ | Version (defaults to latest) |

## Example usage
> *"What does tokio depend on?"*
> *"Show me the dependencies for serde 1.0.0."*
"#;

// ---------------------------------------------------------------------------
// Tool 5: Crate Version History
// ---------------------------------------------------------------------------

pub async fn call_versions(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let crate_name = match require_str(args, "crate_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };

    let url = format!("https://crates.io/api/v1/crates/{crate_name}/versions");
    let json = match crates_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let versions = match json["versions"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return super::McpCallResponse {
            content: format!("No version data found for `{crate_name}`."),
            is_error: false,
        },
    };

    let limit = 20;
    let mut output = format!("## Version history for `{crate_name}` ({} total)\n\n", versions.len());
    output.push_str("| Version | Published | Downloads | Yanked |\n");
    output.push_str("|---|---|---|---|\n");

    for v in versions.iter().take(limit) {
        let num     = v["num"].as_str().unwrap_or("?");
        let created = v["created_at"].as_str().unwrap_or("?").split('T').next().unwrap_or("?");
        let dls     = v["downloads"].as_u64().unwrap_or(0);
        let yanked  = if v["yanked"].as_bool().unwrap_or(false) { "⚠️ yes" } else { "no" };
        output.push_str(&format!("| {num} | {created} | {dls} | {yanked} |\n"));
    }
    if versions.len() > limit {
        output.push_str(&format!("\n*Showing latest {limit} of {} versions.*", versions.len()));
    }

    super::McpCallResponse { content: output.trim_end().to_string(), is_error: false }
}

pub const README_VERSIONS: &str = r#"
# Crate Version History

Lists all published versions of a crate from crates.io, including publish date, downloads, and whether a version was yanked.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `crate_name` | `string` | ✅ | Crate name |

## Example usage
> *"What versions of clap are available?"*
> *"Has any version of ring been yanked?"*
"#;

// ---------------------------------------------------------------------------
// Tool 6: docs.rs Item Lookup
// ---------------------------------------------------------------------------

pub async fn call_docs(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let crate_name = match require_str(args, "crate_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };
    let item = args.get("item").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

    let meta_url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let version = match crates_get(&meta_url, client).await {
        Ok(json) => json["crate"]["newest_version"].as_str().unwrap_or("latest").to_string(),
        Err(e) => return e,
    };

    let module_name = crate_name.replace('-', "_");
    let direct_url = format!("https://docs.rs/{crate_name}/{version}/{module_name}/");
    let search_url = if item.is_empty() {
        direct_url.clone()
    } else {
        format!("{direct_url}?search={}", urlencoding_simple(&item))
    };

    let exists = client
        .head(&direct_url)
        .header("User-Agent", "nixium-ide/1.0")
        .send()
        .await
        .map(|r| r.status().is_success() || r.status().as_u16() == 301 || r.status().as_u16() == 302)
        .unwrap_or(false);

    let status_note = if exists { "" } else { "\n\n⚠️ Could not verify this URL — docs may be under a different module path." };

    let mut output = format!("## docs.rs — `{crate_name}` v{version}\n\n");
    if item.is_empty() {
        output.push_str(&format!("Crate root: {direct_url}"));
    } else {
        output.push_str(&format!("Search `{item}` in `{crate_name}`:\n{search_url}\n\nCrate root: {direct_url}"));
    }
    output.push_str(status_note);

    super::McpCallResponse { content: output, is_error: false }
}

pub const README_DOCS: &str = r#"
# docs.rs Item Lookup

Returns documentation links for a Rust crate and optionally searches for a specific item on [docs.rs](https://docs.rs).

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `crate_name` | `string` | ✅ | Crate name |
| `item` | `string` | ❌ | Item to search for (struct, fn, trait, etc.) |

## Example usage
> *"Where are the docs for tokio::spawn?"*
> *"Find the Serialize trait in serde."*
> *"Show me the docs.rs page for axum."*
"#;
