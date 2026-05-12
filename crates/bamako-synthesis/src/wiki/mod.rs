use sha2::{Digest, Sha256};

/// Insert or update the "## Mentioned In" section of a wiki page.
pub fn update_mentioned_in_section(content: &str, src_title: &str) -> String {
    let mention_line = format!("- [[{}]]", src_title);
    if let Some(pos) = content.find("## Mentioned In") {
        if content.contains(&mention_line) {
            return content.to_string();
        }
        let after_heading = pos + "## Mentioned In".len();
        let mut result = content.to_string();
        result.insert_str(after_heading, &format!("\n{}", mention_line));
        result
    } else {
        format!("{}\n\n## Mentioned In\n{}\n", content.trim_end(), mention_line)
    }
}

/// Build a rich markdown stub for a promoted entity wiki page.
pub fn build_wiki_stub(
    name: &str,
    entity_type: &str,
    description: &str,
    mention_count: i64,
    source_pages: &[String],
    related_entities: &[(String, String)],
) -> String {
    let mut stub = format!(
        "# {}\n\n**Type:** {} · **Mentions:** {}\n\n{}\n",
        name, entity_type, mention_count, description
    );

    if !source_pages.is_empty() {
        stub.push_str("\n## Mentioned In\n");
        for title in source_pages {
            stub.push_str(&format!("- [[{}]]\n", title));
        }
    }

    if !related_entities.is_empty() {
        stub.push_str("\n## Related Entities\n");
        for (rel_name, rel_type) in related_entities {
            stub.push_str(&format!("- [[{}]] — {}\n", rel_name, rel_type));
        }
    }

    stub
}

/// SHA-256 hash of content (hex-encoded). Used to detect whether a page changed since last synthesis.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_mentioned_in_adds_section_when_absent() {
        let out = update_mentioned_in_section("# Entity\n\nSome description.", "SourceDoc");
        assert!(out.contains("## Mentioned In"));
        assert!(out.contains("- [[SourceDoc]]"));
    }

    #[test]
    fn update_mentioned_in_appends_to_existing_section() {
        let content = "# Entity\n\n## Mentioned In\n- [[First]]\n";
        let out = update_mentioned_in_section(content, "Second");
        assert!(out.contains("- [[First]]"));
        assert!(out.contains("- [[Second]]"));
    }

    #[test]
    fn update_mentioned_in_is_idempotent() {
        let content = "# Entity\n\n## Mentioned In\n- [[Doc]]\n";
        let out = update_mentioned_in_section(content, "Doc");
        let count = out.matches("- [[Doc]]").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn build_wiki_stub_structure() {
        let stub = build_wiki_stub(
            "Rust",
            "Language",
            "A systems language.",
            5,
            &["Doc A".to_string(), "Doc B".to_string()],
            &[("Cargo".to_string(), "TOOLCHAIN_OF".to_string())],
        );
        assert!(stub.starts_with("# Rust"));
        assert!(stub.contains("**Type:** Language"));
        assert!(stub.contains("**Mentions:** 5"));
        assert!(stub.contains("## Mentioned In"));
        assert!(stub.contains("- [[Doc A]]"));
        assert!(stub.contains("## Related Entities"));
        assert!(stub.contains("- [[Cargo]] — TOOLCHAIN_OF"));
    }

    #[test]
    fn content_hash_is_deterministic() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
        assert_ne!(content_hash("hello"), content_hash("world"));
    }
}
