"""MCP Streamable HTTP transport client for integration tests."""

import json
import uuid
import httpx


class McpClient:
    """Minimal MCP client over Streamable HTTP (SSE) transport.

    Maintains session state across requests using the mcp-session-id header.
    """

    def __init__(self, base_url: str, timeout: float = 15.0):
        self._url = base_url
        self._client = httpx.Client(timeout=timeout)
        self._session_id: str | None = None
        self._initialized = False

    def initialize(self) -> dict:
        """Send MCP initialize request and store session ID."""
        result, session_id = self._call("initialize", {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "integration-test", "version": "1.0"},
        }, return_session_id=True)
        if session_id:
            self._session_id = session_id
        # Send initialized notification
        self._notify("notifications/initialized")
        self._initialized = True
        return result

    def call_tool(self, tool_name: str, arguments: dict | None = None) -> dict:
        """Call an MCP tool by name."""
        if not self._initialized:
            self.initialize()
        params = {"name": tool_name}
        if arguments is not None:
            params["arguments"] = arguments
        result, _ = self._call("tools/call", params, return_session_id=True)
        return result

    def list_tools(self) -> dict:
        """List available MCP tools."""
        if not self._initialized:
            self.initialize()
        result, _ = self._call("tools/list", {}, return_session_id=True)
        return result

    def _notify(self, method: str) -> None:
        """Send a JSON-RPC notification (no id, no response expected)."""
        payload = {
            "jsonrpc": "2.0",
            "method": method,
        }
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self._session_id:
            headers["mcp-session-id"] = self._session_id
        resp = self._client.post(self._url, json=payload, headers=headers)
        # Update session ID from response if provided
        new_session = resp.headers.get("mcp-session-id")
        if new_session:
            self._session_id = new_session

    def _call(self, method: str, params: dict, return_session_id: bool = False) -> dict | tuple:
        """Send a JSON-RPC request and parse the SSE response."""
        request_id = str(uuid.uuid4())
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": request_id,
        }
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self._session_id:
            headers["mcp-session-id"] = self._session_id

        resp = self._client.post(self._url, json=payload, headers=headers)
        resp.raise_for_status()

        # Capture session ID from response headers
        new_session_id = resp.headers.get("mcp-session-id") or self._session_id

        # Parse SSE stream - look for the JSON-RPC response
        for line in resp.text.split("\n"):
            line = line.strip()
            if line.startswith("data: "):
                data_str = line[6:]
                if not data_str:
                    continue
                try:
                    data = json.loads(data_str)
                    if data.get("id") == request_id:
                        if "error" in data:
                            raise RuntimeError(f"MCP error: {data['error']}")
                        result = data.get("result", {})
                        if return_session_id:
                            return result, new_session_id
                        return result
                except json.JSONDecodeError:
                    continue

        raise RuntimeError(f"No response found for request {request_id}")

    def close(self):
        self._client.close()
