#!/usr/bin/env python3
"""Hello World plugin — demonstrates the Arimalo plugin interface.

Reads config from stdin, prints a greeting, and writes nothing.
Use this as a starting point for your own plugins.
"""

import json
import sys

ctx = json.load(sys.stdin)
config = ctx.get("config", {})

greeting = config.get("greeting", "Hello")
repeat = config.get("repeat", 3)

print(f"Plugin dir: {ctx['plugin_dir']}", file=sys.stderr)
print(f"Sources dir: {ctx['sources_dir']}", file=sys.stderr)
print(f"Data dir: {ctx['data_dir']}", file=sys.stderr)

for i in range(repeat):
    print(f"{greeting} from Arimalo plugin! (#{i + 1})", file=sys.stderr)

result = {
    "files_written": [],
    "records_fetched": 0,
    "warnings": [],
}
print(json.dumps(result))
