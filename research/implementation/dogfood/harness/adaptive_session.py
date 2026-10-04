#!/usr/bin/env python3
"""Operator validation driver. Every coding operation is a real safe MCP call."""
import json
import hashlib
import os
import sys
from pathlib import Path

from mcp_dogfood_driver import MCPClient

binary, registry, trace_path = sys.argv[1:]
env = {"PATH": "/opt/homebrew/bin:/usr/bin:/bin", "HOME": os.environ["HOME"]}
client = MCPClient([binary, "serve", "--profile", "chatgpt-safe", "--registry", registry], env)
try:
    print(json.dumps(client.request("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "sol-adaptive-validation", "version": "1"}})), flush=True)
    with Path(trace_path).open("w") as trace:
        source = Path('/Users/songshiyao/Desktop/Projects/webcodex/crates/webcodex-chatgpt-safe/src/main.rs')
        trace.write(json.dumps({'context': {'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'registry': registry, 'client': 'Codex MCP stdio adapter; ChatGPT Web NOT_RUN'}}) + '\n')
        trace.flush()
        for line in sys.stdin:
            request = json.loads(line)
            response = client.request("tools/call", request)
            trace.write(json.dumps({"request": request, "response": response}) + "\n")
            trace.flush()
            print(json.dumps(response), flush=True)
finally:
    client.close()
