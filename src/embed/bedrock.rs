//! Amazon Bedrock embedding backend.
//!
//! The backend uses the AWS SDK's default credential and region provider
//! chain. Titan Text Embeddings V2 accepts one input per InvokeModel request;
//! the shared embedding driver still provides checkpointed cache writes.

use anyhow::{Context, Result, bail};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::primitives::Blob;
use aws_types::region::Region;
use serde::Deserialize;
use serde_json::json;
use tokio::runtime::Runtime;

use super::Backend;

const DEFAULT_DIMENSIONS: u16 = 1024;

#[derive(Deserialize)]
struct Response {
    embedding: Vec<f32>,
}

pub struct Bedrock {
    runtime: Runtime,
    client: Client,
    model: String,
}

impl Bedrock {
    pub fn load(model: String, region: Option<String>) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("creating the Bedrock async runtime")?;
        let sdk_config = runtime.block_on(async {
            let loader =
                aws_config::from_env().behavior_version(aws_config::BehaviorVersion::latest());
            match region {
                Some(region) => loader.region(Region::new(region)).load().await,
                None => loader.load().await,
            }
        });
        let client = Client::new(&sdk_config);
        eprintln!("bedrock backend: {model} ({DEFAULT_DIMENSIONS} dimensions)");
        Ok(Self {
            runtime,
            client,
            model,
        })
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let body = request_body(&self.model, text)?;
        let output = self
            .runtime
            .block_on(
                self.client
                    .invoke_model()
                    .model_id(&self.model)
                    .content_type("application/json")
                    .accept("application/json")
                    .body(Blob::new(body))
                    .send(),
            )
            .with_context(|| format!("invoking Bedrock model {}", self.model))?;
        let response: Response = serde_json::from_slice(output.body().as_ref())
            .context("decoding the Bedrock embedding response")?;
        if response.embedding.is_empty() {
            bail!("Bedrock returned an empty embedding for {}", self.model);
        }
        Ok(response.embedding)
    }
}

fn request_body(model: &str, text: &str) -> Result<Vec<u8>> {
    let payload = if model.contains("titan-embed-text-v2") {
        json!({
                "inputText": text,
                "dimensions": DEFAULT_DIMENSIONS,
            "normalize": true,
        })
    } else {
        // Titan Text Embeddings G1 accepts only inputText.
        json!({ "inputText": text })
    };
    Ok(serde_json::to_vec(&payload)?)
}

impl Backend for Bedrock {
    fn preferred_chunk(&self) -> usize {
        // Titan exposes one text per InvokeModel request. Keep cache writes
        // frequent so an interrupted or throttled run resumes cheaply.
        16
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_titan_response() {
        let response: Response = serde_json::from_str(r#"{"embedding":[0.1,-0.2]}"#).unwrap();
        assert_eq!(response.embedding, vec![0.1, -0.2]);
    }

    #[test]
    fn uses_v2_options_only_for_v2() {
        let v2: serde_json::Value =
            serde_json::from_slice(&request_body("amazon.titan-embed-text-v2:0", "fn").unwrap())
                .unwrap();
        assert_eq!(v2["dimensions"], 1024);
        assert_eq!(v2["normalize"], true);

        let v1: serde_json::Value =
            serde_json::from_slice(&request_body("amazon.titan-embed-text-v1", "fn").unwrap())
                .unwrap();
        assert!(v1.get("dimensions").is_none());
        assert!(v1.get("normalize").is_none());
    }
}
