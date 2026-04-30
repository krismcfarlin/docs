# Docmost vs Bamako: Feature Comparison
Generated: 2026-03-23

## Sources
- https://docmost.com/docs/ (all sub-pages)
- https://docmost.com (homepage)
- https://docmost.com/docs/editions
- Bamako codebase knowledge as described

---

## Section 1: Features Bamako Already Has (Confirmed Coverage)

| Docmost Feature | Bamako Equivalent |
|---|---|
| Spaces (team organization) | Spaces with parent_space_id, drag/drop reorder |
| Nested pages / page hierarchy | Pages tree with sub-pages, drag/drop |
| Rich text editor | Milkdown WYSIWYG + markdown source toggle |
| Auto-save | Auto-save on edit |
| Page history / restore | Page versions: Fork, Publish, Freeze + version picker dropdown |
| Version diff | Version diff view |
| Import Markdown | Import: markdown files (drag/drop + file picker) |
| Export Markdown | Export: markdown download |
| Print to PDF | PDF print |
| Semantic / AI search | Local vector embeddings (VelesDB/SQLite) + semantic search sidebar |
| Full-text search | Semantic search covers this |
| Delete page, delete space | Delete page, delete space |
| Page rename | Page rename |
| Settings screen | Settings screen (sqld URL, Google OAuth) |
| Import from Google Docs | Google Docs import (OAuth, per-tab splitting) |
| Activity log | Activity log panel |
| Empty state onboarding | Empty state onboarding |

---

## Section 2: Docmost Features Bamako Is Missing

### EDITOR & CONTENT BLOCKS

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 1 | **Inline comments / annotation** | Highlight text, leave a threaded comment in a side panel; open/resolved tabs | Medium | YES — useful for personal review notes and future sharing |
| 2 | **Table of contents panel** | Auto-generated TOC from headings, shown as page outline | Small | YES — great for long docs |
| 3 | **Toggle/collapsible blocks** | Expandable content sections | Small | YES — common wiki feature |
| 4 | **Callout blocks** | Styled info/warning/tip boxes | Small | YES |
| 5 | **Math / equations (KaTeX)** | Inline and block LaTeX math rendering | Small | YES — for technical notes |
| 6 | **Mermaid diagrams** | Text-based flowcharts, sequence diagrams, Gantt, ER, etc. | Small | YES |
| 7 | **Draw.io integration** | Full drag-and-drop diagramming embedded in page | Medium | YES |
| 8 | **Excalidraw integration** | Freehand whiteboard/sketching embedded in page | Medium | YES |
| 9 | **Video embeds** | Embed YouTube, Vimeo, Loom directly in page | Small | MAYBE — useful but less critical offline |
| 10 | **Iframe / service embeds** | Airtable, Figma, Miro, Typeform, Google Drive, Framer, etc. | Small-Medium | MAYBE — most require internet anyway |
| 11 | **Status badges** | Inline "in progress / done / blocked" status indicators | Small | YES — good for project notes |
| 12 | **Date blocks** | Insert date references inline | Small | YES |
| 13 | **Subpage blocks** | Inline link/embed to child pages within editor content | Small | YES |
| 14 | **@ Mentions (people)** | Mention workspace members by @name | Medium | LOW — single-user desktop app, not needed solo |
| 15 | **@ Mentions (pages)** | Link to other pages via @ syntax | Small | YES — internal cross-linking |
| 16 | **Emoji via :keyword: syntax** | Insert emoji with colon shorthand | Small | YES — nice QoL |
| 17 | **Heading anchor links** | Copy link to specific heading/section | Small | YES |
| 18 | **Block drag handles** | Drag any block to reorder via left-side handle | Small | YES — editor UX polish |
| 19 | **Table: merge/split cells** | Full table cell merging and splitting | Medium | YES |
| 20 | **Table: column resize** | Drag to resize table column widths | Small | YES |
| 21 | **Table: cell background color** | Per-cell background coloring | Small | MAYBE |
| 22 | **Text color / highlight color** | Per-character foreground and highlight colors | Small | YES |
| 23 | **Subscript / superscript** | Text formatting for sub/superscript | Small | YES |
| 24 | **Text alignment** | Left, center, right, justify | Small | YES |
| 25 | **Full-width page layout toggle** | Per-page toggle to expand to full browser width | Small | YES |
| 26 | **Edit vs Read mode toggle** | Per-page switch between edit and read-only view | Small | YES — prevents accidental edits |
| 27 | **Copy page as Markdown** | One-click "copy full content as markdown" button | Small | YES |

### PAGE MANAGEMENT

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 28 | **Trash / soft delete** | Pages go to trash first; can restore or permanently delete | Medium | YES — safety net |
| 29 | **Move page between spaces** | Drag or menu to relocate a page to a different space | Medium | YES |
| 30 | **Duplicate / copy page** | Clone a page (and optionally subpages) | Small | YES |
| 31 | **Public sharing / shareable link** | Generate a public URL for a page; optional subpage inclusion + search indexing | Medium | LOW — desktop-first, but useful for sharing outputs |
| 32 | **Page cover images** | Hero image at top of page | Medium | MAYBE — cosmetic |
| 33 | **Page icons** | Emoji or image icon per page (shown in sidebar) | Small | YES — navigation UX |
| 34 | **Page-level permissions** | Override space permissions per individual page (Enterprise) | Large | LOW — single-user initially |

### SPACE MANAGEMENT

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 35 | **Space export (HTML + attachments as ZIP)** | Export full space with folder structure as ZIP | Medium | YES |
| 36 | **Space export (Markdown + attachments as ZIP)** | Bamako has markdown export per page, not full space ZIP | Medium | YES |
| 37 | **Space description field** | Optional description on a space | Small | YES |
| 38 | **Space slug** | URL-friendly identifier editable per space | Small | LOW — less relevant locally |

### IMPORT

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 39 | **Import HTML files** | .html file import (Bamako only has .md and Google Docs) | Small | YES |
| 40 | **Import DOCX (Word)** | Microsoft Word document import (Enterprise in Docmost) | Medium | YES |
| 41 | **Import from Notion (ZIP)** | Full Notion export ZIP import | Large | MAYBE — migration tool |
| 42 | **Import from Confluence** | Confluence export import (Enterprise in Docmost) | Large | LOW — enterprise migration only |
| 43 | **Bulk import via ZIP** | Import a ZIP of multiple .md or .html files at once | Small | YES |

### SEARCH

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 44 | **Attachment full-text search** | Search inside PDF and DOCX file content (Enterprise) | Large | YES — powerful for research notes |
| 45 | **Space filter in search** | Narrow search to a specific space | Small | YES |
| 46 | **Content type filter** | Filter search by pages vs attachments | Small | YES |
| 47 | **Cmd+K universal search shortcut** | Global keyboard shortcut to open search | Small | YES |

### COLLABORATION (Multi-user; lower priority for single-user desktop)

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 48 | **Real-time collaborative editing** | Multiple cursors, live sync via WebSocket | Large | LOW — Bamako is local-first single-user |
| 49 | **User roles (Owner/Admin/Member)** | Workspace-level role system | Large | LOW — single user initially |
| 50 | **Groups / team permission management** | Assign space access to user groups | Large | LOW |
| 51 | **Space member management** | Add/remove users per space with role | Large | LOW |
| 52 | **Invite via email** | Email invitations for workspace members | Large | LOW — no server/email infra |
| 53 | **Resolve comments** | Mark inline comments as resolved (Enterprise) | Medium | LOW without multi-user |

### AI (INLINE EDITOR)

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 54 | **AI writing assistance (Ask AI in editor)** | Improve writing, fix grammar, make longer/shorter, continue, explain, summarize | Medium | YES — high value for personal notes |
| 55 | **AI tone adjustment** | Rewrite as Professional/Casual/Friendly | Small | YES |
| 56 | **AI translation** | Translate page content to 11 languages | Medium | YES |

### AUTHENTICATION & SECURITY (Less relevant for local-first)

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 57 | **TOTP / MFA** | Two-factor authentication via authenticator app (Enterprise) | Large | LOW — local desktop app |
| 58 | **SAML 2.0 SSO** | Enterprise SSO via SAML | Large | NO |
| 59 | **OpenID Connect SSO** | Enterprise SSO via OIDC | Large | NO |
| 60 | **LDAP** | Directory service authentication | Large | NO |

### UX / PREFERENCES

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 61 | **Dark mode / theme toggle** | Light, Dark, or System preference | Small | YES — standard expectation |
| 62 | **Default page edit mode preference** | User setting: open pages in edit vs read mode | Small | YES |
| 63 | **Interface language / i18n** | 12+ language UI translations | Large | MAYBE — depends on audience |

### DEVELOPER / INTEGRATION

| # | Feature | Description | Effort | Want for Desktop-First? |
|---|---|---|---|---|
| 64 | **REST API with API keys** | CRUD pages/spaces/attachments via REST; per-user API keys with expiry (Enterprise) | Large | MAYBE — power users |
| 65 | **MCP server** | Model Context Protocol server for Claude/Cursor AI clients (Enterprise) | Large | MAYBE — AI-first users |
| 66 | **Audit logs** | Workspace-level audit trail of admin actions (Enterprise) | Large | LOW — single user |

---

## Section 3: Priority Recommendations for Bamako (Desktop-First, Local-First)

### High Value / Should Build Soon

These features are either expected by any wiki/notes user or have outsized UX value:

| Priority | Feature | Effort | Rationale |
|---|---|---|---|
| 1 | **Dark mode** (#61) | Small | Table stakes for any desktop app in 2026 |
| 2 | **Cmd+K search shortcut** (#47) | Small | Standard power-user expectation |
| 3 | **Page icons (emoji)** (#33) | Small | Huge sidebar navigability improvement |
| 4 | **Table of contents panel** (#2) | Small | Essential for long documents |
| 5 | **Trash / soft delete** (#28) | Medium | Safety net; avoids accidental data loss |
| 6 | **Toggle/collapsible blocks** (#3) | Small | Common wiki feature, reduces clutter |
| 7 | **Callout blocks** (#4) | Small | Note/warning/tip boxes are heavily used |
| 8 | **Edit vs Read mode toggle** (#26) | Small | Prevents accidental edits |
| 9 | **Mermaid diagrams** (#6) | Small | Technical notes without external tools |
| 10 | **@ page mentions** (#15) | Small | Internal cross-linking; core wiki feature |
| 11 | **Duplicate/copy page** (#30) | Small | Commonly expected |
| 12 | **Move page between spaces** (#29) | Medium | Reorganization as knowledge grows |
| 13 | **Text color / highlight color** (#22) | Small | Formatting users expect |
| 14 | **Math / KaTeX** (#5) | Small | High value for technical users |
| 15 | **Full-width page layout toggle** (#25) | Small | Quick win for reading experience |
| 16 | **AI writing assistance in editor** (#54) | Medium | High value given Bamako already has AI/embeddings |
| 17 | **HTML import** (#39) | Small | Common web content import |
| 18 | **Bulk ZIP import** (#43) | Small | Power migration workflow |
| 19 | **Space export as ZIP** (#35, #36) | Medium | Full backup capability |
| 20 | **Copy page as Markdown** (#27) | Small | One-liner; power user convenience |

### Medium Priority / Nice to Have

| Priority | Feature | Effort | Rationale |
|---|---|---|---|
| 21 | **Draw.io integration** (#7) | Medium | Best-in-class diagram tool |
| 22 | **Excalidraw integration** (#8) | Medium | Whiteboarding / architecture sketches |
| 23 | **Inline comments** (#1) | Medium | Useful for self-review and future collab |
| 24 | **Status badges** (#11) | Small | Project tracking in notes |
| 25 | **Block drag handles** (#18) | Small | Editor polish |
| 26 | **Heading anchor links** (#17) | Small | Deep linking to sections |
| 27 | **Table full features** (#19, #20) | Medium | Table power users |
| 28 | **Page cover images** (#32) | Medium | Visual organization |
| 29 | **DOCX import** (#40) | Medium | Common enterprise document type |
| 30 | **Attachment full-text search** (#44) | Large | Research-heavy use cases |
| 31 | **Public shareable links** (#31) | Medium | Sharing outputs without sharing app |
| 32 | **Video embeds** (#9) | Small | Content richness |
| 33 | **Default edit mode preference** (#62) | Small | Power user preference |

### Low Priority / Skip for Now

| Feature | Reason to Defer |
|---|---|
| Real-time collaboration (#48) | Fundamental architectural change; local-first conflicts with multi-user sync |
| User roles / groups / permissions (#49-52) | Single-user app — unnecessary complexity |
| SAML/OIDC/LDAP SSO (#58-60) | Enterprise server feature, irrelevant locally |
| MFA/TOTP (#57) | Overkill for local desktop |
| REST API / MCP server (#64, #65) | Build after core features stabilize |
| Audit logs (#66) | Single user, already have activity log |
| Confluence import (#42) | Niche migration tool |
| i18n (#63) | Only if targeting non-English markets |
| Notion import (#41) | Large effort, niche migration use case |

---

## Summary Counts

- **Total Docmost features inventoried:** ~66 distinct features
- **Already in Bamako:** ~17 features
- **Missing from Bamako:** ~49 features
- **High-value / small-effort quick wins:** 15 features
- **Features to skip (multi-user/enterprise only):** ~15 features
