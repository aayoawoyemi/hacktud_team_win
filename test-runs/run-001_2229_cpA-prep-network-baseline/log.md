# run-001: Checkpoint A prep: network scan + solo baseline (M5 main, qwen 0.5B) — Sat Nov 07 22:29 CST

## 22:29:37 network on this Mac
lo0: 127.0.0.1
en0: 10.189.147.5
en7: 192.168.8.174

## 22:29:51 LAN scan 192.168.8.0/24 (from en7)
up 192.168.8.1
up 192.168.8.113
up 192.168.8.174
up 192.168.8.204
? (192.168.8.1) at 94:83:c4:92:26:a5 on en7 ifscope [ethernet]
? (192.168.8.113) at 0:5:1b:66:79:84 on en7 ifscope [ethernet]
? (192.168.8.174) at 0:5:1b:63:c9:26 on en7 ifscope permanent [ethernet]
? (192.168.8.204) at 0:5:1b:66:7a:18 on en7 ifscope [ethernet]
? (192.168.8.255) at ff:ff:ff:ff:ff:ff on en7 ifscope [ethernet]
? (224.0.0.251) at 1:0:5e:0:0:fb on en7 ifscope permanent [ethernet]

## 22:29:59 port probe + RTT
192.168.8.113:22 OPEN
192.168.8.113 round-trip min/avg/max/stddev = 0.549/0.716/0.944/0.150 ms
192.168.8.204 round-trip min/avg/max/stddev = 0.506/0.608/0.756/0.091 ms
Browsing for _ssh._tcp.local.
DATE: ---Sat 07 Nov 2026---
22:30:12.738  ...STARTING...
Timestamp     A/R    Flags  if Domain               Service Type         Instance Name
22:30:12.842  Add        2  34 local.               _ssh._tcp.           Preston’s MacBook Air
192.168.8.113 name: ;; connection timed out; no servers could be reached
192.168.8.204 name: ;; connection timed out; no servers could be reached

## 22:31:02 step 1: versions/devices on main (this M5)
```
ggml_metal_device_init: testing tensor API for f16 support
ggml_metal_library_init_from_source: error compiling source
ggml_metal_device_init: - the tensor API is not supported in this environment - disabling
ggml_metal_library_init: using embedded metal library
ggml_metal_library_init: loaded in 0.019 sec
ggml_metal_rsets_init: creating a residency set collection (keep_alive = 180 s)
ggml_metal_device_init: GPU name:   MTL0
ggml_metal_device_init: GPU family: MTLGPUFamilyApple10  (1010)
ggml_metal_device_init: GPU family: MTLGPUFamilyCommon3 (3003)
ggml_metal_device_init: GPU family: MTLGPUFamilyMetal4  (5002)
ggml_metal_device_init: simdgroup reduction   = true
ggml_metal_device_init: simdgroup matrix mul. = true
ggml_metal_device_init: has unified memory    = true
ggml_metal_device_init: has bfloat            = true
ggml_metal_device_init: has tensor            = false
ggml_metal_device_init: use residency sets    = true
ggml_metal_device_init: use shared buffers    = true
ggml_metal_device_init: recommendedMaxWorkingSetSize  = 19069.67 MB
version: 8665 (b8635075f)
built with AppleClang 15.0.0.15000309 for Darwin arm64
ggml_metal_device_init: testing tensor API for f16 support
ggml_metal_library_init_from_source: error compiling source
ggml_metal_device_init: - the tensor API is not supported in this environment - disabling
ggml_metal_library_init: using embedded metal library
ggml_metal_library_init: loaded in 0.010 sec
ggml_metal_rsets_init: creating a residency set collection (keep_alive = 180 s)
ggml_metal_device_init: GPU name:   MTL0
ggml_metal_device_init: GPU family: MTLGPUFamilyApple10  (1010)
ggml_metal_device_init: GPU family: MTLGPUFamilyCommon3 (3003)
ggml_metal_device_init: GPU family: MTLGPUFamilyMetal4  (5002)
ggml_metal_device_init: simdgroup reduction   = true
ggml_metal_device_init: simdgroup matrix mul. = true
ggml_metal_device_init: has unified memory    = true
ggml_metal_device_init: has bfloat            = true
ggml_metal_device_init: has tensor            = false
ggml_metal_device_init: use residency sets    = true
ggml_metal_device_init: use shared buffers    = true
ggml_metal_device_init: recommendedMaxWorkingSetSize  = 19069.67 MB
Available devices:
  MTL0: Apple M5 (18186 MiB, 18185 MiB free)
  BLAS: Accelerate (0 MiB, 0 MiB free)
model: /private/tmp/claude-501/-Users-preston-Documents-code-hackathon/9edc05e3-cb6d-4fb9-b792-966e20abd721/scratchpad/models/qwen2.5-0.5b-instruct-q4_k_m.gguf
491400032 bytes
74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db  /private/tmp/claude-501/-Users-preston-Documents-code-hackathon/9edc05e3-cb6d-4fb9-b792-966e20abd721/scratchpad/models/qwen2.5-0.5b-instruct-q4_k_m.gguf
```
## 22:31:04 step 2: model alone on main (baseline)
- solo llama-server stopped; full log: step2-solo.log
- (rerun: first attempt's result parser had a Python syntax error, no data lost)
- solo run 1: gen 33.0 tok/s (37 tok), prompt 363 tok/s | 'Pooling laptops can help a small office by reducing the number of individual computers, reducing maintenance and repair costs, and increasing overall efficiency'
- solo run 2: gen 37.9 tok/s (33 tok), prompt 496 tok/s | 'Pooling laptops in a small office can significantly reduce costs and increase efficiency by allowing multiple users to work on shared systems simultaneously, th'
- solo run 3: gen 52.9 tok/s (29 tok), prompt 537 tok/s | 'Pooling laptops can help a small office by reducing the load on individual laptops, improving productivity, and reducing the need for frequent replacements or u'
- solo llama-server stopped (22:31:20); full log: step2-solo.log

## 22:31:32 dashboard
- python3 http.server 0.0.0.0:8000 serving checkpoint-a-run/ (test copy of PR #32 page + real IPs, ?direct). pid 66772
- URL: http://localhost:8000/web/?direct (from other Macs: http://192.168.8.174:8000/web/?direct)

## 22:31:32 END of run-001 (helpers not started yet; continues in run-002)
