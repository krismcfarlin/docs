#[cfg(feature = "veles")]
pub mod veles;
#[cfg(feature = "veles")]
pub use veles::VelesClient;

pub const DIMS: usize = 64;

pub struct VelesHit {
    pub score: f32,
    pub version_id: String,
    pub page_id: String,
    pub title: String,
}

/// 64-dim bag-of-words embedding, L2-normalized.
pub fn embed(text: &str) -> [f32; DIMS] {
    let mut v = [0.0f32; DIMS];
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let w = word.to_lowercase();
        if w.len() < 3 {
            continue;
        }
        let mut h: u64 = 14_695_981_039_346_656_037;
        for b in w.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1_099_511_628_211);
        }
        v[(h % DIMS as u64) as usize] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// Cosine similarity between two embeddings.
pub fn cosine(a: &[f32; DIMS], b: &[f32; DIMS]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Serialize embedding to JSON string for SQLite storage.
pub fn to_json(v: &[f32; DIMS]) -> String {
    let inner: Vec<String> = v.iter().map(|x| format!("{:.6}", x)).collect();
    format!("[{}]", inner.join(","))
}

/// Deserialize embedding from SQLite JSON string.
pub fn from_json(s: &str) -> Option<[f32; DIMS]> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let nums: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if nums.len() != DIMS {
        return None;
    }
    let mut arr = [0.0f32; DIMS];
    arr.copy_from_slice(&nums);
    Some(arr)
}

/// Extract a ~160-char snippet from text centered around the first query term match.
pub fn make_snippet(text: &str, query: &str) -> String {
    let plain: String = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let lower = plain.to_lowercase();
    let pos = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .filter_map(|w| lower.find(&w.to_lowercase()))
        .min()
        .unwrap_or(0);
    let start = pos.saturating_sub(40);
    let window: String = plain.chars().skip(start).take(160).collect();
    let trimmed = window.trim().to_string();
    if start > 0 { format!("…{trimmed}") } else { trimmed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_is_normalized() {
        let v = embed("hello world foo bar");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5 || norm < 1e-9);
    }

    #[test]
    fn embed_same_text_same_vector() {
        assert_eq!(embed("rust programming"), embed("rust programming"));
    }

    #[test]
    fn cosine_self_is_one() {
        let v = embed("some content about things");
        let sim = cosine(&v, &v);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn json_roundtrip() {
        let v = embed("roundtrip test");
        let json = to_json(&v);
        let v2 = from_json(&json).unwrap();
        for (a, b) in v.iter().zip(v2.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
