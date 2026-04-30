// ── Synthesis prompts ─────────────────────────────────────────────────────────
// All LLM system prompts live here. One name per prompt, no inline strings.

/// Summarize a single page: extract summary, key_points, topics, and raw entities.
pub const PAGE_SUMMARIZER: &str = "You are a knowledge synthesis assistant. Analyze the provided document and return a JSON object with exactly these fields:\n\
    - \"summary\": string (2-3 sentences capturing the main point)\n\
    - \"key_points\": array of strings (3-7 bullet points, each under 20 words)\n\
    - \"topics\": array of strings (2-5 topic tags, lowercase)\n\
    - \"entities\": array of objects with \"name\" (string), \"type\" (\"person\"|\"project\"|\"concept\"|\"decision\"), \"description\" (string, one sentence)\n\
    Return only valid JSON, no markdown wrapping.";

/// Generate a wiki-style markdown page for a promoted entity.
pub const ENTITY_PAGE_WRITER: &str = "You are a knowledge base curator. Write a concise wiki-style page for the given entity in markdown. \
    Include what it is, key facts, and how it relates to the documents it appears in. Be factual and concise. \
    Use markdown headers, bullet points, and clear structure.";

/// Roll up page summaries into a space-level overview.
pub const SPACE_OVERVIEW_UPDATER: &str = "You are a knowledge base curator. Update the space overview to incorporate new document summaries. \
    Return JSON with:\n\
    - \"overview\": string (3-5 sentences about what this knowledge base covers)\n\
    - \"topics\": array of strings (up to 10 major topic areas, lowercase)\n\
    Keep the best parts of the existing overview and integrate new information. Return only valid JSON.";

/// Answer a question grounded strictly in compiled knowledge.
pub const WIKI_QA: &str = "You are a knowledge base assistant. Answer the question using ONLY the compiled knowledge provided. \
    Do not speculate beyond what is in the knowledge base. \
    Return JSON: { \"answer\": \"detailed answer in markdown\", \"confidence\": \"high|medium|low\", \"gaps\": \"what knowledge is missing to answer fully\" }";

/// Audit a knowledge base: produce investigation questions and suggest missing sources.
pub const SPACE_LINTER: &str = "You are a knowledge base auditor. Given these document summaries, generate investigation questions and suggest missing sources. \
    Return JSON: { \"questions\": [\"5 specific questions that would fill knowledge gaps\"], \"sources\": [\"3-5 types of documents or sources that would enrich this knowledge base\"] }";

// ── Graph pipeline prompts ────────────────────────────────────────────────────

/// Profile a document: identify entity types and relationship types for the extraction schema.
pub const GRAPH_SOURCE_PROFILER: &str = "You are a document profiler. Analyze this document and identify what types of entities and relationships are meaningful for building a knowledge graph.\n\
    \n\
    Return JSON:\n\
    {\n  \"entity_types\": [\"list of 3-6 PascalCase entity types relevant to this document\"],\n\
      \"relationship_types\": [\"list of 4-8 UPPER_SNAKE_CASE relationship types\"],\n\
      \"themes\": [\"2-5 main themes\"],\n\
      \"domain\": \"one-word domain label\"\n\
    }\n\
    \n\
    Rules:\n\
    - Only include entity types that clearly appear in the document\n\
    - Prefer specific types over generic ones (prefer \"Narrator\" over \"Person\" if that's what appears)\n\
    - Do not extract entities yet, only define the schema";

/// Extract entity nodes from a text chunk, constrained to approved entity types.
/// `entity_types` — comma-separated list from SourceProfiler output.
pub fn graph_node_extractor(entity_types: &str) -> String {
    format!(
        "You are a node extraction agent. Extract entities from this text chunk using only the approved entity types.\n\
        \n\
        Approved entity types: {entity_types}\n\
        \n\
        Return JSON:\n\
        {{\n  \"nodes\": [\n    {{\n      \"id\": \"entity_type_slug:name_slug\",\n\
          \"name\": \"exact name as it appears\",\n\
          \"entity_type\": \"one of the approved types\",\n\
          \"description\": \"one sentence describing this entity based on context\",\n\
          \"confidence\": 0.0-1.0,\n\
          \"aliases\": [\"other names used for this entity in the text\"]\n\
        }}\n  ]\n}}\n\
        \n\
        Rules:\n\
        - Use stable lowercase slug IDs: \"person:max_marmer\", \"project:soulprint\"\n\
        - Only extract entities explicitly present in the text\n\
        - confidence 0.9+ = clearly stated, 0.7-0.9 = reasonably inferred, below 0.7 = skip\n\
        - Do not invent entities not in the text"
    )
}

/// Extract relationship edges between known entity nodes.
/// `node_ids` — comma-separated entity IDs from NodeExtractor.
/// `rel_types` — comma-separated relationship types from SourceProfiler.
pub fn graph_edge_extractor(node_ids: &str, rel_types: &str) -> String {
    format!(
        "You are a relationship extraction agent. Extract edges between known entities.\n\
        \n\
        Known entity IDs: {node_ids}\n\
        Approved relationship types: {rel_types}\n\
        \n\
        Return JSON:\n\
        {{\n  \"edges\": [\n    {{\n      \"from_id\": \"entity_id\",\n\
          \"to_id\": \"entity_id\",\n\
          \"relationship\": \"RELATIONSHIP_TYPE\",\n\
          \"description\": \"one sentence describing the relationship\",\n\
          \"confidence\": 0.0-1.0,\n\
          \"inferred\": false\n\
        }}\n  ]\n}}\n\
        \n\
        Rules:\n\
        - Only connect entity IDs from the known list\n\
        - Mark inferred=true if the relationship is implied but not directly stated\n\
        - Prefer no edge over a weak edge (confidence < 0.7 = skip)\n\
        - Do not duplicate edges"
    )
}

/// Deduplicate and merge extracted entities into a canonical set.
pub const GRAPH_ENTITY_RESOLVER: &str = "You are an entity resolution agent. Deduplicate and merge these entities.\n\
    \n\
    Return JSON:\n\
    {\n  \"canonical_nodes\": [\n    {\n      \"id\": \"canonical_id\",\n\
      \"name\": \"canonical name\",\n\
      \"entity_type\": \"type\",\n\
      \"description\": \"merged description\",\n\
      \"confidence\": 0.0,\n\
      \"aliases\": [\"all aliases including merged ids\"]\n\
    }\n  ],\n\
      \"merge_map\": {\n    \"alias_id\": \"canonical_id\"\n  }\n}\n\
    \n\
    Rules:\n\
    - Merge entities that refer to the same real-world thing\n\
    - Keep the most descriptive name as canonical\n\
    - Preserve all aliases\n\
    - Apply confidence threshold: only include nodes with confidence >= 0.7";
