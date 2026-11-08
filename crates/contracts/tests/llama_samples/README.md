# llama.cpp output samples for the parser tests

- `*.dell-8665.txt`: the three samples from PRD WP-2 "Parser samples", captured on the Dell (Windows,
  llama.cpp build 8665) at 3:50pm. Copied byte for byte.
- The tests in `tests/parsers.rs` also prepend `load_backend:` and `ggml_vulkan:` noise lines to these
  samples. Those noise lines are written to match llama.cpp's log format but were not captured from a
  machine. Replace them with real captures once WP-1 lands `fixtures/llama/` (owned by WP-1).
- A Mac (Metal) sample is still missing; WP-2 acceptance needs one (VERIFY: Metal device names `MTL0`).
