use super::{DIMS, VelesHit};

pub struct VelesClient {
    client: reqwest::Client,
    base_url: String,
}

impl VelesClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn ensure_collection(&self, name: &str) -> bool {
        let body = serde_json::json!({
            "name": name,
            "dimension": DIMS as u64,
            "metric": "cosine"
        });
        match self.client
            .post(format!("{}/collections", self.base_url))
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r.status().is_success() || r.status().as_u16() == 409,
            Err(_) => false,
        }
    }

    pub async fn upsert(
        &self,
        collection: &str,
        version_id: &str,
        page_id: &str,
        title: &str,
        vector: &[f32; DIMS],
    ) -> bool {
        let point_id = str_to_id(version_id);
        let body = serde_json::json!({
            "points": [{
                "id": point_id,
                "vector": vector.to_vec(),
                "payload": {
                    "version_id": version_id,
                    "page_id": page_id,
                    "title": title
                }
            }]
        });
        self.client
            .post(format!("{}/collections/{}/points", self.base_url, collection))
            .json(&body)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn search(
        &self,
        collection: &str,
        query_vec: &[f32; DIMS],
        top_k: usize,
    ) -> Option<Vec<VelesHit>> {
        let body = serde_json::json!({
            "vector": query_vec.to_vec(),
            "top_k": top_k as u64
        });
        let resp = self.client
            .post(format!("{}/collections/{}/search", self.base_url, collection))
            .json(&body)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        #[derive(serde::Deserialize)]
        struct RawHit {
            score: f32,
            payload: Option<serde_json::Value>,
        }

        let hits: Vec<RawHit> = resp.json().await.ok()?;
        Some(
            hits.into_iter()
                .filter_map(|h| {
                    let p = h.payload?;
                    Some(VelesHit {
                        score: h.score,
                        version_id: p["version_id"].as_str()?.to_string(),
                        page_id: p["page_id"].as_str()?.to_string(),
                        title: p["title"].as_str().unwrap_or("Untitled").to_string(),
                    })
                })
                .collect(),
        )
    }
}

fn str_to_id(s: &str) -> u64 {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}
