use serde_json::Value;
use std::io::IsTerminal;

const RESET: &str = "\x1b[0m";
const BRAND: &str = "\x1b[1;96m";
const HEADING: &str = "\x1b[1m";
const MUTED: &str = "\x1b[2m";
const ACCENT: &str = "\x1b[94m";
const GOOD: &str = "\x1b[92m";
const WARN: &str = "\x1b[93m";
const BAD: &str = "\x1b[91m";

fn enabled(terminal: bool, no_color: Option<&str>, term: Option<&str>) -> bool {
    terminal && no_color.is_none_or(|value| value.is_empty())
        && !matches!(term, Some("dumb" | "xterm-mono"))
}

pub fn stdout_color() -> bool {
    enabled(std::io::stdout().is_terminal(), std::env::var("NO_COLOR").ok().as_deref(), std::env::var("TERM").ok().as_deref())
}

fn paint(output: &mut String, text: &str, color: &str) {
    output.push_str(color);
    output.push_str(text);
    output.push_str(RESET);
}

fn styled(text: &str, style: &str, color: bool) -> String {
    if color { format!("{style}{text}{RESET}") } else { text.to_owned() }
}

// Names and provider messages can come from guests or remote APIs. They must not
// be able to move the cursor, forge another row, or change the terminal theme.
fn safe(text: &str) -> String {
    text.chars().filter_map(|c| match c {
        '\n' | '\r' | '\t' => Some(' '),
        c if c.is_control() => None,
        c => Some(c),
    }).collect()
}

fn section(title: &str, color: bool) -> String {
    format!("{}\n", styled(&safe(&title.to_uppercase()), BRAND, color))
}

fn tone(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "running" | "ready" | "connected" | "healthy" | "ok" | "in use" | "active" | "published" => GOOD,
        "error" | "failed" | "fail" | "needs attention" | "unavailable" => BAD,
        "warning" | "warn" | "starting" | "stopping" | "pending" | "provisioning" => WARN,
        _ => MUTED,
    }
}

pub fn json(value: &Value, color: bool) -> String {
    let plain = serde_json::to_string_pretty(value).unwrap();
    if !color { return plain; }
    // Tokenize the serializer's JSON, never the unescaped values themselves.
    // Removing our SGR sequences gives exactly the same JSON for every value.
    let bytes = plain.as_bytes();
    let mut output = String::with_capacity(plain.len());
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => { i += 1; break; }
                        _ => i += 1,
                    }
                }
                let key = plain[i..].trim_start().starts_with(':');
                paint(&mut output, &plain[start..i], if key { "\x1b[96m" } else { "\x1b[92m" });
            }
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n' => {
                i += 1;
                while i < bytes.len() && !matches!(bytes[i], b',' | b']' | b'}' | b' ' | b'\r' | b'\n') { i += 1; }
                let token = &plain[start..i];
                paint(&mut output, token, match token {
                    "true" => "\x1b[92m", "false" => "\x1b[91m", "null" => "\x1b[90m", _ => "\x1b[93m",
                });
            }
            _ => { output.push(bytes[i] as char); i += 1; }
        }
    }
    output
}

pub fn help(text: &str, color: bool) -> String {
    if !color { return text.into(); }
    let mut output = String::with_capacity(text.len() + 240);
    output.push_str(&styled("  ◆  YOUGORI", BRAND, color));
    output.push_str("\n  ");
    output.push_str(&styled("Your compute, one command", MUTED, color));
    output.push_str("\n\n");
    output.push_str(&section("Start here", color));
    for (command, description) in [
        ("yougori", "Open the main menu and add this project's shortcut"),
        ("yougori cli", "Choose actions from interactive menus"),
        ("yougori run nginx", "Create and start a container"),
        ("yougori status", "See environments and connections"),
        ("yougori doctor", "Check this computer"),
    ] {
        output.push_str("  ");
        output.push_str(&styled(command, HEADING, color));
        output.push_str("  ");
        output.push_str(&styled(description, MUTED, color));
        output.push('\n');
    }
    output.push('\n');
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        if content.starts_with("Yougori —") {
            // The brand and subtitle above replace the old introductory line.
            continue;
        } else if content.starts_with("Yougori CLI") {
            output.push_str(&section("Advanced control", color));
        } else if content.ends_with(':') && !content.starts_with("  ") {
            output.push_str(&section(content.trim_end_matches(':'), color));
        } else if let Some(command) = content.strip_prefix("  ").filter(|line| {
            line.split_whitespace().next().is_some_and(|word| matches!(word,
                "yougori" | "app" | "env" | "connection" | "remote" | "share" | "ports" | "snapshot" | "backup" | "gpu" |
                "terminal" | "microvm" | "neocloud" | "agent" | "window" | "settings" | "jobs" | "skills" | "schema" | "call" | "cloud" | "domain" | "drives" | "project" | "lan" | "storage"))
        }) {
            let (usage, description) = command.split_once("  ").unwrap_or((command, ""));
            output.push_str("  ");
            paint(&mut output, usage, HEADING);
            if !description.is_empty() { output.push_str("  "); paint(&mut output, description, MUTED); }
        } else { output.push_str(content); }
        if line.ends_with('\n') { output.push('\n'); }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn without_sgr(text: &str) -> String {
        let mut output = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                assert_eq!(chars.next(), Some('['));
                for c in chars.by_ref() { if c == 'm' { break; } assert!(c.is_ascii_digit() || c == ';'); }
            } else { output.push(c); }
        }
        output
    }
    #[test]
    fn redirects_and_explicit_plain_text_preferences_never_receive_ansi() {
        for terminal in [false, true] {
            assert!(!enabled(terminal, Some("1"), Some("xterm-256color")));
            assert!(!enabled(terminal, None, Some("dumb")));
        }
        assert!(!enabled(false, None, Some("xterm-256color")));
        assert!(enabled(true, None, Some("xterm-256color")));
        assert!(enabled(true, Some(""), None));
    }
    #[test]
    fn coloured_json_round_trips_unicode_escapes_nested_values_and_numbers() {
        let value = serde_json::json!({"name":"żółty ✓", "quoted":"\"key\": false", "escape":"\x1b[31m", "path":"C:\\files\\", "values":[true,false,null,-4,2.5,1.23e30,{"inner":"test"}]});
        let plain = json(&value, false);
        let rendered = json(&value, true);
        assert_eq!(without_sgr(&rendered), plain);
        assert_eq!(serde_json::from_str::<Value>(&plain).unwrap(), value);
        assert!(rendered.contains("\x1b[96m\"name\"\x1b[0m"));
        assert!(rendered.contains("\x1b[91mfalse\x1b[0m"));
        assert!(!plain.contains('\x1b'));
        for value in [serde_json::json!(null), serde_json::json!("top-level ✓"), serde_json::json!(-0.2)] {
            assert_eq!(without_sgr(&json(&value, true)), json(&value, false));
        }
    }
    #[test]
    fn help_has_a_short_start_and_keeps_every_command_available() {
        let plain = format!("Yougori — environments, models and Personal Vault\n{}\n{}", yougori_cli::public::HELP, yougori_cli::parse::HELP);
        assert_eq!(help(&plain, false), plain);
        let rendered = without_sgr(&help(&plain, true));
        assert!(rendered.starts_with("  ◆  YOUGORI\n  Your compute, one command"));
        assert!(rendered.contains("yougori run nginx"));
        assert!(rendered.contains("PUBLIC COMMANDS"));
        assert!(rendered.contains("ADVANCED CONTROL"));
        assert!(rendered.contains("app start [--engine]|status|show|quit"));
        assert!(rendered.contains("yougori model run"));
    }
    #[test]
    fn tables_are_aligned_and_guest_text_cannot_control_the_terminal() {
        let rows = vec![vec!["web\x1b[2J\nforged".into(), "running".into()], vec!["db".into(), "stopped".into()]];
        let plain = table(&["NAME", "STATUS"], &rows, false);
        let colored = table(&["NAME", "STATUS"], &rows, true);
        assert!(!plain.contains('\x1b'));
        assert!(!plain.contains("\nforged"));
        assert!(plain.contains("web[2J forged"));
        assert!(plain.contains("────"));
        assert_eq!(without_sgr(&colored), plain);
        assert!(colored.contains("\x1b[92mrunning"));
    }
    #[test]
    fn wide_values_shorten_to_the_available_terminal_width() {
        let rows = vec![vec!["a very long environment name".into(), "running".into(), "https://example.com/this/is/a/very/long/address".into()]];
        let rendered = table_at_width(&["NAME", "STATUS", "ADDRESS"], &rows, false, Some(60));
        assert!(rendered.contains('…'));
        assert!(rendered.lines().all(|line| line.chars().count() <= 60), "{rendered}");
        assert!(rendered.contains("running"));
    }
    #[test]
    fn terminal_errors_remain_readable_and_cannot_inject_control_sequences() {
        let plain = error("Bad name\x1b[2J\nTry another", false);
        assert!(plain.contains("Bad name[2J\n   Try another"));
        assert!(!plain.contains('\x1b'));
        assert_eq!(without_sgr(&error("Bad name\x1b[2J\nTry another", true)), plain);
    }
    #[test]
    fn volume_tables_show_users_sizes_and_what_was_not_checked() {
        let listing = serde_json::json!({"volumes":[
            {"name":"data","inUse":true,"mounts":[{"environment":"db","target":"/var/lib/db"}],"stored":[{"location":"C:\\Yougori\\runtime","usedBy":["db"],"sizeBytes":1_610_612_736u64,"sizeComplete":true}]},
            {"name":"old","inUse":false,"mounts":[],"stored":[{"location":"C:\\Yougori\\runtime","usedBy":[],"sizeBytes":10,"sizeComplete":false}]},
        ],"checked":["C:\\Yougori\\runtime"],"problems":["D:\\Yougori: drive offline"],"note":"Only running container runtimes were checked."});
        let text = volumes(&listing, false);
        assert!(text.contains("data  in use  1.5 GB"), "{text}");
        assert!(text.contains("db:/var/lib/db"), "{text}");
        assert!(!text.contains("db:/var/lib/db, db"), "{text}");
        assert!(text.contains("old   unused  10 B+"), "{text}");
        assert!(text.contains("Not checked: D:\\Yougori: drive offline"));
        assert!(text.contains("Only running"));
    }
    #[test]
    fn neocloud_table_shows_rank_and_partial_provider_failures() {
        let report = serde_json::json!({
            "hours": 8, "discoveredTotal": 3, "rankedTotal": 1, "unrankedTotal": 1, "unavailableTotal": 1,
            "ranked": [{"rank":1,"provider":"vast","name":"RTX 4090","gpuCount":1,"vramGb":24,"hourlyUsd":1.125,"estimatedComputeUsd":9,"offerId":"123"}],
            "unranked": [{"provider":"jarvis"}],
            "providers": [{"provider":"runpod","status":"error","message":"not signed in"}]
        });
        let output = neocloud_prices(&report, false);
        assert!(output.contains("RTX 4090"));
        assert!(output.contains("$1.125"));
        assert!(output.contains("$9.00"));
        assert!(output.contains("runpod: error — not signed in"));
        assert!(output.contains("1 offer was marked unavailable"));
        assert!(!output.contains('\u{1b}'));
    }
}

pub fn stdout_terminal() -> bool {
    std::io::stdout().is_terminal()
}

pub fn error(message: &str, color: bool) -> String {
    let mut output = format!("{}\n", styled("×  Yougori could not complete the command", BAD, color));
    for line in message.lines() {
        output.push_str("   ");
        output.push_str(&safe(line));
        output.push('\n');
    }
    output
}

/// Compact columns with a quiet rule and color reserved for meaningful state.
pub fn table(headers: &[&str], rows: &[Vec<String>], color: bool) -> String {
    let width = if stdout_terminal() { crossterm::terminal::size().ok().map(|(columns, _)| columns as usize) } else { None };
    table_at_width(headers, rows, color, width)
}

fn table_at_width(headers: &[&str], rows: &[Vec<String>], color: bool, max_width: Option<usize>) -> String {
    if headers.is_empty() { return String::new(); }
    let cells: Vec<Vec<String>> = rows.iter().map(|row| row.iter().map(|cell| safe(cell)).collect()).collect();
    let mut widths: Vec<usize> = (0..headers.len())
        .map(|i| cells.iter().filter_map(|r| r.get(i)).map(|cell| cell.chars().count()).chain([headers[i].chars().count()]).max().unwrap_or(0))
        .collect();
    if let Some(max_width) = max_width.filter(|width| *width >= 60) {
        let mut excess = widths.iter().sum::<usize>() + 2 * (headers.len() - 1);
        excess = excess.saturating_sub(max_width);
        let mut flexible: Vec<usize> = (0..headers.len()).collect();
        flexible.sort_by_key(|&i| match headers[i] {
            "ID" | "OFFER ID" => 0,
            "PUBLISHED" | "ADDRESS" | "DETAIL" | "STORED IN" | "USED BY" => 1,
            "OFFER" => 2,
            "NAME" | "ENVIRONMENT" => 3,
            _ => 4,
        });
        for i in flexible {
            if excess == 0 { break; }
            let floor = headers[i].chars().count().max(if matches!(headers[i], "NAME" | "ENVIRONMENT" | "ADDRESS" | "PUBLISHED" | "DETAIL" | "OFFER" | "ID" | "OFFER ID" | "STORED IN" | "USED BY") { 12 } else { 0 });
            let cut = excess.min(widths[i].saturating_sub(floor));
            widths[i] -= cut;
            excess -= cut;
        }
    }
    let line = |values: &[String], status: bool| {
        let last = headers.len() - 1;
        (0..headers.len()).map(|i| {
            let cell = values.get(i).map(String::as_str).unwrap_or("");
            let shortened = if cell.chars().count() > widths[i] {
                format!("{}…", cell.chars().take(widths[i].saturating_sub(1)).collect::<String>())
            } else { cell.to_owned() };
            let padded = if i == last { shortened } else { format!("{shortened:<width$}", width = widths[i]) };
            if color && status && matches!(headers[i], "STATUS" | "STATE") {
                styled(&padded, tone(cell), true)
            } else if color && status && headers[i].is_empty() {
                styled(&padded, tone(cell), true)
            } else { padded }
        }).collect::<Vec<_>>().join("  ").trim_end().to_owned()
    };
    let head = line(&headers.iter().map(|h| h.to_string()).collect::<Vec<_>>(), false);
    let rule = widths.iter().enumerate().map(|(i, width)| "─".repeat(width + if i + 1 == widths.len() { 0 } else { 2 })).collect::<String>();
    let mut output = format!("{}\n{}\n", styled(&head, HEADING, color), styled(&rule, MUTED, color));
    for row in &cells {
        output.push_str(&line(row, true));
        output.push('\n');
    }
    output
}

fn text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| if value.is_null() { "—".into() } else { value.to_string() })
}
fn percent(value: &Value) -> String { value.as_f64().map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into()) }
fn gigabytes(value: &Value) -> String { value.as_f64().map(|v| format!("{v:.1} GB")).unwrap_or_else(|| "—".into()) }
fn links(published: &Value) -> String {
    published.as_array().into_iter().flatten().map(|p| format!(":{} {}", p["port"], p["urls"][0].as_str().unwrap_or(p["kind"].as_str().unwrap_or("")))).collect::<Vec<_>>().join(", ")
}

pub fn ps(environments: &Value, color: bool) -> String {
    let rows: Vec<Vec<String>> = environments.as_array().into_iter().flatten().map(|env| vec![
        text(&env["name"]), yougori_cli::overview::env_type(env).into(), text(&env["status"]),
        percent(&env["cpuUsage"]), gigabytes(&env["memoryUsageGb"]), text(&env["id"]),
    ]).collect();
    let mut output = section("Environments", color);
    if rows.is_empty() { output.push_str("No environments yet. Start one with `yougori run IMAGE`.\n"); }
    else { output.push_str(&table(&["NAME", "TYPE", "STATUS", "CPU", "MEMORY", "ID"], &rows, color)); }
    output
}

pub fn ports(publications: &Value, color: bool) -> String {
    let rows: Vec<Vec<String>> = publications.as_array().into_iter().flatten().map(|row| {
        let p = &row["publication"];
        vec![text(&row["environment"]), text(&p["port"]), if p["domain"] == true { "domain".into() } else { text(&p["kind"]) }, text(&p["status"]), text(&p["id"]), p["urls"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")]
    }).collect();
    let mut output = section("Published ports", color);
    if rows.is_empty() { output.push_str("Nothing is published. Publish a port with `yougori ports publish ENV --port N --kind local|cloudflare`.\n"); }
    else { output.push_str(&table(&["ENVIRONMENT", "PORT", "KIND", "STATUS", "ID", "ADDRESS"], &rows, color)); }
    output
}

pub fn neocloud_prices(report: &Value, color: bool) -> String {
    let hours = report["hours"].as_f64().unwrap_or(1.0);
    let rows: Vec<Vec<String>> = report["ranked"].as_array().into_iter().flatten().map(|quote| {
        let gpu = quote["gpuCount"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "—".into());
        let vram = quote["vramGb"].as_f64().map(|n| format!("{n} GB")).unwrap_or_else(|| "—".into());
        let hourly = quote["hourlyUsd"].as_f64().map(|n| format!("${n:.3}")).unwrap_or_else(|| "—".into());
        let estimate = quote["estimatedComputeUsd"].as_f64().map(|n| format!("${n:.2}")).unwrap_or_else(|| "—".into());
        vec![text(&quote["rank"]), text(&quote["provider"]), text(&quote["name"]), gpu, vram, hourly, estimate, text(&quote["offerId"])]
    }).collect();
    let mut rendered = section("Neocloud prices", color);
    rendered.push_str(&format!("{}\n\n", styled(&format!("{} offers discovered  ·  {} ranked  ·  {hours}h estimate", report["discoveredTotal"].as_u64().unwrap_or(0), report["rankedTotal"].as_u64().unwrap_or(0)), MUTED, color)));
    rendered.push_str(&if rows.is_empty() { "No comparable hourly USD offers were returned.\n".into() }
        else { table(&["#", "PROVIDER", "OFFER", "GPUS", "VRAM/GPU", "USD/HR", "COMPUTE EST.", "OFFER ID"], &rows, color) });
    rendered.push_str(&format!("The {hours}h estimate covers compute only.\n"));
    let unranked = report["unrankedTotal"].as_u64().unwrap_or(0);
    if unranked > 0 { rendered.push_str(&format!("{unranked} offer{} lacked a comparable USD hourly quote; use --format json to inspect {}.\n", if unranked == 1 { "" } else { "s" }, if unranked == 1 { "it" } else { "them" })); }
    let unavailable = report["unavailableTotal"].as_u64().unwrap_or(0);
    if unavailable > 0 { rendered.push_str(&format!("{unavailable} offer{} {} marked unavailable by the provider; use --format json to inspect {}.\n", if unavailable == 1 { "" } else { "s" }, if unavailable == 1 { "was" } else { "were" }, if unavailable == 1 { "it" } else { "them" })); }
    for provider in report["providers"].as_array().into_iter().flatten() {
        let status = provider["status"].as_str().unwrap_or("unknown");
        if status != "ok" {
            let message = provider["message"].as_str().unwrap_or("").replace(['\r', '\n'], " ");
            rendered.push_str(&format!("{}: {} — {}\n", safe(&text(&provider["provider"])), safe(status), safe(&message)));
        }
    }
    rendered.push_str("Confirm the provider checkout price before creating; storage, network, taxes and stopped-resource charges are excluded.\n");
    rendered
}

fn bytes(value: &Value) -> String {
    let Some(v) = value.as_f64() else { return "—".into() };
    let units = ["B", "KB", "MB", "GB", "TB"];
    let exponent = if v < 1.0 { 0 } else { ((v.log(1024.0)).floor() as usize).min(units.len() - 1) };
    if exponent == 0 { format!("{v:.0} B") } else { format!("{:.1} {}", v / 1024f64.powi(exponent as i32), units[exponent]) }
}

pub fn volumes(listing: &Value, color: bool) -> String {
    let rows: Vec<Vec<String>> = listing["volumes"].as_array().into_iter().flatten().map(|volume| {
        let mut users: Vec<String> = volume["mounts"].as_array().into_iter().flatten().map(|m| format!("{}:{}", text(&m["environment"]), text(&m["target"]))).collect();
        for stored in volume["stored"].as_array().into_iter().flatten() {
            for user in stored["usedBy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !users.iter().any(|u| u.starts_with(&format!("{user}:"))) { users.push(user.to_owned()); }
            }
        }
        let stored = volume["stored"].as_array().cloned().unwrap_or_default();
        let size = if stored.iter().any(|s| s["sizeBytes"].is_number()) {
            let total: f64 = stored.iter().filter_map(|s| s["sizeBytes"].as_f64()).sum();
            format!("{}{}", bytes(&total.into()), if stored.iter().any(|s| s["sizeComplete"] == false) { "+" } else { "" })
        } else { "—".into() };
        vec![
            text(&volume["name"]),
            if volume["inUse"] == true { "in use".into() } else { "unused".into() },
            size,
            if stored.is_empty() { "not checked".into() } else { stored.iter().map(|s| text(&s["location"])).collect::<Vec<_>>().join(", ") },
            if users.is_empty() { "—".into() } else { users.join(", ") },
        ]
    }).collect();
    let mut output = section("Volumes", color);
    output.push_str(&if rows.is_empty() { "No volumes found.\n".to_owned() } else { table(&["NAME", "STATE", "SIZE", "STORED IN", "USED BY"], &rows, color) });
    for problem in listing["problems"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        output.push_str(&format!("Not checked: {}\n", safe(problem)));
    }
    if let Some(note) = listing["note"].as_str() {
        output.push_str(&format!("{}\n", safe(note)));
    }
    output
}

pub fn top(snapshot: &Value, color: bool) -> String {
    let host = &snapshot["host"];
    let mut output = section("Live usage", color);
    output.push_str(&format!("{}  CPU {} / {} cores    Memory {} / {}    GPU {}\n\n",
        styled("THIS PC", ACCENT, color), percent(&host["cpuPercent"]), safe(&text(&host["cpus"])), gigabytes(&host["memoryGb"]), gigabytes(&host["totalMemoryGb"]), percent(&host["gpuPercent"])));
    let rows: Vec<Vec<String>> = snapshot["environments"].as_array().into_iter().flatten().map(|env| vec![
        text(&env["name"]), text(&env["type"]), percent(&env["cpuPercent"]), gigabytes(&env["memoryGb"]),
        format!("+{}", gigabytes(&env["storageAddedGb"])), env["networkMbps"].as_f64().map(|v| format!("{v:.1} Mb/s")).unwrap_or_else(|| "—".into()),
    ]).collect();
    if rows.is_empty() { output.push_str("No environments are running.
"); }
    else { output.push_str(&table(&["NAME", "TYPE", "CPU", "MEMORY", "STORAGE", "NETWORK"], &rows, color)); }
    output
}

pub fn doctor(report: &Value, color: bool) -> String {
    let rows: Vec<Vec<String>> = report["checks"].as_array().into_iter().flatten().map(|c| {
        let status = match c["status"].as_str() { Some("ok") => "OK", Some("warning") => "WARN", Some("error") => "FAIL", _ => "INFO" };
        let detail = match c["fix"].as_str() { Some(fix) => format!("{} → {fix}", text(&c["detail"])), None => text(&c["detail"]) };
        vec![status.into(), text(&c["name"]), detail]
    }).collect();
    let verdict = if report["ready"] == true { "Ready" } else { "Needs attention — fix the FAIL checks first" };
    let verdict_color = if report["ready"] == true { GOOD } else { BAD };
    format!("{}{}\n{}\n", section("System check", color), table(&["", "CHECK", "DETAIL"], &rows, color), styled(verdict, verdict_color, color))
}

pub fn status(overview: &Value, color: bool) -> String {
    if overview["engine"]["running"] != true {
        return format!("{}{}\n{}\n", section("Yougori status", color), styled("● Engine is offline", BAD, color), safe(&text(&overview["hint"])));
    }
    let envs = &overview["environments"];
    let types = envs["byType"].as_object().map(|t| t.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let sharing = &overview["sharing"];
    let domains = overview["savedDomains"].as_array().map(|d| d.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).filter(|d| !d.is_empty()).unwrap_or_else(|| "none".into());
    let mut output = format!(
        "{}{}  Engine {}\n{} environments  ·  {} running  ·  {} connections  ·  {} active jobs\n",
        section("Yougori status", color), styled("● Online", GOOD, color), safe(&text(&overview["engine"]["version"])), envs["total"], envs["running"], overview["connections"], overview["activeJobs"],
    );
    if !types.is_empty() { output.push_str(&format!("{}\n", styled(&safe(&types), MUTED, color))); }
    output.push_str(&format!("Sharing: {}  ·  Saved domains: {}\n\n",
        if sharing["link"].is_string() { format!("link on, {} people", sharing["people"]) } else { "link off".into() }, safe(&domains)));
    let rows: Vec<Vec<String>> = envs["list"].as_array().into_iter().flatten().map(|env| vec![
        text(&env["name"]), text(&env["type"]), text(&env["status"]), percent(&env["cpuPercent"]), gigabytes(&env["memoryGb"]), links(&env["published"]),
    ]).collect();
    if rows.is_empty() {
        output.push_str("No environments yet. Create one with `yougori run IMAGE`.\n");
    } else {
        output.push_str(&section("Environments", color));
        output.push_str(&table(&["NAME", "TYPE", "STATUS", "CPU", "MEMORY", "PUBLISHED"], &rows, color));
    }
    output
}
