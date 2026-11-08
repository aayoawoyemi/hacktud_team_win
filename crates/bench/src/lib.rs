//! CONCEPT: bench crate (WP-8). Sends prompts to the pool's OpenAI-style endpoint (llama-server on main), measures
//!          each request (time to first token, generation tokens/s, prompt tokens/s, total time) and writes one
//!          §6.8 BenchResult per request as JSON lines + CSV, plus a markdown summary table.
//! WHY: R8.1-R8.4, R8.6. The demo's numbers (single machine vs pool, D-07 size ratio) must come from a repeatable
//!      runner, not a stopwatch. Checked against llama-server b8665 (Sat 9:05pm): a streamed chat completion is
//!      HTTP/1.1 `Transfer-Encoding: chunked` SSE; with `stream_options.include_usage` the last event carries both
//!      `usage` and llama-server's own `timings` object, and every event carries `system_fingerprint`
//!      ("b8665-b8635075f"), so the build number is recorded without asking anyone (R8 acceptance).
//! DEPENDS ON: contracts (BenchResult, ServerTimings, ErrorInfo), netcheck::result::now_rfc3339, a llama-server
//!      endpoint (the WP-1 manual setup is enough).
//! AFFECTS: WP-13 runbook and WP-14 write-up read the JSONL/CSV/markdown. Scenario ids are free strings (O-13).
//!
//! Not done (Checkpoint C items, skipped on purpose): R8.8 live demo mode (depends on O-10), R8.7 orchestration
//! (the runner does not change the pool; the person running it sets `scenario` to match what is running).

use contracts::{BenchResult, ErrorCode, ErrorInfo, SCHEMA_VERSION, ServerTimings};
use serde::Deserialize;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// =====================================================================================================
// Config and prompts (O-24: one TOML file per role)
// =====================================================================================================

/// CONCEPT: one bench run = one scenario on whatever the pool is serving right now.
/// WHY: O-24 (TOML per role). `scenario`, `model_id`, `nodes`, `plan_id` are labels the runner can't discover
///      itself (the endpoint doesn't say which machines hold layers), so the person running it states them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `http://host:port` of llama-server (plain HTTP only; it's a LAN service, D-02).
    pub endpoint: String,
    pub prompts_file: PathBuf,
    pub output_dir: PathBuf,
    /// §6.8 scenario id (O-13), e.g. `single_fit`, `split_3`, `pool_part1`.
    pub scenario: String,
    pub model_id: String,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub ctx_size: Option<u32>,
    /// Only prompts with this `use` run (R8.3). Default "bench".
    #[serde(default = "default_use")]
    pub prompt_use: String,
    /// Unmeasured runs per prompt before the measured ones (R8.4): loads weights into caches, warms the Air.
    #[serde(default = "default_warmup")]
    pub warmup: u32,
    #[serde(default = "default_repeats")]
    pub repeats: u32,
    /// Per-request socket timeout. A Part 2 first token over RPC can take a while; 600 s is generous on purpose.
    #[serde(default = "default_timeout_s")]
    pub timeout_s: u64,
    /// D-07 "size ratio" for the summary (model bytes / largest single machine's usable bytes, from WP-5).
    /// Optional label: the runner can't compute it.
    #[serde(default)]
    pub size_ratio: Option<f64>,
}

fn default_use() -> String {
    "bench".into()
}
fn default_warmup() -> u32 {
    1
}
fn default_repeats() -> u32 {
    3
}
fn default_timeout_s() -> u64 {
    600
}

/// R8.3: one prompt from `prompts/*.toml` (`[[prompt]]` tables).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub id: String,
    pub text: String,
    pub max_tokens: u32,
    /// "bench" or "demo".
    #[serde(rename = "use")]
    pub use_: String,
}

#[derive(Deserialize)]
struct PromptFile {
    prompt: Vec<Prompt>,
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("E_CONFIG: read {}: {e}", path.display()))?;
    let cfg: Config =
        toml::from_str(&text).map_err(|e| format!("E_CONFIG: {}: {e}", path.display()))?;
    if cfg.repeats == 0 {
        return Err("E_CONFIG: repeats must be at least 1".into());
    }
    parse_endpoint(&cfg.endpoint)?;
    Ok(cfg)
}

/// Loads prompts and keeps the ones whose `use` matches. Empty result is an error: a run that measures nothing
/// would write an empty file that looks like success.
pub fn load_prompts(path: &Path, use_: &str) -> Result<Vec<Prompt>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("E_CONFIG: read {}: {e}", path.display()))?;
    let file: PromptFile =
        toml::from_str(&text).map_err(|e| format!("E_CONFIG: {}: {e}", path.display()))?;
    let picked: Vec<Prompt> = file.prompt.into_iter().filter(|p| p.use_ == use_).collect();
    if picked.is_empty() {
        return Err(format!(
            "E_CONFIG: no prompts with use = \"{use_}\" in {}",
            path.display()
        ));
    }
    Ok(picked)
}

/// `http://host:port[/]` → ("host", port). Only http: TLS would need a crate, and the endpoint is on our LAN.
pub fn parse_endpoint(endpoint: &str) -> Result<(String, u16), String> {
    let rest = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| format!("E_CONFIG: endpoint must start with http:// (got {endpoint})"))?
        .trim_end_matches('/');
    let (host, port) = rest.rsplit_once(':').ok_or_else(|| {
        format!("E_CONFIG: endpoint needs a port, e.g. http://127.0.0.1:8080 (got {endpoint})")
    })?;
    let port: u16 = port
        .parse()
        .map_err(|_| format!("E_CONFIG: bad port in {endpoint}"))?;
    if host.is_empty() || host.contains('/') {
        return Err(format!("E_CONFIG: bad host in {endpoint}"));
    }
    Ok((host.to_string(), port))
}

// =====================================================================================================
// One request (R8.1, R8.2, R8.5)
// =====================================================================================================

/// CONCEPT: what one streamed request measured, before it's labelled into a BenchResult.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Measurement {
    pub ttft_ms: Option<u64>,
    pub total_ms: u64,
    /// Content (or reasoning) deltas seen in the stream. Fallback token count when the server sends no usage.
    pub content_chunks: u32,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub timings: Option<ServerTimings>,
    /// From `system_fingerprint` "b8665-<commit>".
    pub llama_cpp_build: Option<u32>,
    pub text: String,
}

/// R8.1: one streaming chat completion. Errors are returned as text with a §6.11 code prefix; the caller turns
/// them into `BenchResult.error` so a failed request still leaves a line in the output (never silently dropped).
pub fn run_one(endpoint: &str, prompt: &Prompt, timeout: Duration) -> Result<Measurement, String> {
    let (host, port) = parse_endpoint(endpoint)?;
    // WHY cache_prompt=false: llama-server reuses the KV of an identical prompt by default, so repeats 2..N would
    // skip prompt processing and report a near-zero TTFT. Benchmarks must measure the full request every time.
    // temperature 0: same prompt → same output length → comparable repeats.
    let body = serde_json::json!({
        "messages": [{ "role": "user", "content": prompt.text }],
        "max_tokens": prompt.max_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
        "temperature": 0,
        "cache_prompt": false,
    })
    .to_string();

    let start = Instant::now();
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("E_REQUEST_FAILED: resolve {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("E_REQUEST_FAILED: no address for {host}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("E_REQUEST_FAILED: connect {addr}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_nodelay(true).ok();
    let req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("E_REQUEST_FAILED: send: {e}"))?;

    let mut reader = BufReader::new(stream);
    let (status, chunked) = read_head(&mut reader)?;
    if status != 200 {
        let mut rest = String::new();
        let _ = reader.take(2000).read_to_string(&mut rest);
        return Err(format!("E_REQUEST_FAILED: HTTP {status}: {}", rest.trim()));
    }
    let body: Box<dyn Read> = if chunked {
        Box::new(Chunked::new(reader))
    } else {
        Box::new(reader)
    };
    read_sse(BufReader::new(body), start)
}

/// Reads the status line and headers. Returns (status code, body is chunked).
fn read_head(r: &mut impl BufRead) -> Result<(u16, bool), String> {
    let mut line = String::new();
    r.read_line(&mut line)
        .map_err(|e| format!("E_REQUEST_FAILED: read status: {e}"))?;
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("E_REQUEST_FAILED: bad status line {:?}", line.trim()))?;
    let mut chunked = false;
    loop {
        line.clear();
        let n = r
            .read_line(&mut line)
            .map_err(|e| format!("E_REQUEST_FAILED: read headers: {e}"))?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("transfer-encoding")
            && v.trim().eq_ignore_ascii_case("chunked")
        {
            chunked = true;
        }
    }
    Ok((status, chunked))
}

/// Parses the SSE stream line by line, timestamping when each line arrives.
fn read_sse(mut r: impl BufRead, start: Instant) -> Result<Measurement, String> {
    let mut m = Measurement::default();
    let mut line = String::new();
    loop {
        line.clear();
        let n = r
            .read_line(&mut line)
            .map_err(|e| format!("E_REQUEST_FAILED: stream cut: {e}"))?;
        if n == 0 {
            break;
        }
        let Some(data) = line.trim_end().strip_prefix("data:") else {
            continue; // blank separators, comments, `event:` lines
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let v: serde_json::Value = serde_json::from_str(data)
            .map_err(|e| format!("E_REQUEST_FAILED: bad event JSON: {e}"))?;
        if let Some(err) = v.get("error") {
            return Err(format!("E_REQUEST_FAILED: server error: {err}"));
        }
        if m.llama_cpp_build.is_none() {
            m.llama_cpp_build = v["system_fingerprint"]
                .as_str()
                .and_then(build_from_fingerprint);
        }
        // Both count as generated tokens for TTFT: reasoning models (Gemma 4 thinking) stream reasoning first.
        let delta = &v["choices"][0]["delta"];
        for key in ["content", "reasoning_content"] {
            if let Some(s) = delta[key].as_str().filter(|s| !s.is_empty()) {
                m.ttft_ms.get_or_insert(start.elapsed().as_millis() as u64);
                m.content_chunks += 1;
                if key == "content" {
                    m.text.push_str(s);
                }
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            m.prompt_tokens = u["prompt_tokens"].as_u64().map(|x| x as u32);
            m.completion_tokens = u["completion_tokens"].as_u64().map(|x| x as u32);
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            m.timings = Some(ServerTimings {
                prompt_n: t["prompt_n"].as_u64().map(|x| x as u32),
                prompt_ms: t["prompt_ms"].as_f64(),
                prompt_per_token_ms: t["prompt_per_token_ms"].as_f64(),
                prompt_per_second: t["prompt_per_second"].as_f64(),
                predicted_n: t["predicted_n"].as_u64().map(|x| x as u32),
                predicted_ms: t["predicted_ms"].as_f64(),
                predicted_per_token_ms: t["predicted_per_token_ms"].as_f64(),
                predicted_per_second: t["predicted_per_second"].as_f64(),
            });
        }
    }
    m.total_ms = start.elapsed().as_millis() as u64;
    if m.ttft_ms.is_none() && m.completion_tokens.unwrap_or(0) == 0 {
        return Err("E_REQUEST_FAILED: stream ended without any generated token".into());
    }
    Ok(m)
}

/// "b8665-b8635075f" → 8665.
fn build_from_fingerprint(s: &str) -> Option<u32> {
    s.strip_prefix('b')?.split('-').next()?.parse().ok()
}

/// CONCEPT: HTTP/1.1 chunked transfer decoding (RFC 9112 §7.1): `<hex size>[;ext]\r\n<data>\r\n` ... `0\r\n\r\n`.
/// WHY: llama-server streams SSE chunked (seen at b8665). A chunk boundary can fall mid-line, so the SSE parser
///      must read decoded bytes, not raw socket bytes.
struct Chunked<R> {
    inner: R,
    left: usize,
    done: bool,
}

impl<R: BufRead> Chunked<R> {
    fn new(inner: R) -> Self {
        Chunked {
            inner,
            left: 0,
            done: false,
        }
    }
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut size_line = String::new();
            if self.inner.read_line(&mut size_line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "chunked body cut before the last chunk",
                ));
            }
            let hex = size_line.split(';').next().unwrap_or("").trim();
            self.left = usize::from_str_radix(hex, 16).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("bad chunk size {hex:?}"),
                )
            })?;
            if self.left == 0 {
                self.done = true; // trailers, if any, are ignored: Connection: close ends the socket anyway
                return Ok(0);
            }
        }
        let want = buf.len().min(self.left);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunk cut short",
            ));
        }
        self.left -= n;
        if self.left == 0 {
            let mut crlf = [0u8; 2];
            self.inner.read_exact(&mut crlf)?;
        }
        Ok(n)
    }
}

// =====================================================================================================
// Labelling, output files and summary (R8.4, R8.6)
// =====================================================================================================

/// Builds the §6.8 line. Generation tokens/s prefers llama-server's own `predicted_per_second` (measured inside
/// the server, excludes network and prompt time). Fallback: tokens after the first / time after the first token,
/// marked "estimated" in `notes` (R8.2).
pub fn to_result(
    cfg: &Config,
    run_id: &str,
    prompt: &Prompt,
    repeat_index: u32,
    warmup: bool,
    started_at: String,
    outcome: Result<Measurement, String>,
) -> BenchResult {
    let mut r = BenchResult {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        scenario: cfg.scenario.clone(),
        repeat_index,
        warmup,
        started_at,
        model_id: cfg.model_id.clone(),
        plan_id: cfg.plan_id.clone(),
        nodes: cfg.nodes.clone(),
        ctx_size: cfg.ctx_size,
        prompt_id: prompt.id.clone(),
        max_tokens: Some(prompt.max_tokens),
        ..Default::default()
    };
    match outcome {
        Err(msg) => {
            r.error = Some(ErrorInfo {
                code: ErrorCode::ERequestFailed as i32,
                message: msg,
            });
        }
        Ok(m) => {
            r.llama_cpp_build = m.llama_cpp_build;
            r.prompt_tokens = m.prompt_tokens;
            r.ttft_ms = m.ttft_ms;
            r.total_ms = Some(m.total_ms);
            r.prompt_tokens_per_s = m.timings.as_ref().and_then(|t| t.prompt_per_second);
            r.completion_tokens = m.completion_tokens;
            let mut notes = vec![];
            if r.completion_tokens.is_none() {
                r.completion_tokens = Some(m.content_chunks);
                notes.push("completion_tokens estimated from stream chunks");
            }
            r.gen_tokens_per_s = m.timings.as_ref().and_then(|t| t.predicted_per_second);
            if r.gen_tokens_per_s.is_none() {
                let n = r.completion_tokens.unwrap_or(0);
                let gen_ms = m.total_ms.saturating_sub(m.ttft_ms.unwrap_or(m.total_ms));
                if n > 1 && gen_ms > 0 {
                    r.gen_tokens_per_s = Some((n - 1) as f64 * 1000.0 / gen_ms as f64);
                    notes.push("gen_tokens_per_s estimated client-side");
                }
            }
            r.server_timings = m.timings;
            r.notes = notes.join("; ");
        }
    }
    r
}

const CSV_HEADER: &str = "run_id,scenario,repeat_index,warmup,started_at,model_id,plan_id,nodes,llama_cpp_build,ctx_size,prompt_id,max_tokens,prompt_tokens,completion_tokens,ttft_ms,gen_tokens_per_s,prompt_tokens_per_s,total_ms,error,notes";

/// One CSV row with the same columns as the JSON line (minus the nested server_timings object).
pub fn csv_row(r: &BenchResult) -> String {
    fn o<T: ToString>(v: &Option<T>) -> String {
        v.as_ref().map(|x| x.to_string()).unwrap_or_default()
    }
    // RFC 4180 quoting for free-text fields (error messages contain commas and quotes).
    fn q(s: &str) -> String {
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    let f2 = |v: Option<f64>| v.map(|x| format!("{x:.2}")).unwrap_or_default();
    [
        q(&r.run_id),
        q(&r.scenario),
        r.repeat_index.to_string(),
        r.warmup.to_string(),
        q(&r.started_at),
        q(&r.model_id),
        q(&r.plan_id.clone().unwrap_or_default()),
        q(&r.nodes.join(" ")),
        o(&r.llama_cpp_build),
        o(&r.ctx_size),
        q(&r.prompt_id),
        o(&r.max_tokens),
        o(&r.prompt_tokens),
        o(&r.completion_tokens),
        o(&r.ttft_ms),
        f2(r.gen_tokens_per_s),
        f2(r.prompt_tokens_per_s),
        o(&r.total_ms),
        q(&r.error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_default()),
        q(&r.notes),
    ]
    .join(",")
}

fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    let n = xs.len();
    Some(if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    })
}

/// R8.6: markdown table, one row per prompt, medians over measured (non-warm-up, error-free) runs. Median, not
/// mean: one slow outlier (Air throttling, Wi-Fi blip) shouldn't move the headline number. Min-max spread is shown
/// so the R8 acceptance ("repeats stay within a recorded spread") is visible at a glance.
pub fn summary_markdown(cfg: &Config, results: &[BenchResult]) -> String {
    let fmt = |v: Option<f64>, d: usize| {
        v.map(|x| format!("{x:.d$}"))
            .unwrap_or_else(|| "n/a".into())
    };
    let machines = if cfg.nodes.is_empty() {
        "?".to_string()
    } else {
        cfg.nodes.len().to_string()
    };
    let mut out = String::from(
        "| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    let mut ids: Vec<&str> = results.iter().map(|r| r.prompt_id.as_str()).collect();
    ids.dedup();
    for id in ids {
        let measured: Vec<&BenchResult> = results
            .iter()
            .filter(|r| r.prompt_id == id && !r.warmup)
            .collect();
        let ok: Vec<&&BenchResult> = measured.iter().filter(|r| r.error.is_none()).collect();
        let tps: Vec<f64> = ok.iter().filter_map(|r| r.gen_tokens_per_s).collect();
        let spread = match (
            tps.iter().cloned().reduce(f64::min),
            tps.iter().cloned().reduce(f64::max),
        ) {
            (Some(lo), Some(hi)) => format!(" ({lo:.1}-{hi:.1})"),
            _ => String::new(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {}{} | {} | {} |\n",
            cfg.scenario,
            cfg.model_id,
            machines,
            id,
            ok.len(),
            measured.len(),
            fmt(
                median(
                    ok.iter()
                        .filter_map(|r| r.ttft_ms.map(|x| x as f64))
                        .collect()
                ),
                0
            ),
            fmt(median(tps.clone()), 1),
            spread,
            fmt(
                median(
                    ok.iter()
                        .filter_map(|r| r.total_ms.map(|x| x as f64))
                        .collect()
                ),
                0
            ),
            fmt(cfg.size_ratio, 2),
        ));
    }
    out
}

/// R8.4: runs warm-ups + repeats for every prompt, appending each result to `<output_dir>/<run_id>.jsonl` and
/// `.csv` as soon as it exists (a crash at repeat 9 keeps repeats 1-8), then writes `<run_id>.md`.
/// Returns the results and the summary. `progress` gets one human line per request.
pub fn run(
    cfg: &Config,
    prompts: &[Prompt],
    mut progress: impl FnMut(&str),
) -> Result<(Vec<BenchResult>, String), String> {
    let started = netcheck::result::now_rfc3339();
    // "2026-11-07T23:10:00Z" → "bench-2026-11-07T23-10-00Z": file-name safe on Windows too (no ':').
    let run_id = format!("bench-{}", started.replace(':', "-"));
    std::fs::create_dir_all(&cfg.output_dir)
        .map_err(|e| format!("E_CONFIG: create {}: {e}", cfg.output_dir.display()))?;
    let path = |ext: &str| cfg.output_dir.join(format!("{run_id}.{ext}"));
    let open = |p: PathBuf| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| format!("E_CONFIG: open {}: {e}", p.display()))
    };
    let mut jsonl = open(path("jsonl"))?;
    let mut csv = open(path("csv"))?;
    writeln!(csv, "{CSV_HEADER}").map_err(|e| e.to_string())?;

    let timeout = Duration::from_secs(cfg.timeout_s);
    let mut results = vec![];
    for p in prompts {
        for i in 0..cfg.warmup + cfg.repeats {
            let warmup = i < cfg.warmup;
            let repeat_index = if warmup { i + 1 } else { i - cfg.warmup + 1 };
            let at = netcheck::result::now_rfc3339();
            let r = to_result(
                cfg,
                &run_id,
                p,
                repeat_index,
                warmup,
                at,
                run_one(&cfg.endpoint, p, timeout),
            );
            progress(&format!(
                "{} {} #{}: {}",
                p.id,
                if warmup { "warmup" } else { "run" },
                repeat_index,
                match &r.error {
                    Some(e) => e.message.clone(),
                    None => format!(
                        "TTFT {} ms, {} tok/s, {} tokens, {} ms",
                        r.ttft_ms.map(|x| x.to_string()).unwrap_or("?".into()),
                        r.gen_tokens_per_s
                            .map(|x| format!("{x:.1}"))
                            .unwrap_or("?".into()),
                        r.completion_tokens.unwrap_or(0),
                        r.total_ms.unwrap_or(0)
                    ),
                }
            ));
            let line = serde_json::to_string(&r).map_err(|e| format!("E_INTERNAL: {e}"))?;
            writeln!(jsonl, "{line}").map_err(|e| format!("E_INTERNAL: write jsonl: {e}"))?;
            writeln!(csv, "{}", csv_row(&r)).map_err(|e| format!("E_INTERNAL: write csv: {e}"))?;
            results.push(r);
        }
    }
    let md = summary_markdown(cfg, &results);
    std::fs::write(path("md"), &md).map_err(|e| format!("E_INTERNAL: write summary: {e}"))?;
    Ok((results, md))
}

// =====================================================================================================
// Tests: a fake llama-server replays the exact b8665 stream recorded at 9:05pm, split into awkward chunks.
// =====================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    // Recorded from llama-server b8665 + qwen2.5-0.5b (Sat 9:05pm), ids shortened.
    const STREAM: &str = concat!(
        "data: {\"choices\":[{\"finish_reason\":null,\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null}}],\"system_fingerprint\":\"b8665-b8635075f\",\"object\":\"chat.completion.chunk\"}\n\n",
        "data: {\"choices\":[{\"finish_reason\":null,\"index\":0,\"delta\":{\"content\":\"Hello\"}}],\"system_fingerprint\":\"b8665-b8635075f\",\"object\":\"chat.completion.chunk\"}\n\n",
        "data: {\"choices\":[{\"finish_reason\":null,\"index\":0,\"delta\":{\"content\":\"!\"}}],\"system_fingerprint\":\"b8665-b8635075f\",\"object\":\"chat.completion.chunk\"}\n\n",
        "data: {\"choices\":[{\"finish_reason\":\"stop\",\"index\":0,\"delta\":{}}],\"system_fingerprint\":\"b8665-b8635075f\",\"object\":\"chat.completion.chunk\"}\n\n",
        "data: {\"choices\":[],\"system_fingerprint\":\"b8665-b8635075f\",\"object\":\"chat.completion.chunk\",\"usage\":{\"completion_tokens\":3,\"prompt_tokens\":36,\"total_tokens\":39},\"timings\":{\"cache_n\":0,\"prompt_n\":36,\"prompt_ms\":39.727,\"prompt_per_token_ms\":1.1035277777777777,\"prompt_per_second\":906.1847106501876,\"predicted_n\":3,\"predicted_ms\":17.098,\"predicted_per_token_ms\":5.699333333333333,\"predicted_per_second\":175.45911802550006}}\n\n",
        "data: [DONE]\n\n",
    );

    /// Encodes `body` as chunks of `size` bytes, so boundaries fall mid-line and mid-JSON.
    fn chunk(body: &str, size: usize) -> Vec<u8> {
        let mut out = vec![];
        for c in body.as_bytes().chunks(size) {
            out.extend(format!("{:x}\r\n", c.len()).bytes());
            out.extend(c);
            out.extend(b"\r\n");
        }
        out.extend(b"0\r\n\r\n");
        out
    }

    /// One-shot fake server on a free port; returns the endpoint URL.
    fn serve(head: &'static str, body: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf); // the request; content not checked here
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&body).unwrap();
        });
        format!("http://{addr}")
    }

    fn prompt() -> Prompt {
        Prompt {
            id: "p1".into(),
            text: "Say hi in 3 words.".into(),
            max_tokens: 12,
            use_: "bench".into(),
        }
    }

    fn cfg() -> Config {
        toml::from_str(
            "endpoint = \"http://127.0.0.1:1\"\nprompts_file = \"p.toml\"\noutput_dir = \"out\"\nscenario = \"single_fit\"\nmodel_id = \"m\"\nnodes = [\"a\", \"b\"]\n",
        )
        .unwrap()
    }

    const OK_CHUNKED: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";

    #[test]
    fn chunked_stream_split_mid_line_is_measured() {
        for size in [1, 7, 64, 100_000] {
            let url = serve(OK_CHUNKED, chunk(STREAM, size));
            let m = run_one(&url, &prompt(), Duration::from_secs(5)).unwrap();
            assert_eq!(m.text, "Hello!", "chunk size {size}");
            assert_eq!(m.content_chunks, 2);
            assert_eq!((m.prompt_tokens, m.completion_tokens), (Some(36), Some(3)));
            assert_eq!(m.llama_cpp_build, Some(8665));
            assert!(m.ttft_ms.is_some());
            assert_eq!(
                m.timings.unwrap().predicted_per_second,
                Some(175.45911802550006)
            );
        }
    }

    #[test]
    fn plain_body_without_chunking_also_works() {
        let url = serve(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n",
            STREAM.as_bytes().to_vec(),
        );
        assert_eq!(
            run_one(&url, &prompt(), Duration::from_secs(5))
                .unwrap()
                .text,
            "Hello!"
        );
    }

    #[test]
    fn http_error_becomes_request_failed_line() {
        let url = serve(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 7\r\n\r\n",
            b"loading".to_vec(),
        );
        let out = run_one(&url, &prompt(), Duration::from_secs(5));
        let err = out.clone().unwrap_err();
        assert!(err.starts_with("E_REQUEST_FAILED: HTTP 503"), "{err}");
        let r = to_result(&cfg(), "run", &prompt(), 1, false, "t".into(), out);
        assert_eq!(
            serde_json::to_value(&r).unwrap()["error"]["code"],
            "E_REQUEST_FAILED"
        );
    }

    #[test]
    fn cut_stream_is_an_error_not_a_short_success() {
        let mut body = chunk(STREAM, 50);
        body.truncate(120); // socket closes mid-chunk, before any generated token
        let url = serve(OK_CHUNKED, body);
        assert!(run_one(&url, &prompt(), Duration::from_secs(5)).is_err());
    }

    #[test]
    fn result_prefers_server_timings_and_falls_back_to_estimates() {
        let url = serve(OK_CHUNKED, chunk(STREAM, 64));
        let m = run_one(&url, &prompt(), Duration::from_secs(5)).unwrap();
        let r = to_result(&cfg(), "run", &prompt(), 1, false, "t".into(), Ok(m));
        assert_eq!(r.gen_tokens_per_s, Some(175.45911802550006));
        assert_eq!(r.prompt_tokens_per_s, Some(906.1847106501876));
        assert_eq!(r.notes, "");

        // No usage, no timings: count chunks, estimate client-side, and say so.
        let m = Measurement {
            ttft_ms: Some(100),
            total_ms: 1100,
            content_chunks: 11,
            ..Default::default()
        };
        let r = to_result(&cfg(), "run", &prompt(), 1, false, "t".into(), Ok(m));
        assert_eq!(r.completion_tokens, Some(11));
        assert_eq!(r.gen_tokens_per_s, Some(10.0)); // 10 tokens after the first in 1000 ms
        assert!(r.notes.contains("estimated"));
    }

    #[test]
    fn csv_quotes_free_text_and_summary_uses_medians() {
        let mk = |tps: f64, warm: bool| BenchResult {
            prompt_id: "p1".into(),
            warmup: warm,
            gen_tokens_per_s: Some(tps),
            ttft_ms: Some(100),
            total_ms: Some(1000),
            ..Default::default()
        };
        let mut failed = mk(0.0, false);
        failed.gen_tokens_per_s = None;
        failed.error = Some(ErrorInfo {
            code: ErrorCode::ERequestFailed as i32,
            message: "HTTP 503: a, \"b\"".into(),
        });
        assert!(csv_row(&failed).contains("\"HTTP 503: a, \"\"b\"\"\""));

        let rs = vec![
            mk(999.0, true),
            mk(10.0, false),
            mk(30.0, false),
            mk(20.0, false),
            failed,
        ];
        let md = summary_markdown(&cfg(), &rs);
        assert!(
            md.contains("| single_fit | m | 2 | p1 | 3/4 | 100 | 20.0 (10.0-30.0) | 1000 | n/a |"),
            "{md}"
        );
    }

    #[test]
    fn endpoint_and_prompt_parsing() {
        assert_eq!(
            parse_endpoint("http://192.168.8.10:8080/").unwrap(),
            ("192.168.8.10".into(), 8080)
        );
        assert!(parse_endpoint("https://x:1").is_err());
        assert!(parse_endpoint("http://x").is_err());
        let f: PromptFile = toml::from_str(
            "[[prompt]]\nid = \"a\"\ntext = \"t\"\nmax_tokens = 5\nuse = \"demo\"\n",
        )
        .unwrap();
        assert_eq!(f.prompt[0].use_, "demo");
        assert_eq!(build_from_fingerprint("b8665-b8635075f"), Some(8665));
    }
}
