# Task 7: Source Discovery — GitHubSearch + Aggregator

**Files:**
- Replace stub: `crates/proxy-sub/src/discover/github_search.rs`
- Replace stub: `crates/proxy-sub/src/discover/aggregator.rs`
- Test: inline tests

**Interfaces:**
- Consumes: `Discover` trait (Task 6, in `crate::discover`)
- Produces: `GitHubSearchDiscover`, `GitHubSearchConfig`, `AggregatorDiscover`, `AggregatorConfig`

## Requirements

### 1. GitHubSearchDiscover

**Config struct:**
```rust
pub struct GitHubSearchConfig {
    pub token: Option<String>,
    pub max_results: u32,
    pub keywords: Vec<String>,
    pub timeout_sec: u64,
}
```

**Search strategy:**
1. **Repository search**: `GET https://api.github.com/search/repositories?q={keyword}&sort=updated&order=desc&per_page={max_results}`
2. **Code search**: `GET https://api.github.com/search/code?q={keyword}&sort=updated&order=desc&per_page={max_results}`

**Repository search processing:**
- For each repo: extract `full_name` and `default_branch`
- Generate likely subscription file URLs: `https://raw.githubusercontent.com/{full_name}/{branch}/{filename}` for filenames: `clash.yaml`, `proxy.yaml`, `v2ray.yaml`, `sub.yaml`

**Code search processing:**
- For each result: extract `html_url`
- Convert GitHub page URL to raw URL: replace `github.com` → `raw.githubusercontent.com`, remove `/blob/` segment

**Rate limit handling:**
- 403 → log warning, return empty
- 429 → log warning, return empty

**Auth header**: If token is set, add `Authorization: Bearer {token}` and `Accept: application/vnd.github+json`

**Dedup**: After collecting all URLs from all keywords, dedup the final list

**Inline tests:**
- `test_github_to_raw_url`: verify URL conversion
- `test_github_to_raw_url_no_blob`: verify passthrough for already-raw URLs

### 2. AggregatorDiscover

**Config struct:**
```rust
pub struct AggregatorConfig {
    pub url: String,
    pub format: String,  // "text", "json", "yaml"
    pub timeout_sec: u64,
}
```

**Format parsing:**
- `text`: one URL per line, skip empty lines and lines starting with `#`, only keep lines starting with `http://` or `https://`
- `json`: JSON array of `{ "url": "..." }` objects or plain strings
- `yaml`: YAML with `subscriptions:` key containing a list of strings or `{url: ...}` objects

**Error handling**: fetch failure → log warning, return empty; parse failure → log warning, return empty

**Inline tests:**
- `test_parse_text_list`: 3 URLs from text with comment and blank line
- `test_parse_json_list`: 2 URLs from JSON array
- `test_parse_yaml_list`: 2 URLs from YAML

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add GitHubSearch and Aggregator discoverers`
