#!/usr/bin/env python3
import sys, json, os, time

def read_jsonl(path):
    with open(path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except Exception:
                continue

def main():
    if len(sys.argv) < 3:
        print("usage: bench_collate.py <output.jsonl> <input1.jsonl> [input2.jsonl ...]")
        return 2
    outp = sys.argv[1]
    inputs = sys.argv[2:]
    ts = int(time.time())
    with open(outp, 'w') as w:
        for src in inputs:
            meta_emitted = False
            for obj in read_jsonl(src):
                t = obj.get('type')
                obj['source'] = src
                if t == 'meta' and not meta_emitted:
                    # emit only the first meta per source
                    w.write(json.dumps(obj) + "\n")
                    meta_emitted = True
                elif t == 'result' or t is None:
                    w.write(json.dumps(obj) + "\n")
            if not meta_emitted:
                # no meta found; emit minimal one
                w.write(json.dumps({
                    'type':'meta',
                    'host':'unknown',
                    'source': src,
                    'ts': ts
                }) + "\n")
    print(f"Wrote {outp}")

if __name__ == '__main__':
    sys.exit(main())

