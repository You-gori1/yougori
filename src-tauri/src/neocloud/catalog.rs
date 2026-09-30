use super::{pricing, program, run_with_timeout, token};
use futures_util::future::join_all;
use serde_json::{json, Value};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

pub(super) async fn discover_raw(provider: &str, location: Option<&str>) -> Result<Value, String> {
    program(provider)?;
    if location.is_some_and(|s| !s.is_empty() && !token(s, 128)) {
        return Err("Invalid region or project ID".into());
    }
    let mut queries: Vec<(&str, Vec<String>)> = match provider {
        "runpod" => vec![
            ("offers", strings(&["gpu", "list", "--include-unavailable"])),
            ("locations", strings(&["datacenter", "list"])),
            ("images", strings(&["template", "search", "pytorch"])),
        ],
        "vast" => vec![(
            "offers",
            strings(&[
                "search",
                "offers",
                "verified=true direct_port_count>=1",
                "--raw",
            ]),
        )],
        "jarvis" => vec![
            ("offers", strings(&["gpus", "--json"])),
            ("cpus", strings(&["cpus", "--json"])),
            ("images", strings(&["templates", "--json"])),
            ("account", strings(&["status", "--json"])),
        ],
        "crusoe" => vec![
            ("offers", strings(&["compute", "vms", "types", "--json"])),
            ("locations", strings(&["locations", "list", "--json"])),
            ("images", strings(&["compute", "images", "list", "--json"])),
        ],
        "prime" => vec![(
            "offers",
            strings(&[
                "availability",
                "list",
                "--no-group-similar",
                "--output",
                "json",
            ]),
        )],
        "latitude" => vec![
            (
                "offers",
                strings(&["plans", "stock", "--gpu", "-o", "json"]),
            ),
            ("cpus", strings(&["plans", "stock", "-o", "json"])),
            ("projects", strings(&["projects", "list", "-o", "json"])),
            ("sshKeys", strings(&["ssh-keys", "list", "-o", "json"])),
        ],
        "thunder" => vec![("instances", strings(&["status", "--json"]))],
        "e2e" => vec![(
            "instances",
            super::accounts::check_args(provider, location)?,
        )],
        "civo" => vec![
            ("locations", strings(&["region", "ls", "--output", "json"])),
            ("sshKeys", strings(&["sshkey", "list", "--output", "json"])),
        ],
        "nebius" => vec![],
        _ => unreachable!(),
    };
    let location = location.filter(|s| !s.is_empty());
    if provider == "civo" {
        if let Some(region) = location {
            for (section, command) in [
                ("offers", "size"),
                ("images", "diskimage"),
                ("firewalls", "firewall"),
            ] {
                queries.push((
                    section,
                    strings(&[command, "ls", "--region", region, "--output", "json"]),
                ));
            }
        }
    }
    if provider == "nebius" {
        if let Some(project) = location {
            for (section, service, resource) in [
                ("platforms", "compute", "platform"),
                ("images", "compute", "image"),
                ("subnets", "vpc", "subnet"),
            ] {
                queries.push((
                    section,
                    strings(&[
                        service,
                        resource,
                        "list",
                        "--parent-id",
                        project,
                        "--all",
                        "--format",
                        "json",
                    ]),
                ));
            }
        }
    }
    let mut result = json!({"issues":[]});
    for (section, outcome) in join_all(queries.into_iter().map(|(section, args)| async move {
        (section, run_with_timeout(provider, &args, 35).await)
    }))
    .await
    {
        match outcome {
            Ok(value) if value.is_object() || value.is_array() => result[section] = value,
            Ok(_) => result["issues"].as_array_mut().unwrap().push(json!(format!("{section}: the CLI returned text instead of structured data. Update the provider CLI."))),
            Err(error) => result["issues"].as_array_mut().unwrap().push(json!(format!("{section}: {error}"))),
        }
    }
    if location.is_none() && matches!(provider, "nebius" | "civo") {
        result["issues"]
            .as_array_mut()
            .unwrap()
            .push(json!(if provider == "nebius" {
                "Select a project to load platforms, presets, images and subnets."
            } else {
                "Select a region to load CPU and GPU sizes, images and firewalls."
            }));
    }
    if matches!(provider, "thunder" | "e2e") {
        result["issues"].as_array_mut().unwrap().push(json!("This provider CLI does not expose a structured hardware/price catalog. Use its pricing page to select a configuration; account and lifecycle operations use the CLI."));
    }
    Ok(result)
}

pub(super) fn rows(value: &Value) -> Vec<&Value> {
    if let Some(rows) = value.as_array() {
        return rows.iter().collect();
    }
    for key in [
        "items",
        "data",
        "results",
        "regions",
        "templates",
        "projects",
        "platforms",
    ] {
        if let Some(child) = value.get(key) {
            let rows = rows(child);
            if !rows.is_empty() {
                return rows;
            }
        }
    }
    vec![]
}

fn choices(value: &Value, provider: &str, section: &str) -> Vec<Value> {
    rows(value).into_iter().filter_map(|v| {
        if let Some(id) = v.as_str() { return Some(json!({"id":id,"name":id})); }
        let row = v.get("attributes").unwrap_or(v);
        let metadata = row.get("metadata").unwrap_or(row);
        let id = if provider == "nebius" && section == "platforms" { row.get("name").or(metadata.get("name")) }
            else if section == "locations" { row.get("code").or(row.get("id")).or(row.get("name")) }
            else if provider == "civo" { row.get("name").or(row.get("id")) }
            else if provider == "runpod" && section == "images" { row.get("imageName") }
            else { v.get("id").or(metadata.get("id")).or(row.get("id")).or(row.get("name")).or(row.get("template_name")) }?;
        let id = id.as_str().map(str::to_owned).or_else(|| id.as_u64().map(|n| n.to_string()))?;
        Some(json!({"id":id,"name":metadata.get("name").or(row.get("displayName")).or(row.get("name")).and_then(Value::as_str).unwrap_or(&id)}))
    }).collect()
}

#[tauri::command]
pub async fn neocloud_catalog(
    provider: String,
    product: String,
    location: Option<String>,
) -> Result<Value, String> {
    if !matches!(product.as_str(), "cpu" | "gpu") {
        return Err("Choose CPU or GPU compute".into());
    }
    let mut raw = discover_raw(&provider, location.as_deref()).await?;
    if provider == "runpod" && product == "cpu" {
        raw["issues"].as_array_mut().unwrap().push(json!("The RunPod CLI does not expose a CPU pod catalogue. Review the provider-selected CPU configuration and price in your account."));
    }
    let offers = pricing::normalize(&provider, &product, location.as_deref(), &raw);
    let mut options = json!({});
    for section in [
        "locations",
        "images",
        "sshKeys",
        "firewalls",
        "platforms",
        "subnets",
        "projects",
    ] {
        options[section] = json!(choices(&raw[section], &provider, section));
    }
    Ok(
        json!({"provider":provider,"product":product,"checkedAt":chrono::Utc::now().to_rfc3339(),"offers":offers,"choices":options,"issues":raw["issues"],"source":"provider CLI"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choices_keep_resource_ids_and_do_not_pick_an_unrelated_nested_array() {
        assert_eq!(
            choices(
                &json!({"items":[{"metadata":{"id":"img-1","name":"Ubuntu"}}]}),
                "nebius",
                "images"
            )[0]["id"],
            "img-1"
        );
        assert_eq!(
            choices(
                &json!([{ "id":"uuid", "name":"g4s.small" }]),
                "civo",
                "images"
            )[0]["id"],
            "g4s.small"
        );
        assert!(choices(&json!({"credentials":["secret"]}), "civo", "images").is_empty());
    }
}
