# Public Proxy Source Research

Checked on 2026-07-07 from the local dev workspace.

## Selected Sources

| Source | URL shape | Format |
|---|---|---|
| Proxifly | `https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/all/data.txt` | `scheme://host:port` |
| Databay HTTP | `https://raw.githubusercontent.com/databay-labs/free-proxy-list/master/http.txt` | `host:port` |
| Databay SOCKS5 | `https://raw.githubusercontent.com/databay-labs/free-proxy-list/master/socks5.txt` | `host:port` |
| IPLocate | `https://raw.githubusercontent.com/iplocate/free-proxy-list/main/all-proxies.txt` | `scheme://host:port` |
| VPSLab HTTP | `https://raw.githubusercontent.com/VPSLabCloud/VPSLab-Free-Proxy-List/main/http_all.txt` | comments plus `host:port` |
| VPSLab SOCKS5 | `https://raw.githubusercontent.com/VPSLabCloud/VPSLab-Free-Proxy-List/main/socks5_all.txt` | comments plus `host:port` |
| Monosans | `https://raw.githubusercontent.com/monosans/proxy-list/main/proxies.json` | JSON array |

## Observed Samples

Proxifly and IPLocate include schemes:

```text
socks5://208.102.51.6:58208
```

Databay uses plain entries:

```text
160.25.237.73:1111
```

VPSLab includes comments and blank lines:

```text
# Updated Proxies: 2026-07-07 09:52 UTC
# Protocol: http | SSL: all | Anonymity: all

194.150.110.134:80
```

Monosans JSON includes `protocol`, `host`, and numeric `port` fields.

## Excluded

VPN registration automation is excluded. Legal alternatives are user-supplied
OpenVPN/WireGuard profiles or provider API keys in a separate provider task.
