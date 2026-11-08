# run-002: Checkpoint A, 0.5B model split across 3 wired Macs — started Sat Nov 07 22:33:51 CST

Setup: main = Preston's M5 192.168.8.174 (llama-server, model qwen2.5-0.5b-instruct-q4_k_m, sha256 74a4da8c…a9db).
Helpers: 192.168.8.113, 192.168.8.204 (rpc-server -d MTL0 -p 50052 -c, started by Preston). llama.cpp b8665 (b8635075f).
Network: GL.iNet Opal + switch, USB-C gigabit, RTT 0.6-0.7 ms (run-001). Dashboard: http://localhost:8000/web/?direct
Goal (PRD §8): one answer through one endpoint with layers on RPC0, RPC1 and MTL0.

## Steps
- 22:33:51 WAITING for rpc-server on .113:50052 and .204:50052
- 22:33:59 dashboard server restarted after folder rename (was 404 for ~1 min; reload the page)
- 22:34:12 192.168.8.204:50052 OPEN (rpc-server up)
- 22:35:09 posted rpc-server instructions to P4 + P5 in #11
- 22:36:01 #11 P5: **.204 = P5's M5 Air** (en7, 1000baseT full-duplex). rpc-server already running: v3.6.1, endpoint 192.168.8.204:50052, MTL0 Apple M5 18186 MiB (18185 free), cache ~/Library/Caches/llama.cpp/rpc/, no BLAS exposed. Also netcheck serve on .204:50060.
- 22:36:10 #11 P4: **.113 = P4's M3 Air (the usual main)**, en7 1000baseT full-duplex. rpc-server b8665 at /Users/preston/Documents/hackathon/p4-runtime/llama/llama-b8665/rpc-server. P4 prepared start-helper.command (-d MTL0 -H 192.168.8.113 -p 50052 -c + caffeinate); needs Preston to run it in a normal Terminal on the Air. Not up yet.
- 22:3x .204:50052 OPEN (seen by watcher); .113:50052 still closed
- 22:43:13 posted in #11: P4 to write ~/Desktop/start-helper.md on the Air (helper command for .113)
- 22:46:47 192.168.8.113:50052 OPEN (rpc-server up)

## 22:46:57 step 4: launch pool (load 1, cold)
```
/Users/preston/llama-b8665/llama-b8665/llama-server -m /private/tmp/claude-501/-Users-preston-Documents-code-hackathon/9edc05e3-cb6d-4fb9-b792-966e20abd721/scratchpad/models/qwen2.5-0.5b-instruct-q4_k_m.gguf --rpc 192.168.8.113:50052,192.168.8.204:50052 -ngl 99 -c 4096 --host 0.0.0.0 --port 8080
```
- 22:46:57 load-1 ready in 0.1 s (health ok: )
- buffers (load-1.log):
- 22:47:06 ❌ attempt 1 never ran: my zsh launch line passed the whole command as one filename ('No such file or directory'). Not a pool failure. Retrying.
- 22:47:11 load-1 (cold): {"status":"ok"} after 4.3 s
- buffers + devices (load-1.log):
    ggml_metal_library_init_from_source: error compiling source
    llama_params_fit_impl:   - RPC0 (192.168.8.113:50052):  12124 total,    130 used,  11992 free vs. target of   1024
    llama_params_fit_impl:   - RPC1 (192.168.8.204:50052):  18186 total,    144 used,  18041 free vs. target of   1024
    llama_model_load_from_file_impl: using device RPC0 (192.168.8.113:50052) (unknown id) - 12123 MiB free
    llama_model_load_from_file_impl: using device RPC1 (192.168.8.204:50052) (unknown id) - 18185 MiB free
    llama_model_load_from_file_impl: using device MTL0 (Apple M5) (unknown id) - 18185 MiB free
    load_tensors: offloading output layer to GPU
    load_tensors: offloading 23 repeating layers to GPU
    load_tensors: offloaded 25/25 layers to GPU
    load_tensors:   CPU_Mapped model buffer size =    89.26 MiB
    load_tensors:  MTL0_Mapped model buffer size =   462.96 MiB
    load_tensors: RPC0[192.168.8.113:50052] model buffer size =    69.32 MiB
    load_tensors: RPC0[192.168.8.204:50052] model buffer size =    88.97 MiB
    llama_context:        CPU  output buffer size =     2.32 MiB
    llama_kv_cache:       MTL0 KV buffer size =    16.00 MiB
    llama_kv_cache: RPC0[192.168.8.113:50052] KV buffer size =    14.00 MiB
    llama_kv_cache: RPC0[192.168.8.204:50052] KV buffer size =    18.00 MiB
    sched_reserve: RPC0[192.168.8.113:50052] compute buffer size =    47.51 MiB
    sched_reserve: RPC0[192.168.8.204:50052] compute buffer size =    36.01 MiB
    sched_reserve:       MTL0 compute buffer size =   300.25 MiB
    sched_reserve:        CPU compute buffer size =    11.51 MiB

## 22:47:19 layer split (from KV buffer sizes: 2 MiB per layer at -c 4096)
- .113 M3 Air RPC0: 69.3 MiB weights, 14 MiB KV = 7 layers
- .204 M5 Air RPC1: 89.0 MiB weights, 18 MiB KV = 9 layers
- M5 main MTL0: 16 MiB KV = 8 layers (+ output layer); CPU holds token_embd (89 MiB)
- llama.cpp default split by free memory (no -ts). **Model split across 3 machines ✅**

## 22:47:19 step 5: ask over the LAN (endpoint http://192.168.8.174:8080)
- pool run 1: gen 30.7 tok/s (65 tok), prompt 74 tok/s | 'Pooling laptops at a small office can significantly reduce the cost of maintaining and updating individual laptops, as one person owns multiple laptops and can '
- pool run 2: gen 44.6 tok/s (20 tok), prompt 382 tok/s | 'Pooling laptops in a small office can help streamline work processes, reduce costs, and improve efficiency.'
- pool run 3: gen 33.8 tok/s (27 tok), prompt 346 tok/s | 'Pooling laptops can significantly enhance productivity by allowing multiple employees to work together, sharing resources and workload, improving communication,'
- Metal 'error compiling source' line = tensor-API probe, then 'not supported, disabling' + embedded library: harmless.

## 22:47:34 step 6: load 2 (helpers cached with -c)
- 22:47:34 load-1 llama-server stopped
- 192.168.8.113:50052 still open after main disconnect ✅
- 192.168.8.204:50052 still open after main disconnect ✅
- 22:47:38 load-2: {"status":"ok"} after 3.2 s
    load: special tokens cache size = 23
    load: token to piece cache size = 0.9310 MB
    load_tensors:   CPU_Mapped model buffer size =    89.26 MiB
    load_tensors:  MTL0_Mapped model buffer size =   462.96 MiB
    load_tensors: RPC0[192.168.8.113:50052] model buffer size =    69.32 MiB
    load_tensors: RPC0[192.168.8.204:50052] model buffer size =    88.97 MiB
    llama_kv_cache:       MTL0 KV buffer size =    16.00 MiB
    llama_kv_cache: RPC0[192.168.8.113:50052] KV buffer size =    14.00 MiB
- pool load-2 run: gen 37.1 tok/s (41 tok), prompt 419 tok/s | 'Pooling laptops in a small office can significantly reduce costs, increase productivity, and improve communication by centralizing resources and facilitating co'

## 22:47:49 RESULT: ✅ Checkpoint A reached (0.5B model)
- One endpoint (http://192.168.8.174:8080) answered 4/4 requests with layers on 3 wired Macs (.113 7 layers, .204 9, M5 8 + output).
- Load: cold 4.3 s, second 3.2 s (model is tiny: ~160 MiB to helpers; the cache matters for Gemma, not here).
- Pool gen 31-45 tok/s vs solo M5 33-53 tok/s (same prompt). Expected: a 0.5B model fits on one Mac, so the pool only adds network hops. Speed claims need Part 2.
- Helpers keep serving after main disconnects (ports stayed open between loads).
- Mistakes (mine, not the pool's): zsh word-splitting broke the first launch line; dashboard server 404 for ~1 min after folder rename.
- Left running: llama-server (load-2, pid 70000) for the dashboard; helpers on .113 and .204; dashboard on :8000.
