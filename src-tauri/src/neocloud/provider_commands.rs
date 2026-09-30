use super::{token, CreateRequest, Deployment};
use serde_json::Value;

pub(super) fn e2e_context(location: &str) -> Result<(&str, &str), String> {
    let (project, region) = location
        .split_once(':')
        .ok_or("E2E requires project:location, for example 123:Delhi")?;
    if project.parse::<u64>().is_err() || !matches!(region, "Delhi" | "Mumbai") {
        return Err("Choose an E2E numeric project ID and Delhi or Mumbai".into());
    }
    Ok((project, region))
}

pub(super) fn cpu_size(offer: &str) -> Option<(u32, u32)> {
    let (cpu, ram) = offer.split_once(':')?;
    let (cpu, ram) = (cpu.parse::<u32>().ok()?, ram.parse::<u32>().ok()?);
    ((1..=1024).contains(&cpu) && (1..=8192).contains(&ram)).then_some((cpu, ram))
}

pub(super) fn create(request: &CreateRequest) -> Option<Vec<String>> {
    let p = request;
    let disk = p.disk_gb.to_string();
    let cpus = p.cpu_cores.to_string();
    let memory = p.memory_gb.to_string();
    let gpus = p.gpu_count.to_string();
    let args: Vec<&str> = match p.provider.as_str() {
        "prime" => vec![
            "pods",
            "create",
            "--id",
            &p.offer,
            "--name",
            &p.name,
            "--image",
            &p.image,
            "--disk-size",
            &disk,
            "--vcpus",
            &cpus,
            "--memory",
            &memory,
            "--yes",
        ],
        "thunder" => vec![
            "create",
            "--gpu",
            &p.offer,
            "--num-gpus",
            &gpus,
            "--vcpus",
            &cpus,
            "--template",
            &p.image,
            "--disk",
            &disk,
            "--json",
        ],
        "latitude" => vec![
            "servers",
            "create",
            "--hostname",
            &p.name,
            "--plan",
            &p.offer,
            "--site",
            &p.location,
            "--project",
            &p.platform,
            "--operating_system",
            &p.image,
            "--ssh_keys",
            &p.subnet_id,
            "-o",
            "json",
        ],
        "e2e" => {
            let (project, region) = e2e_context(&p.location).expect("validated E2E context");
            vec![
                "--project_id",
                project,
                "--location",
                region,
                "node",
                "create",
                "--name",
                &p.name,
                "--plan",
                &p.offer,
                "--image",
                &p.image,
                "--region",
                if region == "Delhi" { "ncr" } else { "mumbai" },
                "--security_group_id",
                &p.firewall,
                "--ssh_keys",
                &p.ssh_public_key,
            ]
        }
        _ => return None,
    };
    Some(args.into_iter().map(str::to_owned).collect())
}

pub(super) fn action(p: &Deployment, action: &str) -> Option<Result<Vec<String>, String>> {
    let id = p.resource_id.as_str();
    let args = match (p.provider.as_str(), action) {
        ("prime" | "thunder", "start" | "stop") => return Some(Err("This provider supports termination, not reversible stop/start. Export data and use Delete provider resource when finished.".into())),
        ("prime", "inspect") => vec!["pods", "status", id, "--output", "json"],
        ("prime", "delete") => vec!["pods", "terminate", id, "--yes"],
        ("thunder", "inspect") => vec!["status", "--json"],
        ("thunder", "delete") => vec!["delete", id, "--json", "--yes"],
        ("latitude", "inspect") => vec!["servers", "get", "--id", id, "-o", "json"],
        ("latitude", "delete") => vec!["servers", "destroy", "--id", id, "-o", "json"],
        ("latitude", "start") => vec!["servers", "power-on", id, "-o", "json"],
        ("latitude", "stop") => vec!["servers", "power-off", id, "-o", "json"],
        ("e2e", "inspect" | "delete" | "start" | "stop") => {
            let (project, region) = match e2e_context(&p.location) { Ok(v) => v, Err(e) => return Some(Err(e)) };
            let mut args = vec!["--project_id", project, "--location", region, "node"];
            match action {
                "start" => args.extend(["--action", "power_on"]),
                "stop" => args.extend(["--action", "power_off"]),
                "inspect" => args.push("get"), _ => args.push("delete"),
            }
            args.extend(["--node_id", id]); args
        }
        _ => return None,
    };
    Some(Ok(args.into_iter().map(str::to_owned).collect()))
}

pub(super) fn id(provider: &str, value: &Value) -> Option<String> {
    let value = if provider == "latitude" {
        value.as_array().and_then(|a| a.first()).unwrap_or(value)
    } else {
        value
    };
    if provider == "prime" {
        if let Some(output) = value.as_str() {
            let candidate = output
                .lines()
                .find_map(|line| line.trim().strip_prefix("Successfully created pod "))?
                .trim();
            return token(candidate, 128).then(|| candidate.into());
        }
    }
    let field = if provider == "thunder" {
        &value["identifier"]
    } else {
        value.get("data").unwrap_or(value).get("id")?
    };
    field
        .as_str()
        .map(str::to_owned)
        .or_else(|| field.as_u64().map(|id| id.to_string()))
}

pub(super) fn resource(provider: &str, id: &str, value: Value) -> Result<Value, String> {
    if provider == "thunder" {
        let rows = value
            .as_array()
            .ok_or("Thunder did not return a structured instance list")?;
        return rows
            .iter()
            .find(|v| v["id"].as_str() == Some(id))
            .cloned()
            .ok_or("Provider resource not found".into());
    }
    if provider == "latitude" {
        let row = value.as_array().and_then(|a| a.first()).unwrap_or(&value);
        let row = row.get("data").unwrap_or(row);
        return row
            .get("attributes")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or("Latitude did not return a server".into());
    }
    if provider == "e2e" {
        return value
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or("E2E did not return a node".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn parse_provider_ids_and_do_not_treat_unknown_text_as_success() {
        assert_eq!(
            id("prime", &json!("Successfully created pod pod123\n")),
            Some("pod123".into())
        );
        assert!(id("prime", &json!("Failed to create pod")).is_none());
        assert_eq!(id("thunder", &json!({"identifier":0})), Some("0".into()));
        assert_eq!(id("e2e", &json!({"data":{"id":12}})), Some("12".into()));
        assert!(resource("thunder", "1", json!([])).is_err());
        assert_eq!(cpu_size("8:32"), Some((8, 32)));
        assert_eq!(cpu_size("0:32"), None);
        assert!(e2e_context("--bad:Delhi").is_err());
    }
}
