"""Integration test suite for proxy-pool-rust service.

All tests run against a deployed instance. Configure via environment variables:
  PROXY_POOL_HOST   - server hostname/IP (default: 100.64.0.2)
  PROXY_POOL_API_PORT   - REST API port (default: 8000)
  PROXY_POOL_GW_PORT    - Gateway port (default: 9080)
  PROXY_POOL_MCP_PORT   - MCP HTTP port (default: 9000)
  PROXY_POOL_GIT_HASH   - expected git hash (default: auto-detect from git)
"""

import os

# ---------------------------------------------------------------------------
# Connection settings
# ---------------------------------------------------------------------------
HOST = os.getenv("PROXY_POOL_HOST", "100.64.0.2")
API_PORT = int(os.getenv("PROXY_POOL_API_PORT", "8000"))
GW_PORT = int(os.getenv("PROXY_POOL_GW_PORT", "9080"))
MCP_PORT = int(os.getenv("PROXY_POOL_MCP_PORT", "9000"))

API_BASE = f"http://{HOST}:{API_PORT}"
GW_ADDR = f"{HOST}:{GW_PORT}"
MCP_BASE = f"http://{HOST}:{MCP_PORT}/mcp"

# ---------------------------------------------------------------------------
# Expected values
# ---------------------------------------------------------------------------
EXPECTED_GIT_HASH = os.getenv("PROXY_POOL_GIT_HASH", "")

# Domains for routing tests
DOMESTIC_DOMAIN = "baidu.com"
OVERSEAS_DOMAIN = "google.com"

# IP echo services
IP_ECHO_URL = "http://ifconfig.me/ip"
IP_ECHO_HTTPS = "https://ifconfig.me/ip"

# Timeouts
DEFAULT_TIMEOUT = 15
CONNECT_TIMEOUT = 10
