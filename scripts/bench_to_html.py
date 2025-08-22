#!/usr/bin/env python3
import sys, json, os, html

TEMPLATE_HEAD = """
<!doctype html>
<meta charset="utf-8"/>
<title>ParXive Bench Summary</title>
<style>
 body{font-family:system-ui,Segoe UI,Arial,sans-serif;margin:20px}
 table{border-collapse:collapse;width:100%}
 th,td{border:1px solid #ddd;padding:6px 8px;font-size:14px}
 th{background:#f0f0f0;position:sticky;top:0}
 .ok{color:#0a0}
 .bad{color:#a00}
 .mono{font-family:ui-monospace,Consolas,monospace}
 .small{font-size:12px;color:#555}
</style>
<h1>ParXive Bench Summary</h1>
<div class="small">Input: {src}</div>
<h2>Environments</h2>
<div>
{env_blocks}
</div>
"""

TEMPLATE_HOST_HEAD = """
<h2>Results: {host}</h2>
<table>
<thead><tr>
 <th>Scenario</th><th>GPU</th><th>K</th><th>Parity%</th><th>Chunk</th><th>Interleave</th>
 <th>Total MiB</th><th>Parity MiB</th><th>Encode ms</th><th>Repair ms</th><th>Repaired</th><th>Failed</th><th>OK</th>
</tr></thead>
<tbody>
"""

TEMPLATE_TABLE_FOOT = """
</tbody>
</table>
"""

def human_mib(x):
    try:
        return f"{(int(x)/(1024*1024)):.1f}"
    except Exception:
        return ""

def main():
    if len(sys.argv) < 3:
        print("usage: bench_to_html.py <input.jsonl> <output.html>")
        return 2
    src = sys.argv[1]
    outp = sys.argv[2]
    metas = []
    rows = []
    with open(src) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            t = obj.get('type')
            if t == 'meta':
                metas.append(obj)
            elif t == 'result' or t is None:
                rows.append(obj)
    # Build env blocks
    env_blocks = []
    for m in metas:
        env_blocks.append('<pre class="mono">{}</pre>'.format(html.escape(json.dumps(m, indent=2))))
    with open(outp, 'w') as w:
        # Avoid str.format on TEMPLATE_HEAD because CSS braces conflict with formatting.
        head = TEMPLATE_HEAD.replace('{src}', html.escape(src)).replace('{env_blocks}', '\n'.join(env_blocks))
        w.write(head)
        # Group rows by host (from corresponding meta by source) or by r['source']
        # Build source->host map
        source_to_host = {}
        for m in metas:
            srcname = m.get('source', '')
            host = m.get('host', srcname)
            source_to_host[srcname] = host
        # Group
        groups = {}
        for r in rows:
            srcname = r.get('source', '')
            host = source_to_host.get(srcname, srcname)
            groups.setdefault(host, []).append(r)
        for host in sorted(groups.keys()):
            w.write(TEMPLATE_HOST_HEAD.format(host=html.escape(str(host))))
            for r in groups[host]:
                ok = r.get('ok')
                w.write('<tr>')
                w.write('<td>{}</td>'.format(html.escape(str(r.get('scenario','')))))
                w.write('<td>{}</td>'.format(html.escape(str(r.get('gpu','')) or str(r.get('gpu_mode','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('k','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('parity_pct','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('chunk','')))))
                w.write('<td>{}</td>'.format(html.escape(str(r.get('interleave','')))))
                w.write('<td class="mono">{}</td>'.format(human_mib(r.get('total_bytes',0))))
                w.write('<td class="mono">{}</td>'.format(human_mib(r.get('parity_bytes',0))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('encode_ms','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('repair_ms','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('repaired_chunks','')))))
                w.write('<td class="mono">{}</td>'.format(html.escape(str(r.get('failed_chunks','')))))
                cls = 'ok' if ok else 'bad'
                w.write('<td class="{}">{}</td>'.format(cls, html.escape(str(ok))))
                w.write('</tr>\n')
            w.write(TEMPLATE_TABLE_FOOT)
    print(f"Wrote {outp}")

if __name__ == '__main__':
    sys.exit(main())
