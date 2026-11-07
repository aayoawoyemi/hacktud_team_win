#!/usr/bin/env python3
"""Check that an OpenAI-compatible endpoint (EXO, llama-server) returns valid tool calls for a model. Stdlib only.

Runs N trials of a 2-step tool loop:
  1. ask a question that needs the `read_file` tool -> expect finish_reason "tool_calls" with valid JSON arguments
  2. send the tool result back -> expect a final text answer that uses it
Prints pass count and median latency per step.

  python3 tool_test.py --url http://localhost:52415/v1/chat/completions \
      --model mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit --trials 10
"""
import argparse, json, time, urllib.request

TOOLS = [{
    "type": "function",
    "function": {
        "name": "read_file",
        "description": "Read a source file from the legacy codebase and return its text.",
        "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
    },
}]
COBOL = """       IDENTIFICATION DIVISION.
       PROGRAM-ID. INTCALC.
       PROCEDURE DIVISION.
           COMPUTE WS-INTEREST = WS-PRINCIPAL * WS-RATE / 1200.
           DISPLAY WS-INTEREST.
           STOP RUN."""


def post(url, payload, timeout):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read()), time.time() - t0


def trial(args):
    msgs = [{"role": "system", "content": "You modernize legacy code. Use tools to read files before answering."},
            {"role": "user", "content": "What does src/INTCALC.cbl compute? Read it first."}]
    base = {"model": args.model, "tools": TOOLS, "max_tokens": args.max_tokens, "stream": False,
            "chat_template_kwargs": {"enable_thinking": args.thinking}}
    r1, t1 = post(args.url, {**base, "messages": msgs}, args.timeout)
    m1 = r1["choices"][0]["message"]
    calls = m1.get("tool_calls") or []
    if not calls:
        return False, t1, None, "no tool_calls; got: " + (m1.get("content") or "")[:120]
    call = calls[0]
    try:
        path = json.loads(call["function"]["arguments"])["path"]
    except Exception as e:
        return False, t1, None, f"bad arguments {call['function'].get('arguments')!r}: {e}"
    msgs += [m1, {"role": "tool", "tool_call_id": call.get("id", "0"), "content": COBOL}]
    r2, t2 = post(args.url, {**base, "messages": msgs}, args.timeout)
    text = (r2["choices"][0]["message"].get("content") or "")
    ok = "interest" in text.lower()
    return ok, t1, t2, f"path={path!r} answer={text[:100]!r}"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", required=True)
    p.add_argument("--model", required=True)
    p.add_argument("--trials", type=int, default=10)
    p.add_argument("--max-tokens", type=int, default=512)
    p.add_argument("--timeout", type=int, default=300)
    p.add_argument("--thinking", action="store_true", help="enable_thinking=true in the chat template")
    args = p.parse_args()
    passed, t1s, t2s = 0, [], []
    for i in range(args.trials):
        try:
            ok, t1, t2, note = trial(args)
        except Exception as e:
            ok, t1, t2, note = False, None, None, repr(e)[:160]
        passed += ok
        if t1: t1s.append(t1)
        if t2: t2s.append(t2)
        print(f"trial {i}: {'PASS' if ok else 'FAIL'} step1={t1 and round(t1,1)}s step2={t2 and round(t2,1)}s {note}")
    med = lambda xs: round(sorted(xs)[len(xs) // 2], 1) if xs else None
    print(f"SUMMARY tool loop: {passed}/{args.trials} passed; median step1={med(t1s)}s step2={med(t2s)}s")


if __name__ == "__main__":
    main()
