/// cargo run --bin test_gdoc
/// Fetches a Google Doc, runs extract_tab_markdown, prints result + raw element list.

#[tokio::main]
async fn main() {
    let doc_id = "1mHs-Oyn1NRMyCvUwl06LpLSvL_7aSPkFOJEYDAQT_EE";

    // ── Load token ────────────────────────────────────────────────────────────
    let home = std::env::var("HOME").expect("no $HOME");
    let settings_str = std::fs::read_to_string(format!("{home}/.bamako/settings.json"))
        .expect("can't read ~/.bamako/settings.json");
    let settings: serde_json::Value =
        serde_json::from_str(&settings_str).expect("bad JSON");
    let token = settings["google_access_token"]
        .as_str()
        .expect("no google_access_token");

    // ── Fetch doc ─────────────────────────────────────────────────────────────
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("https://docs.googleapis.com/v1/documents/{doc_id}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("request failed");

    let doc: serde_json::Value = resp.json().await.expect("json parse");
    if let Some(err) = doc["error"]["message"].as_str() {
        eprintln!("Docs API error: {err}");
        std::process::exit(1);
    }

    // dump raw JSON for inspection
    std::fs::write("/tmp/gdoc_raw.json", serde_json::to_string_pretty(&doc).unwrap()).ok();
    println!("Raw JSON → /tmp/gdoc_raw.json\n");

    let empty = serde_json::Value::Object(Default::default());
    let inline_objects = doc.get("inlineObjects").cloned().unwrap_or(empty.clone());

    let top_tabs = match doc["tabs"].as_array() {
        Some(t) => t.clone(),
        None => {
            let md = extract_tab_markdown(&doc["body"], &empty);
            println!("=== ROOT BODY ===\n{md}");
            return;
        }
    };

    walk_tabs(&top_tabs, &inline_objects);
}

fn walk_tabs(tabs: &[serde_json::Value], fallback_inline: &serde_json::Value) {
    for tab in tabs {
        let title = tab.pointer("/tabProperties/title")
            .and_then(|v| v.as_str()).unwrap_or("Untitled");
        println!("\n{}\nTAB: {title}\n{}", "=".repeat(60), "=".repeat(60));

        if let Some(body) = tab.pointer("/documentTab/body") {
            let inline = tab.pointer("/documentTab/inlineObjects")
                .cloned()
                .unwrap_or_else(|| fallback_inline.clone());

            // Print raw paragraph structure
            println!("--- RAW PARAGRAPHS ---");
            if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
                for (i, elem) in content.iter().enumerate() {
                    if let Some(para) = elem.get("paragraph") {
                        let style = para.pointer("/paragraphStyle/namedStyleType")
                            .and_then(|v| v.as_str()).unwrap_or("?");
                        let is_bullet = para.get("bullet").is_some();
                        let runs: Vec<String> = para.get("elements")
                            .and_then(|e| e.as_array())
                            .map(|rs| rs.iter().filter_map(|r| {
                                r.get("textRun").and_then(|tr| tr["content"].as_str())
                                    .map(|s| format!("{:?}", s))
                            }).collect())
                            .unwrap_or_default();
                        println!("  [{i}] {style} bullet={is_bullet} runs={}", runs.join(", "));
                    }
                }
            }

            println!("\n--- GENERATED MARKDOWN ---");
            let md = extract_tab_markdown(body, &inline);
            println!("{md}");
        }

        if let Some(children) = tab.get("childTabs").and_then(|v| v.as_array()) {
            walk_tabs(children, fallback_inline);
        }
    }
}

// ── extract_tab_markdown (mirror of import.rs) ────────────────────────────────

fn is_code_font(text_run: &serde_json::Value) -> bool {
    let family = text_run.pointer("/textStyle/weightedFontFamily/fontFamily")
        .and_then(|v| v.as_str()).unwrap_or("");
    matches!(family,
        "Courier New"|"Courier"|"Consolas"|"Roboto Mono"|"Source Code Pro"
        |"Inconsolata"|"Fira Mono"|"Fira Code"|"Lucida Console"|"Monaco")
}

fn extract_tab_markdown(body: &serde_json::Value, _inline_objects: &serde_json::Value) -> String {
    let mut md = String::new();
    let elements = match body.get("content").and_then(|c| c.as_array()) {
        Some(e) => e,
        None => return md,
    };
    let mut prev_was_list = false;

    for elem in elements {
        if let Some(para) = elem.get("paragraph") {
            let style = para.pointer("/paragraphStyle/namedStyleType")
                .and_then(|s| s.as_str()).unwrap_or("NORMAL_TEXT");
            let is_bullet = para.get("bullet").is_some();
            let nesting = para.pointer("/bullet/nestingLevel")
                .and_then(|v| v.as_u64()).unwrap_or(0) as usize;

            let mut line = String::new();
            let mut pending_break = false;

            if let Some(runs) = para.get("elements").and_then(|e| e.as_array()) {
                for run in runs {
                    if run.pointer("/inlineObjectElement/inlineObjectId").is_some() { continue; }
                    if let Some(text_run) = run.get("textRun") {
                        let raw = text_run["content"].as_str().unwrap_or("");
                        let bold   = text_run.pointer("/textStyle/bold").and_then(|v| v.as_bool()).unwrap_or(false);
                        let italic = text_run.pointer("/textStyle/italic").and_then(|v| v.as_bool()).unwrap_or(false);
                        let code   = is_code_font(text_run);
                        let link   = text_run.pointer("/textStyle/link/url").and_then(|v| v.as_str());

                        let raw_stripped = raw.trim_end_matches('\n');
                        let parts: Vec<&str> = raw_stripped.split('\x0b').collect();

                        for (i, part) in parts.iter().enumerate() {
                            if part.is_empty() {
                                if !line.is_empty() { pending_break = true; }
                                continue;
                            }
                            if (i > 0 || pending_break) && !line.is_empty() {
                                if is_bullet { line.push_str("  \n"); } else { line.push_str("\n\n"); }
                            }
                            pending_break = false;
                            let styled = if code { format!("`{part}`") }
                                else if bold && italic { format!("***{part}***") }
                                else if bold  { format!("**{part}**") }
                                else if italic { format!("*{part}*") }
                                else { part.to_string() };
                            if let Some(url) = link {
                                line.push_str(&format!("[{styled}]({url})"));
                            } else {
                                line.push_str(&styled);
                            }
                        }
                    }
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                if prev_was_list { md.push('\n'); }
                prev_was_list = false;
                md.push('\n');
                continue;
            }

            if is_bullet {
                let indent = "  ".repeat(nesting);
                md.push_str(&format!("{indent}- {trimmed}\n"));
                prev_was_list = true;
            } else {
                if prev_was_list { md.push('\n'); }
                prev_was_list = false;
                match style {
                    "TITLE"     => md.push_str(&format!("# {trimmed}\n\n")),
                    "HEADING_1" => md.push_str(&format!("# {trimmed}\n\n")),
                    "HEADING_2" => md.push_str(&format!("## {trimmed}\n\n")),
                    "HEADING_3" => md.push_str(&format!("### {trimmed}\n\n")),
                    "HEADING_4" => md.push_str(&format!("#### {trimmed}\n\n")),
                    "HEADING_5" => md.push_str(&format!("##### {trimmed}\n\n")),
                    _           => md.push_str(&format!("{trimmed}\n\n")),
                }
            }
        } else if let Some(table) = elem.get("table") {
            if prev_was_list { md.push('\n'); }
            prev_was_list = false;
            if let Some(rows) = table.get("tableRows").and_then(|r| r.as_array()) {
                for (i, row) in rows.iter().enumerate() {
                    let cells: Vec<String> = row.get("tableCells")
                        .and_then(|c| c.as_array())
                        .map(|cells| cells.iter().map(|cell| {
                            cell.get("content").and_then(|c| c.as_array())
                                .map(|elems| extract_tab_markdown(
                                    &serde_json::json!({"content": elems}),
                                    &serde_json::Value::Null,
                                ).replace('\n', " ").trim().to_string())
                                .unwrap_or_default()
                        }).collect())
                        .unwrap_or_default();
                    if cells.is_empty() { continue; }
                    md.push_str(&format!("| {} |\n", cells.join(" | ")));
                    if i == 0 {
                        let sep = cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | ");
                        md.push_str(&format!("| {sep} |\n"));
                    }
                }
                md.push('\n');
            }
        }
    }
    md
}
