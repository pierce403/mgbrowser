#!/usr/bin/env python3
"""Check specified-style behavior in the supplied installed restricted worker."""
import json
import subprocess
import sys

request = {"url": "https://example.test/installed-cssom", "html": """
<!doctype html><html><head><title>CSSOM fallback</title></head><body><p id="proof"></p>
<script>
const proof = document.getElementById('proof');
const style = proof.style;
style.cssText = 'width: 12px; margin: 1px 2px';
style.width = 'invalid';
if (style.getPropertyValue('width') !== '12px' || style.marginLeft !== '2px')
  throw Error('installed CSSOM parsing or invalid-write behavior failed');
style.width = '27px';
if (style.getPropertyValue('width') !== '27px') throw Error('installed CSSOM mutation failed');
proof.setAttribute('data-cssom', 'verified'); document.title = 'Installed CSSOM ready';
</script></body></html>"""}
result = subprocess.run([sys.argv[1], "--script-worker"], env={},
                        input=json.dumps(request).encode(), stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, check=True, timeout=5)
assert len(result.stdout) <= 4 * 1024 * 1024 and len(result.stderr) <= 65536
reply = json.loads(result.stdout)
assert reply["applied"] and not reply["errors"] and reply["scripts_executed"] == 1, reply
assert 'data-cssom="verified"' in reply["html"] and 'Installed CSSOM ready' in reply["html"], reply
assert reply["boa"]["profile"] == 'boa-page-process-v1' and reply["boa"]["fatal_reason"] is None
assert reply["boa"]["worker_memory"]["cumulative_bytes"] > 0 and reply["allocations"] is None
print('INSTALLED_CSSOM_OK', flush=True)
