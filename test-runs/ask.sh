#!/bin/sh
# ask.sh <host:port> [label]: one chat request, prints tok/s and the answer (one line) for the log.
curl -s -m 120 http://$1/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"Explain in two sentences why pooling laptops helps a small office."}],"max_tokens":128,"cache_prompt":false}' |
python3 -c '
import json,sys
d=json.load(sys.stdin); t=d["timings"]; a=d["choices"][0]["message"]["content"].replace("\n"," ")
print("%s: gen %.1f tok/s (%d tok), prompt %.0f tok/s | %r" % (sys.argv[1], t["predicted_per_second"], t["predicted_n"], t["prompt_per_second"], a[:160]))' "${2:-ask}"
