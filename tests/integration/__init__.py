"""Integration test runner entry point.

Usage:
  # Run all integration tests
  pytest tests/integration/ -v

  # Run specific layer
  pytest tests/integration/test_l1_health.py -v
  pytest tests/integration/test_l2_api.py -v
  pytest tests/integration/test_l3_routing.py -v
  pytest tests/integration/test_l4_mcp.py -v

  # Run with custom server
  PROXY_POOL_HOST=192.168.1.100 pytest tests/integration/ -v

  # Run slow tests (fallback/fault injection)
  pytest tests/integration/ -v -m slow

  # Skip slow tests (default)
  pytest tests/integration/ -v -m "not slow"

Dependencies:
  pip install pytest requests httpx httpx[socks]
"""
