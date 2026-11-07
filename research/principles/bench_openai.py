#!/usr/bin/env python3
"""Measure any OpenAI-compatible chat endpoint (Parallax, EXO, llama-server). Python 3 stdlib only.

Reports per request: time to first token (TTFT), decode tokens/sec, total tokens. With --concurrency N it sends N
requests at once and reports aggregate tokens/sec.

Examples:
  python3 bench_openai.py --url http://localhost:3001/v1/chat/completions --model Qwen/Qwen3.6-35B-A3B
  python3 bench_openai.py --url http://localhost:52415/v1/chat/completions --model mlx-community/Qwen3.6-35B-A3B-4bit
  python3 bench_openai.py ... --prompt-tokens 1000 --concurrency 4 --runs 3 --out results.csv --label parallax-3mac
"""
import argparse, csv, json, os, threading, time, urllib.request

FILLER = ("The client file lists a 30-year fixed mortgage, two prior addresses, income from salary and rental "
          "property, and a credit report pulled last month. ")


def make_prompt(n_tokens):
    # ~25 tokens per FILLER sentence; close enough for TTFT scaling
    body = FILLER * max(1, n_tokens // 25)
    return body + "\nSummarize the file above in 5 bullet points, then write a 200-word note to the client."


def one_request(args, prompt, results, idx):
    payload = {
        "model": args.model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": args.max_tokens,
        "temperature": 0.7,
        "stream": True,
        "stream_options": {"include_usage": True},
        "chat_template_kwargs": {"enable_thinking": False},
    }
    req = urllib.request.Request(args.url, data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    t0 = time.time(); t_first = None; chunks = 0; usage_tokens = None; err = ""
    try:
        with urllib.request.urlopen(req, timeout=args.timeout) as r:
            for raw in r:
                line = raw.decode("utf-8", "ignore").strip()
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if data == "[DONE]":
                    break
                try:
                    obj = json.loads(data)
                except ValueError:
                    continue
                if obj.get("usage") and obj["usage"].get("completion_tokens"):
                    usage_tokens = obj["usage"]["completion_tokens"]
                for ch in obj.get("choices") or []:
                    delta = ch.get("delta") or {}
                    if delta.get("content") or delta.get("reasoning_content"):
                        if t_first is None:
                            t_first = time.time()
                        chunks += 1
    except Exception as e:  # record failures, they are part of the result
        err = repr(e)[:200]
    t_end = time.time()
    n = usage_tokens or chunks
    decode_s = (t_end - t_first) if t_first else 0
    results[idx] = {
        "ttft_s": round(t_first - t0, 2) if t_first else None,
        "decode_tps": round((n - 1) / decode_s, 2) if decode_s > 0 and n > 1 else None,
        "tokens": n, "total_s": round(t_end - t0, 2), "error": err,
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", required=True)
    p.add_argument("--model", required=True)
    p.add_argument("--prompt-tokens", type=int, default=200)
    p.add_argument("--max-tokens", type=int, default=256)
    p.add_argument("--concurrency", type=int, default=1)
    p.add_argument("--runs", type=int, default=3)
    p.add_argument("--timeout", type=int, default=600)
    p.add_argument("--label", default="")
    p.add_argument("--out", default="")
    args = p.parse_args()
    prompt = make_prompt(args.prompt_tokens)
    rows = []
    for run in range(args.runs):
        results = [None] * args.concurrency
        threads = [threading.Thread(target=one_request, args=(args, prompt, results, i)) for i in range(args.concurrency)]
        t0 = time.time()
        for t in threads: t.start()
        for t in threads: t.join()
        wall = time.time() - t0
        total_tokens = sum(r["tokens"] for r in results if r and not r["error"])
        agg = round(total_tokens / wall, 2) if wall else 0
        for i, r in enumerate(results):
            r.update({"label": args.label, "run": run, "slot": i, "concurrency": args.concurrency,
                      "prompt_tokens": args.prompt_tokens, "aggregate_tps": agg})
            rows.append(r)
            print(json.dumps(r))
        print(f"run {run}: concurrency={args.concurrency} aggregate={agg} tok/s wall={wall:.1f}s")
    ok = [r for r in rows if not r["error"] and r["decode_tps"]]
    if ok:
        med = sorted(r["decode_tps"] for r in ok)[len(ok) // 2]
        ttft = sorted(r["ttft_s"] for r in ok)[len(ok) // 2]
        print(f"SUMMARY {args.label}: ok={len(ok)}/{len(rows)} median decode={med} tok/s median TTFT={ttft}s")
    else:
        print(f"SUMMARY {args.label}: all {len(rows)} requests failed")
    if args.out:
        new = not os.path.exists(args.out)
        with open(args.out, "a", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            if new: w.writeheader()
            w.writerows(rows)


if __name__ == "__main__":
    main()
