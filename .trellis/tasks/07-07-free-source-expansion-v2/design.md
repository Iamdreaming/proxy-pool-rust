# Design: free-source-expansion-v2

## Source Model

The new sources are normal `proxy-core::fetcher::base::Fetcher`
implementations. They return raw candidates and let the scheduler perform the
existing deduplication, validation, source survival accounting, scoring, and
storage.

The MVP keeps one source module for public raw list sources rather than copying
the same HTTP/error-handling code into five files. Each configured source is a
small set of endpoints:

- a stable source id prefix;
- a human-readable source name;
- a URL;
- an optional forced protocol when the list itself does not include a scheme;
- a parser kind (`text` or `monosans_json`);
- whether the URL should use the GitHub raw mirror prefix.

## Parsing

Text parsing accepts:

- `host:port`
- `http://host:port`
- `https://host:port`
- `socks4://host:port`
- `socks5://host:port`

Blank lines and lines beginning with `#` are ignored. Entries without a scheme
use the endpoint's forced protocol. Entries with a scheme use the parsed scheme.
Invalid protocols, blank hosts, and zero/invalid ports are skipped.

Monosans JSON parsing accepts an array of objects with `protocol`, `host`, and
numeric or string `port` fields.

## Config

`FetchersConfig` gains toggles:

- `proxifly`
- `databay`
- `iplocate`
- `vpslab`
- `monosans`

All default to enabled to match existing fetcher toggles. GitHub-backed sources
use `github_mirror_prefix` when their toggle has `use_mirror: true`.

## Rollback

Disable a noisy source in config with:

```yaml
pool:
  fetchers:
    proxifly: { enabled: false }
```

Or revert the source module and config wiring if candidate volume causes
unacceptable refresh pressure.
