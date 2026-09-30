use super::{neocloud_discover, token};
use futures_util::future::join_all;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Quote {
    rank: Option<usize>,
    provider: String,
    product: String,
    offer_id: String,
    name: String,
    location: Option<String>,
    hourly_price: Option<f64>,
    currency: Option<String>,
    hourly_usd: Option<f64>,
    estimated_compute_usd: Option<f64>,
    gpu_count: Option<u32>,
    vram_gb: Option<f64>,
    available: Option<bool>,
    price_field: Option<&'static str>,
    cpu_cores: Option<f64>,
    memory_gb: Option<f64>,
    storage_gb: Option<f64>,
    platform: Option<String>,
    monthly_price: Option<f64>,
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn first_number(row: &Value, fields: &[&'static str]) -> Option<(f64, &'static str)> {
    fields
        .iter()
        .find_map(|&field| number(&row[field]).map(|value| (value, field)))
}

fn first_string(row: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        row[*field]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| row[*field].as_u64().map(|n| n.to_string()))
    })
}

fn rows(value: &Value) -> Vec<&Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    for key in [
        "items",
        "data",
        "gpus",
        "gpuTypes",
        "types",
        "vm_types",
        "sizes",
        "results",
        "gpu_resources",
        "combinations",
        "platforms",
    ] {
        if let Some(child) = value.get(key) {
            let found = rows(child);
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

fn gpu_row(provider: &str, row: &Value) -> bool {
    if provider == "runpod" || provider == "vast" || provider == "jarvis" {
        return true;
    }
    if row["gpu_count"].as_u64().is_some_and(|count| count > 0)
        || row["gpuCount"].as_u64().is_some_and(|count| count > 0)
    {
        return true;
    }
    let name = first_string(row, &["name", "type", "size", "id"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "gpu", "h100", "h200", "a100", "b200", "b300", "l40s", "mi300", "mi355", "gb200",
    ]
    .iter()
    .any(|part| name.contains(part))
}

fn vram_gb(provider: &str, row: &Value) -> Option<f64> {
    if provider == "vast" {
        return number(&row["gpu_ram"]).map(|mb| mb / 1000.0);
    }
    if let Some((value, _)) = first_number(
        row,
        &[
            "memoryInGb",
            "memory_in_gb",
            "vramGb",
            "vram_gb",
            "vram",
            "gpu_memory",
        ],
    ) {
        return Some(value);
    }
    let text = first_string(row, &["vram", "gpu_memory"])?;
    let compact = text.trim().to_ascii_lowercase().replace(' ', "");
    let number = compact.trim_end_matches("gib").trim_end_matches("gb");
    if number == compact {
        return None;
    }
    number
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n > 0.0)
}

fn price(provider: &str, row: &Value) -> (Option<f64>, Option<String>, Option<&'static str>) {
    let fields: &[&'static str] = match provider {
        // RunPod's CLI default creates Secure Cloud pods; do not rank its
        // cheaper Community price as if the current create command used it.
        "runpod" => &["securePricePerHr"],
        "vast" => &["dph_total"],
        "jarvis" => &["price_per_hour"],
        "prime" => &["price_value"],
        _ => &[
            "hourly_price",
            "price_per_hour",
            "hourlyPrice",
            "pricePerHour",
            "hourlyUsd",
        ],
    };
    let Some((amount, field)) = first_number(row, fields).filter(|(amount, _)| *amount > 0.0)
    else {
        return (None, None, None);
    };
    let currency = match provider {
        "runpod" | "vast" | "prime" | "latitude" => Some("USD".to_owned()),
        _ if field == "hourlyUsd" => Some("USD".to_owned()),
        _ => first_string(row, &["currency", "currencyCode", "currency_code"])
            .map(|value| value.to_ascii_uppercase()),
    };
    (Some(amount), currency, Some(field))
}

pub(super) fn normalize(
    provider: &str,
    product: &str,
    location: Option<&str>,
    response: &Value,
) -> Vec<Quote> {
    let source = if provider == "jarvis" && product == "cpu" {
        &response["cpus"]
    } else if provider == "nebius" {
        &response["platforms"]
    } else if provider == "latitude" && product == "cpu" {
        &response["cpus"]
    } else if let Some(offers) = response.get("offers") {
        offers
    } else {
        response
    };
    let mut expanded = Vec::new();
    for original in rows(source) {
        if provider == "runpod"
            && original["dataCenterAvailability"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        {
            for stock in original["dataCenterAvailability"].as_array().unwrap() {
                let mut row = original.clone();
                row["dataCenterId"] = stock["dataCenterId"].clone();
                row["stockStatus"] = stock["stockStatus"].clone();
                row.as_object_mut().unwrap().remove("available");
                expanded.push(row);
            }
        } else if provider == "nebius" {
            let platform = first_string(original, &["name", "id"])
                .or_else(|| first_string(&original["metadata"], &["name", "id"]));
            let presets = original
                .get("presets")
                .or_else(|| original["spec"].get("presets"));
            for preset in presets.and_then(Value::as_array).into_iter().flatten() {
                let mut row = preset.clone();
                row["platform"] = json!(platform);
                let resources = preset.get("resources").unwrap_or(preset);
                row["gpu_count"] = resources["gpu_count"].clone();
                row["vcpus"] = resources["vcpu_count"].clone();
                row["memory_gb"] = resources["memory_gibibytes"].clone();
                row["vram_gb"] = original["spec"]["gpu_memory_gigabytes"].clone();
                if row["vram_gb"].is_null() {
                    row["vram_gb"] = original["spec"]["gpu_memory_gibibytes"].clone();
                }
                expanded.push(row);
            }
        } else if provider == "jarvis" && product == "cpu" {
            if let Some(regions) = original["regions"].as_object() {
                for (region, stock) in regions {
                    let mut row = original.clone();
                    row["id"] = json!(format!("{}:{}", original["vcpus"], original["ram_gb"]));
                    row["name"] = json!(format!(
                        "{} vCPU / {} GB RAM",
                        original["vcpus"], original["ram_gb"]
                    ));
                    row["region"] = json!(region);
                    row["available"] = stock.clone();
                    expanded.push(row);
                }
            } else {
                expanded.push(original.clone());
            }
        } else {
            expanded.push(original.clone());
        }
    }
    expanded
        .iter()
        .filter_map(|row| {
            if !row.is_object() {
                return None;
            }
            let is_gpu = if provider == "latitude" {
                first_string(row, &["plan_slug", "slug"]).is_some_and(|s| s.starts_with('g'))
            } else if provider == "jarvis" {
                product == "gpu"
            } else {
                gpu_row(provider, row)
            };
            if (product == "gpu") != is_gpu {
                return None;
            }
            let offer_id = first_string(
                row,
                match provider {
                    "civo" => &["name", "size", "id"],
                    "jarvis" => &["id", "gpu_type", "type", "name"],
                    "latitude" => &["plan_slug", "slug", "id"],
                    _ => &["id", "gpu_id", "gpuId", "gpu_type", "type", "name"],
                },
            )?;
            let name = first_string(
                row,
                &[
                    "displayName",
                    "display_name",
                    "gpu_name",
                    "gpu_type",
                    "name",
                    "type",
                    "plan_slug",
                ],
            )
            .unwrap_or_else(|| offer_id.clone());
            let (hourly_price, mut currency, price_field) = price(provider, row);
            if provider == "latitude" && number(&row["monthly_usd"]).is_some() {
                currency = Some("USD".into());
            }
            if provider == "jarvis" {
                currency = first_string(&response["account"], &["currency"])
                    .or(currency)
                    .map(|s| s.to_uppercase());
            }
            let hourly_usd = (currency.as_deref() == Some("USD"))
                .then_some(hourly_price)
                .flatten();
            let available = if provider == "runpod" && row["secureCloud"] == false {
                Some(false)
            } else {
                row["available"]
                    .as_bool()
                    .or_else(|| row["rentable"].as_bool())
                    .or_else(|| {
                        row["effective_num_free_devices"]
                            .as_u64()
                            .or(row["num_free_devices"].as_u64())
                            .map(|n| n > 0)
                    })
                    .or_else(|| {
                        first_string(row, &["stockStatus", "stock_status", "stock_level"]).and_then(
                            |s| match s.to_ascii_lowercase().as_str() {
                                "unavailable" | "none" | "out_of_stock" | "out of stock" => {
                                    Some(false)
                                }
                                "available" | "in_stock" | "in stock" | "low" | "medium"
                                | "high" | "unique" => Some(true),
                                _ => None,
                            },
                        )
                    })
            };
            let region = first_string(row, &["location", "region", "geolocation", "dataCenterId"])
                .or_else(|| {
                    matches!(provider, "civo" | "nebius")
                        .then(|| location.map(str::to_owned))
                        .flatten()
                });
            let cpu_cores = first_number(
                row,
                &[
                    "vcpus",
                    "cpu_cores",
                    "cpuCores",
                    "cpu_count",
                    "cpus_per_gpu",
                ],
            )
            .map(|(n, _)| n);
            let memory_gb = first_number(
                row,
                &["memory_gb", "ram_gb", "memory_total_gb", "ram_per_gpu"],
            )
            .map(|(n, _)| n)
            .or_else(|| {
                if provider == "civo" {
                    number(&row["ram_mb"])
                        .or_else(|| number(&row["ram"]))
                        .map(|mb| mb / 1024.0)
                } else if provider == "vast" {
                    number(&row["cpu_ram"]).map(|mb| mb / 1000.0)
                } else {
                    None
                }
            });
            Some(Quote {
                rank: None,
                provider: provider.into(),
                product: product.into(),
                offer_id,
                name,
                location: region,
                hourly_price,
                currency,
                hourly_usd,
                estimated_compute_usd: None,
                gpu_count: (product == "gpu")
                    .then(|| {
                        first_number(row, &["num_gpus", "gpu_count", "gpuCount"])
                            .map(|(n, _)| n as u32)
                            .filter(|n| *n > 0)
                            .or_else(|| matches!(provider, "runpod" | "jarvis").then_some(1))
                    })
                    .flatten(),
                vram_gb: (product == "gpu").then(|| vram_gb(provider, row)).flatten(),
                available,
                price_field,
                cpu_cores,
                memory_gb,
                storage_gb: first_number(
                    row,
                    &["disk_gb", "disk_space", "disk_gb_available", "disk_size"],
                )
                .map(|(n, _)| n),
                platform: first_string(row, &["platform"]),
                monthly_price: first_number(row, &["monthly_usd", "monthly_price"]).map(|(n, _)| n),
            })
        })
        .collect()
}

pub(super) fn runpod_secure_quote(
    inventory: &Value,
    gpu_id: &str,
    data_center: &str,
    approved_hourly_usd: f64,
) -> Result<f64, String> {
    let quote = normalize("runpod", "gpu", None, inventory).into_iter()
        .find(|quote| quote.offer_id == gpu_id && quote.location.as_deref() == Some(data_center))
        .ok_or("The selected RunPod GPU is no longer listed at that data center. Refresh the catalog before creating.")?;
    if quote.available != Some(true) {
        return Err("The selected RunPod GPU is no longer in stock at that data center. Refresh the catalog before creating.".into());
    }
    let price = quote.hourly_usd.filter(|price| *price > 0.0)
        .ok_or("RunPod no longer reports a Secure Cloud price for this GPU. Refresh the catalog before creating.")?;
    if price > approved_hourly_usd + 0.000_001 {
        return Err(format!("RunPod's price rose to USD {price:.4}/hr, above the approved USD {approved_hourly_usd:.4}/hr. Refresh the catalog and review the new price before creating."));
    }
    Ok(price)
}

fn accounts_product_supported(id: &str, product: &str) -> bool {
    super::accounts::metadata()
        .iter()
        .find(|p| p["id"] == id)
        .and_then(|p| p["products"].as_array())
        .is_some_and(|p| p.iter().any(|p| p == product))
}

pub(super) async fn compare(
    provider: Option<String>,
    product: Option<String>,
    offer: Option<String>,
    location: Option<String>,
    hours: Option<f64>,
    max_hourly: Option<f64>,
    min_vram_gb: Option<f64>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let product = product.unwrap_or_else(|| "gpu".into());
    if !matches!(product.as_str(), "gpu" | "cpu") {
        return Err("Choose --product gpu or cpu".into());
    }
    let hours = hours.unwrap_or(1.0);
    if !hours.is_finite() || !(0.01..=8760.0).contains(&hours) {
        return Err("--hours must be between 0.01 and 8760".into());
    }
    if max_hourly.is_some_and(|n| !n.is_finite() || n <= 0.0 || n > 1_000_000.0) {
        return Err("--max-hourly must be a positive USD amount".into());
    }
    if min_vram_gb.is_some_and(|n| product != "gpu" || !n.is_finite() || n <= 0.0 || n > 1024.0) {
        return Err("--min-vram-gb requires GPU and a value from 0 to 1024".into());
    }
    let limit = limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return Err("--limit must be between 1 and 100".into());
    }
    let location = location
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if location.as_deref().is_some_and(|value| !token(value, 128)) {
        return Err("--location must be a provider region or project ID".into());
    }
    let offer = offer
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if offer
        .as_deref()
        .is_some_and(|value| value.len() > 128 || value.chars().any(char::is_control))
    {
        return Err("--offer must be a provider offer ID of at most 128 characters".into());
    }
    let provider = provider.filter(|id| id != "all");
    let providers = super::accounts::metadata();
    let selected: Vec<_> = providers
        .iter()
        .filter(|info| provider.as_ref().is_none_or(|id| info["id"] == *id))
        .collect();
    if selected.is_empty() {
        return Err("Unknown Neocloud provider. Run `yougori neocloud providers`".into());
    }
    let mut statuses = Vec::new();
    for info in &selected {
        if !accounts_product_supported(info["id"].as_str().unwrap(), &product) {
            statuses.push(json!({"provider":info["id"].as_str().unwrap(),"status":"unsupported","message":"This provider does not support the selected compute category"}));
        }
    }
    let checks = selected
        .into_iter()
        .filter(|info| accounts_product_supported(info["id"].as_str().unwrap(), &product))
        .map(|info| {
            let location = location.clone();
            async move {
                let result = tokio::time::timeout(
                    Duration::from_secs(60),
                    neocloud_discover(info["id"].as_str().unwrap().into(), location.clone()),
                )
                .await;
                (info["id"].as_str().unwrap(), location, result)
            }
        });
    let mut offers = Vec::new();
    for (id, location, outcome) in join_all(checks).await {
        match outcome {
            Ok(Ok(response)) => {
                let quotes = normalize(id, &product, location.as_deref(), &response);
                let priced = quotes.iter().filter(|q| q.hourly_usd.is_some()).count();
                let status = if response["issues"].as_array().is_some_and(|a| !a.is_empty()) { "partial" } else if response.is_string() { "unstructured" }
                    else if id == "civo" && location.is_none() { "needs_location" }
                    else if quotes.is_empty() { "no_offers" }
                    else if priced == 0 { "no_comparable_price" }
                    else { "ok" };
                let message = match status {
                    "partial" => "Some provider lookups failed; see the provider catalog for details",
                    "unstructured" => "Provider CLI returned text rather than structured offers; no price was inferred",
                    "needs_location" => "Pass --location with a Civo region to request VM sizes",
                    "no_offers" if id == "nebius" => "Platform discovery does not include priced presets; no price was inferred",
                    "no_offers" => "No matching offer rows were returned for this compute type",
                    "no_comparable_price" => "Offers have no explicit hourly USD price; they were left unranked",
                    _ => "Live provider CLI offers",
                };
                statuses.push(json!({"provider":id,"status":status,"message":message,"offers":quotes.len(),"pricedUsd":priced,"issues":response["issues"]}));
                offers.extend(quotes);
            }
            Ok(Err(error)) => statuses.push(json!({"provider":id,"status":"error","message":error.chars().take(500).collect::<String>()})),
            Err(_) => statuses.push(json!({"provider":id,"status":"timeout","message":"Provider discovery exceeded 60 seconds; no price was inferred"})),
        }
    }
    statuses.sort_by(|a, b| a["provider"].as_str().cmp(&b["provider"].as_str()));
    let total_offers = offers.len();
    let mut ranked = Vec::new();
    let mut unranked = Vec::new();
    let mut unavailable = Vec::new();
    let mut filtered_out = 0;
    for mut quote in offers {
        if offer.as_ref().is_some_and(|id| id != &quote.offer_id) {
            filtered_out += 1;
            continue;
        }
        if quote.available == Some(false) {
            unavailable.push(quote);
            continue;
        }
        if min_vram_gb.is_some_and(|minimum| quote.vram_gb.is_none_or(|gb| gb < minimum)) {
            filtered_out += 1;
            continue;
        }
        if let Some(hourly) = quote.hourly_usd {
            if max_hourly.is_some_and(|maximum| hourly > maximum) {
                filtered_out += 1;
                continue;
            }
            quote.estimated_compute_usd = Some(hourly * hours);
            ranked.push(quote);
        } else {
            unranked.push(quote);
        }
    }
    ranked.sort_by(|a, b| {
        a.hourly_usd
            .unwrap()
            .total_cmp(&b.hourly_usd.unwrap())
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.offer_id.cmp(&b.offer_id))
    });
    let ranked_total = ranked.len();
    let unranked_total = unranked.len();
    let unavailable_total = unavailable.len();
    let partial = statuses.iter().any(|status| status["status"] != "ok");
    for (index, quote) in ranked.iter_mut().enumerate() {
        quote.rank = Some(index + 1);
    }
    ranked.truncate(limit);
    unranked.truncate(limit);
    unavailable.truncate(limit);
    Ok(json!({
        "checkedAt": chrono::Utc::now().to_rfc3339(),
        "product": product, "hours": hours, "currency": "USD", "requestedLocation": location,
        "requestedOffer": offer,
        "ranked": ranked, "rankedTotal": ranked_total, "unranked": unranked,
        "unrankedTotal": unranked_total, "unavailable": unavailable,
        "unavailableTotal": unavailable_total,
        "discoveredTotal": total_offers, "filteredOut": filtered_out, "providers": statuses,
        "complete": !partial && ranked_total > 0,
        "partial": partial,
        "hasComparableQuotes": ranked_total > 0,
        "exitCode": if ranked_total > 0 { 0 } else { 2 },
        "note": "Prices come from live provider CLI responses, not guaranteed exhaustive. Rankings include only explicit USD hourly compute prices. Estimates exclude storage, network, IPs, taxes, discounts, minimums and stopped-resource charges. Confirm the provider checkout price before creating a resource. --location is provider discovery context (Civo region or Nebius project), not a universal geographic filter."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_combinations_nebius_presets_and_prime_capacity_remain_selectable() {
        let jarvis = normalize(
            "jarvis",
            "cpu",
            None,
            &json!({"account":{"currency":"INR"},"cpus":{"combinations":[{"vcpus":8,"ram_gb":32,"price_per_hour":12,"regions":{"IN2":true,"EU1":false}}]}}),
        );
        assert_eq!(jarvis.len(), 2);
        assert!(jarvis.iter().all(|q| q.offer_id == "8:32"
            && q.hourly_usd.is_none()
            && q.gpu_count.is_none()
            && q.memory_gb == Some(32.0)));
        assert_eq!(
            jarvis.iter().filter(|q| q.available == Some(true)).count(),
            1
        );
        let presets = json!({"platforms":{"items":[{"metadata":{"name":"gpu-h100"},"spec":{"gpu_memory_gigabytes":80,"presets":[{"name":"1gpu-16vcpu-200gb","resources":{"gpu_count":1,"vcpu_count":16,"memory_gibibytes":200}}]}}]}});
        let nebius = normalize("nebius", "gpu", Some("project-1"), &presets);
        assert_eq!(nebius.len(), 1);
        assert_eq!(nebius[0].platform.as_deref(), Some("gpu-h100"));
        assert_eq!(nebius[0].cpu_cores, Some(16.0));
        assert_eq!(nebius[0].vram_gb, Some(80.0));
        assert!(nebius[0].hourly_price.is_none());
        let prime = normalize(
            "prime",
            "gpu",
            None,
            &json!({"offers":{"gpu_resources":[{"id":"short-id","gpu_type":"H100","gpu_count":2,"price_value":4.0,"gpu_memory":80,"vcpus":"16","memory_gb":"128","disk_gb":"200","stock_status":"High"}]}}),
        );
        assert_eq!(prime[0].hourly_usd, Some(4.0));
        assert_eq!(prime[0].gpu_count, Some(2));
        assert_eq!(prime[0].storage_gb, Some(200.0));
    }

    #[test]
    fn latitude_default_stock_includes_gpus_but_cpu_category_does_not() {
        let response = json!({"cpus":[{"plan_slug":"c3-small","monthly_usd":"100","location":"NYC","stock_level":"High"},{"plan_slug":"g3-h100","monthly_usd":"999","location":"NYC"}]});
        let cpu = normalize("latitude", "cpu", None, &response);
        assert_eq!(cpu.len(), 1);
        assert_eq!(cpu[0].monthly_price, Some(100.0));
        assert_eq!(cpu[0].currency.as_deref(), Some("USD"));
        assert!(cpu[0].hourly_usd.is_none());
        let runpod = normalize(
            "runpod",
            "gpu",
            None,
            &json!({"offers":[{"gpuId":"H100","available":true,"secureCloud":true,"securePricePerHr":2,"dataCenterAvailability":[{"dataCenterId":"US","stockStatus":"High"},{"dataCenterId":"EU","stockStatus":"None"}]}]}),
        );
        assert_eq!(runpod.len(), 2);
        assert_eq!(runpod[1].available, Some(false));
        assert_eq!(runpod[0].location.as_deref(), Some("US"));
    }

    #[test]
    fn runpod_and_vast_quotes_are_comparable_but_community_and_unknown_currency_are_not() {
        let runpod = normalize(
            "runpod",
            "gpu",
            None,
            &json!([{
                "id":"H100", "displayName":"H100", "securePricePerHr":2.5,
                "communityPricePerHr":0.5, "memoryInGb":80
            }]),
        );
        assert_eq!(runpod[0].hourly_usd, Some(2.5));
        assert_eq!(runpod[0].vram_gb, Some(80.0));
        let vast = normalize(
            "vast",
            "gpu",
            None,
            &json!([{
                "id":123, "gpu_name":"RTX 4090", "dph_total":1.1,
                "gpu_ram":24000, "num_gpus":1, "rentable":true
            }]),
        );
        assert_eq!(vast[0].hourly_usd, Some(1.1));
        assert_eq!(vast[0].vram_gb, Some(24.0));
        let jarvis = normalize(
            "jarvis",
            "gpu",
            None,
            &json!({"offers":[{
                "gpu_type":"A100", "price_per_hour":100, "vram":"80 GB"
            }]}),
        );
        assert_eq!(jarvis[0].hourly_price, Some(100.0));
        assert_eq!(jarvis[0].hourly_usd, None);
    }

    #[test]
    fn monthly_or_missing_prices_never_enter_hourly_ranking() {
        let civo = normalize(
            "civo",
            "gpu",
            Some("LON1"),
            &json!({"offers":[{
                "name":"gpu-small", "price":700, "monthly_price":700
            }]}),
        );
        assert_eq!(civo.len(), 1);
        assert_eq!(civo[0].hourly_usd, None);
        assert_eq!(civo[0].location.as_deref(), Some("LON1"));
        assert!(normalize(
            "nebius",
            "gpu",
            Some("project-1"),
            &json!({"platforms":[{"id":"x"}]})
        )
        .is_empty());
        let cpu = normalize(
            "crusoe",
            "cpu",
            None,
            &json!({"offers":[
                {"name":"c1a.2x","hourly_price":0.12,"currency":"USD"},
                {"name":"h100-80gb-sxm-ib.8x","hourly_price":20,"currency":"USD"}
            ]}),
        );
        assert_eq!(cpu.len(), 1);
        assert_eq!(cpu[0].offer_id, "c1a.2x");
        assert_eq!(cpu[0].hourly_usd, Some(0.12));
    }

    #[test]
    fn price_and_capacity_parsing_rejects_ambiguous_values() {
        assert_eq!(number(&json!("$1.20")), None);
        assert_eq!(number(&json!(-1)), None);
        assert_eq!(vram_gb("jarvis", &json!({"vram":"48 GB"})), Some(48.0));
        // Jarvis' typed CLI schema documents bare numeric VRAM in GB.
        assert_eq!(vram_gb("jarvis", &json!({"vram":"48"})), Some(48.0));
    }

    #[test]
    fn no_stock_gpu_is_not_mistaken_for_a_rentable_quote() {
        let quotes = normalize(
            "runpod",
            "gpu",
            None,
            &json!([{
                "id":"H100", "securePricePerHr":2.5, "stockStatus":"None"
            }]),
        );
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].available, Some(false));
    }

    #[test]
    fn runpod_create_rechecks_secure_stock_and_approved_price_per_datacenter() {
        let inventory = json!([{
            "gpuId":"NVIDIA H100 80GB HBM3", "displayName":"H100 SXM", "available":true,
            "secureCloud":true, "securePricePerHr":2.69, "communityPricePerHr":1.99,
            "dataCenterAvailability":[
                {"dataCenterId":"US-KS-2","stockStatus":"Low"},
                {"dataCenterId":"EU-RO-1","stockStatus":"none"}
            ]
        }]);
        assert_eq!(
            runpod_secure_quote(&inventory, "NVIDIA H100 80GB HBM3", "US-KS-2", 2.69),
            Ok(2.69)
        );
        assert!(
            runpod_secure_quote(&inventory, "NVIDIA H100 80GB HBM3", "US-KS-2", 2.68)
                .unwrap_err()
                .contains("price rose")
        );
        assert!(
            runpod_secure_quote(&inventory, "NVIDIA H100 80GB HBM3", "EU-RO-1", 2.69)
                .unwrap_err()
                .contains("no longer in stock")
        );
        assert!(
            runpod_secure_quote(&inventory, "NVIDIA H100 80GB HBM3", "US-MO-1", 2.69)
                .unwrap_err()
                .contains("no longer listed")
        );
    }

    #[tokio::test]
    async fn invalid_filters_fail_before_invoking_any_provider() {
        assert!(compare(
            None,
            Some("serverless".into()),
            None,
            None,
            None,
            None,
            None,
            None
        )
        .await
        .is_err());
        assert!(
            compare(None, None, None, None, Some(f64::NAN), None, None, None)
                .await
                .is_err()
        );
        assert!(compare(
            None,
            Some("cpu".into()),
            None,
            None,
            None,
            None,
            Some(24.0),
            None
        )
        .await
        .is_err());
        assert!(compare(
            Some("unknown".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None
        )
        .await
        .is_err());
    }
}
