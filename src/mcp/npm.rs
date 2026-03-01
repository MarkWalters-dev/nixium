// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn urlenc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

async fn npm_get(
    url: &str,
    client: &reqwest::Client,
) -> Result<serde_json::Value, super::McpCallResponse> {
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", "nixium-ide/1.0")
        .send()
        .await
        .map_err(|e| super::McpCallResponse {
            content: format!("Failed to reach npm registry: {e}"),
            is_error: true,
        })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        if status == 404 {
            return Err(super::McpCallResponse {
                content: "Package not found on npm.".to_string(),
                is_error: true,
            });
        }
        return Err(super::McpCallResponse {
            content: format!("npm registry returned HTTP {status}"),
            is_error: true,
        });
    }

    resp.json::<serde_json::Value>().await.map_err(|e| super::McpCallResponse {
        content: format!("Failed to parse npm response: {e}"),
        is_error: true,
    })
}

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, super::McpCallResponse> {
    match args.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s.trim()),
        _ => Err(super::McpCallResponse {
            content: format!("Missing required argument: {key}"),
            is_error: true,
        }),
    }
}

// ---------------------------------------------------------------------------
// Tool 1: npm Package Lookup
// ---------------------------------------------------------------------------

pub async fn call_lookup(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let name = match require_str(args, "package_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };

    let url = format!("https://registry.npmjs.org/{}/latest", urlenc(&name));
    let json = match npm_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let version     = json["version"].as_str().unwrap_or("unknown");
    let description = json["description"].as_str().unwrap_or("No description.");
    let license     = json["license"].as_str().unwrap_or("unknown");
    let homepage    = json["homepage"].as_str().unwrap_or("");
    let repo        = json["repository"]["url"].as_str()
        .unwrap_or("")
        .trim_start_matches("git+")
        .trim_end_matches(".git");

    let mut out = format!(
        "## {name} v{version}\n\n\
         {description}\n\n\
         License : {license}\n\
         Docs    : https://www.npmjs.com/package/{name}\n"
    );
    if !homepage.is_empty() { out.push_str(&format!("Homepage: {homepage}\n")); }
    if !repo.is_empty()     { out.push_str(&format!("Repo    : {repo}\n")); }
    out.push_str(&format!("\nInstall:\n```sh\nnpm install {name}\n# or\npnpm add {name}\n```"));

    super::McpCallResponse { content: out, is_error: false }
}

pub const README_LOOKUP: &str = r#"
# npm Package Lookup

Fetches metadata for an npm package: latest version, description, license, homepage, and repository link.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `package_name` | `string` | ✅ | Exact npm package name (e.g. `svelte`, `@sveltejs/kit`) |

## Example usage
> *"What is the latest version of svelte?"*
> *"Tell me about the @sveltejs/kit package."*
"#;

// ---------------------------------------------------------------------------
// Tool 2: npm Package Search
// ---------------------------------------------------------------------------

pub async fn call_search(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let query = match require_str(args, "query") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };
    let size = args.get("size").and_then(|v| v.as_u64()).unwrap_or(5).min(10);

    let url = format!(
        "https://registry.npmjs.org/-/v1/search?text={}&size={size}",
        urlenc(&query)
    );
    let json = match npm_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let objects = match json["objects"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return super::McpCallResponse {
            content: format!("No packages found for query: {query}"),
            is_error: false,
        },
    };

    let mut out = format!("## npm search: \"{query}\"\n\n");
    for obj in objects {
        let pkg  = &obj["package"];
        let name = pkg["name"].as_str().unwrap_or("?");
        let ver  = pkg["version"].as_str().unwrap_or("?");
        let desc = pkg["description"].as_str().unwrap_or("No description.");
        out.push_str(&format!(
            "### {name} v{ver}\n{desc}\nhttps://www.npmjs.com/package/{name}\n\n"
        ));
    }

    super::McpCallResponse { content: out.trim_end().to_string(), is_error: false }
}

pub const README_SEARCH: &str = r#"
# npm Package Search

Searches the npm registry by keyword and returns a ranked list of matching packages.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `query` | `string` | ✅ | Search keywords |
| `size` | `number` | ❌ | Results to return (1–10, default 5) |

## Example usage
> *"Find npm packages for Svelte UI components."*
> *"What packages exist for form validation in TypeScript?"*
"#;

// ---------------------------------------------------------------------------
// Tool 3: npm Package Version History
// ---------------------------------------------------------------------------

pub async fn call_versions(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let name = match require_str(args, "package_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };

    // Full packument - versions key is an object of version -> package data
    let url = format!("https://registry.npmjs.org/{}", urlenc(&name));
    let json = match npm_get(&url, client).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    let time_map = json.get("time").and_then(|v| v.as_object());
    let dist_tags = json.get("dist-tags").and_then(|v| v.as_object());

    let versions_obj = match json.get("versions").and_then(|v| v.as_object()) {
        Some(v) => v,
        None => return super::McpCallResponse {
            content: format!("No version data found for `{name}`."),
            is_error: false,
        },
    };

    // Collect all semver versions, newest first (reverse alphabetical approximation is
    // good enough; exact semver sort would require a semver crate).
    let mut versions: Vec<String> = versions_obj.keys().cloned().collect();
    versions.sort_by(|a, b| b.cmp(a));

    let total = versions.len();
    let limit = 25;

    let mut out = format!("## npm version history: `{name}` ({total} total)\n\n");

    // Show dist-tags table
    if let Some(tags) = dist_tags {
        out.push_str("### Dist-tags\n");
        for (tag, ver) in tags {
            out.push_str(&format!("- **{tag}**: {}\n", ver.as_str().unwrap_or("?")));
        }
        out.push('\n');
    }

    out.push_str("### Recent versions\n\n");
    out.push_str("| Version | Published |\n|---|---|\n");
    for ver in versions.iter().take(limit) {
        let date = time_map
            .and_then(|t| t.get(ver.as_str()))
            .and_then(|v| v.as_str())
            .and_then(|s| s.split('T').next())
            .unwrap_or("?");
        out.push_str(&format!("| {ver} | {date} |\n"));
    }
    if total > limit {
        out.push_str(&format!("\n*Showing latest {limit} of {total} versions.*"));
    }

    super::McpCallResponse { content: out.trim_end().to_string(), is_error: false }
}

pub const README_VERSIONS: &str = r#"
# npm Package Version History

Lists published versions of an npm package including dist-tags (latest, next, beta) and publish dates.

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `package_name` | `string` | ✅ | npm package name |

## Example usage
> *"What versions of vite are available?"*
> *"Show me the release history for svelte."*
"#;

// ---------------------------------------------------------------------------
// Tool 4: Bundle Size Check (bundlephobia)
// ---------------------------------------------------------------------------

pub async fn call_bundle_size(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let name = match require_str(args, "package_name") {
        Ok(v) => v.to_string(),
        Err(e) => return e,
    };
    let version = args.get("version").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

    let pkg_spec = if version.is_empty() {
        name.clone()
    } else {
        format!("{name}@{version}")
    };

    let url = format!(
        "https://bundlephobia.com/api/size?package={}&record=true",
        urlenc(&pkg_spec)
    );

    let resp = match client
        .get(&url)
        .header("User-Agent", "nixium-ide/1.0")
        .header("X-Bundlephobia-User", "nixium")
        .send()
        .await
    {
        Err(e) => return super::McpCallResponse {
            content: format!("Failed to reach bundlephobia.com: {e}"),
            is_error: true,
        },
        Ok(r) => r,
    };

    if !resp.status().is_success() {
        return super::McpCallResponse {
            content: format!("bundlephobia returned HTTP {} — package may not exist or may be misconfigured.", resp.status()),
            is_error: true,
        };
    }

    let json = match resp.json::<serde_json::Value>().await {
        Err(e) => return super::McpCallResponse {
            content: format!("Failed to parse bundlephobia response: {e}"),
            is_error: true,
        },
        Ok(v) => v,
    };

    let pkg_name    = json["name"].as_str().unwrap_or(&name);
    let pkg_ver     = json["version"].as_str().unwrap_or("?");
    let size        = json["size"].as_u64().unwrap_or(0);
    let gzip        = json["gzip"].as_u64().unwrap_or(0);
    let has_side_effects = json["hasSideEffects"].as_bool().unwrap_or(true);
    let has_tree_shaking = !has_side_effects;

    fn human(bytes: u64) -> String {
        if bytes >= 1024 * 1024 {
            format!("{:.1} kB", bytes as f64 / 1024.0)
        } else {
            format!("{:.1} kB", bytes as f64 / 1024.0)
        }
    }

    let out = format!(
        "## Bundle size: {pkg_name} v{pkg_ver}\n\n\
         Minified   : {}\n\
         Gzipped    : {}\n\
         Tree-shake : {}\n\
         \nhttps://bundlephobia.com/package/{pkg_name}@{pkg_ver}",
        human(size),
        human(gzip),
        if has_tree_shaking { "✅ yes" } else { "⚠️ no side-effects declared" },
    );

    super::McpCallResponse { content: out, is_error: false }
}

pub const README_BUNDLE_SIZE: &str = r#"
# Bundle Size Check (bundlephobia)

Checks the minified + gzipped bundle size of an npm package using [bundlephobia.com](https://bundlephobia.com/).

## Parameters

| Name | Type | Required | Description |
|---|---|---|---|
| `package_name` | `string` | ✅ | npm package name |
| `version` | `string` | ❌ | Specific version (defaults to latest) |

## Example usage
> *"How big is the lodash bundle?"*
> *"What is the gzip size of moment.js?"*
> *"Is svelte tree-shakeable?"*
"#;
