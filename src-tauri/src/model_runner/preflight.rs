use super::*;
const ARCHITECTURES:&str=include_str!("architectures-5.18.0.json");
async fn fetch_metadata(client:&reqwest::Client,url:String)->Result<Value,String>{
    let mut response=client.get(url).send().await.map_err(|_|"Cannot reach Hugging Face for model compatibility preflight")?;
    if !response.status().is_success(){return Err(format!("Hugging Face model metadata returned HTTP {}. Private/gated repositories are not supported by this generic preflight; use an accessible model or a dedicated deployment with a protected HF_TOKEN binding. No environment was created.",response.status().as_u16()))}
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|"Cannot read Hugging Face model metadata")?{if bytes.len()+chunk.len()>2*1024*1024{return Err("Model metadata exceeds 2 MiB; inspect this repository's dedicated runner".into())}bytes.extend_from_slice(&chunk)}
    serde_json::from_slice(&bytes).map_err(|_|"Model metadata is invalid JSON".into())
}
fn inspect(model:&str,metadata:&Value,config:&Value)->Value{
    let files=metadata["siblings"].as_array().cloned().unwrap_or_default();
    let has=|name:&str|files.iter().any(|f|f["rfilename"]==name);
    let decision=has("joint_head_config.json")&&has("joint_head.safetensors");
    let task=metadata["pipeline_tag"].as_str().unwrap_or("unknown");
    let model_type=config["model_type"].as_str().unwrap_or("unknown");
    let catalog:Value=serde_json::from_str(ARCHITECTURES).expect("pinned architecture catalog");
    let known=catalog["types"].as_array().unwrap().iter().any(|name|name==model_type);
    let safetensors=files.iter().filter(|f|f["rfilename"].as_str().is_some_and(|s|s.ends_with(".safetensors"))).collect::<Vec<_>>();
    let task_compatible=["unknown","text-generation","image-text-to-text"].contains(&task);
    let supported=!decision&&known&&task_compatible&&!safetensors.is_empty();
    let weight_bytes=safetensors.iter().filter_map(|f|f["lfs"]["size"].as_u64().or(f["size"].as_u64())).sum::<u64>();
    let parameters=metadata["safetensors"]["total"].as_u64();
    let estimated_bytes=if weight_bytes>0{Some(weight_bytes)}else{parameters.map(|p|p.saturating_mul(2))};
    let storage=estimated_bytes.map(|n|((n as f64/(1024.0*1024.0*1024.0))*1.25+12.0).ceil());
    let vram=estimated_bytes.map(|n|((n as f64/(1024.0*1024.0*1024.0))*1.2+2.0).ceil());
    let reason=if decision{"Structured decision model with a custom prediction head; use its dedicated SDK/decision API, not the generic chat runner"}else if !task_compatible{"This repository's task requires a specialized runner; the Yougori chat runner accepts causal text generation"}else if !known{"The pinned Transformers architecture catalog does not support this model type with built-in causal generation"}else if safetensors.is_empty(){"No safetensors weights were found; this runner does not execute remote code or load pickle checkpoints"}else{"Built-in Transformers causal generation is supported; guest dependency/config validation remains required"};
    let dependencies=if supported{json!({"applicable":true,"pythonMinimum":"3.10","transformers":"5.18.0","accelerate":"1.15.0","huggingfaceHub":"1.33.0","pytorch":"2.8.0","cuda":"12.8","additionalRepositoryDependenciesVerified":false})}else{json!({"applicable":false,"requiresDedicatedSdk":true,"pythonMinimum":null,"reason":"Select the repository's dedicated SDK and required Python version, then pin them in the deployment setup; generic chat dependencies do not establish compatibility"})};
    json!({"model":model,"task":if decision{"structured-decision"}else{task},"modelType":model_type,"supported":supported,"runner":if supported{"yougori-transformers-chat"}else{"dedicatedRunnerRequired"},"reason":reason,"revision":metadata["sha"],"dependencies":dependencies,"resources":{"cpuRecommended":2,"memoryGbRecommended":4,"storageGbRecommended":storage,"gpuMemoryGbEstimated":vram,"weightsBytes":estimated_bytes,"estimateOnly":true},"downloads":{"location":"persistent guest model volume","revisionPinned":true,"safetensorsOnly":true,"checksumVerification":"requiredBeforeLoad","checksumsVerified":false,"hostWeightImportRequired":false},"remoteCodeAllowed":false})
}
pub(super) async fn preflight(model:&str)->Result<Value,String>{
    let client=reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(10)).timeout(std::time::Duration::from_secs(35)).redirect(reqwest::redirect::Policy::limited(3)).build().map_err(|_|"Cannot initialize model preflight")?;
    let metadata=fetch_metadata(&client,format!("https://huggingface.co/api/models/{model}?blobs=true")).await?;
    let sha=metadata["sha"].as_str().filter(|s|s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("Model metadata did not provide an immutable revision")?;
    let config=fetch_metadata(&client,format!("https://huggingface.co/{model}/resolve/{sha}/config.json")).await?;
    Ok(inspect(model,&metadata,&config))
}
#[tauri::command]
pub async fn model_preflight(model:String)->Result<Value,String>{preflight(&normalize_model(&model)?).await}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn decision_models_never_get_a_chat_deployment(){let meta=json!({"sha":"a".repeat(40),"siblings":[{"rfilename":"joint_head_config.json"},{"rfilename":"joint_head.safetensors"}],"pipeline_tag":"text-generation"});let result=inspect("Cloudflare/clef",&meta,&json!({"model_type":"qwen3_5"}));assert_eq!(result["supported"],false);assert_eq!(result["task"],"structured-decision");assert_eq!(result["dependencies"]["applicable"],false);assert_eq!(result["dependencies"]["requiresDedicatedSdk"],true);assert!(result["dependencies"]["pythonMinimum"].is_null());}
    #[test]fn unknown_architectures_and_unsafe_weights_are_rejected_before_creation(){for(config,files)in[(json!({"model_type":"future_unknown"}),json!([{"rfilename":"model.safetensors"}])),(json!({"model_type":"llama"}),json!([{"rfilename":"pytorch_model.bin"}]))]{let result=inspect("test/model",&json!({"siblings":files}),&config);assert_eq!(result["supported"],false);}}
    #[test]fn known_supported_weights_report_persistent_direct_download_and_resources(){let result=inspect("test/model",&json!({"pipeline_tag":"text-generation","siblings":[{"rfilename":"model.safetensors","size":1073741824u64}]}),&json!({"model_type":"llama"}));assert_eq!(result["supported"],true);assert_eq!(result["resources"]["storageGbRecommended"],14.0);assert_eq!(result["downloads"]["hostWeightImportRequired"],false);}
}
