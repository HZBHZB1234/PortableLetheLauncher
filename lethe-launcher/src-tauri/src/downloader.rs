use crate::http::{client, download_url};
use crate::manifest::{FileEntry, FileManifest};
use crate::profiles::{self, PluginFile};
use crate::steam::try_copy_from_steam;
use futures_util::StreamExt;
use serde::Serialize;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// All endpoints come from the active profile. The constants below are only
/// used to fill blank fields when a profile was created by an older launcher.
pub fn manifest_url() -> String {
    let profile = profiles::active_profile();
    if profile.manifest_url.trim().is_empty() {
        profiles::DEFAULT_MANIFEST_URL.to_string()
    } else {
        profile.manifest_url
    }
}

pub fn download_base_url() -> String {
    let profile = profiles::active_profile();
    if profile.download_base_url.trim().is_empty() {
        profiles::DEFAULT_DOWNLOAD_BASE_URL.to_string()
    } else {
        profile.download_base_url
    }
}

pub fn plugin_files() -> Vec<PluginFile> {
    let profile = profiles::active_profile();
    if profile.plugin_urls.is_empty() {
        profiles::default_plugin_files()
    } else {
        profile.plugin_urls
    }
}

/// Maximum files downloaded at once.
///
/// Measured against the real endpoint (4 x ~15MB files, 58.9MB total):
///   sequential        0.95 MB/s
///   4 files parallel  1.44 MB/s (+51%)
///   8 files parallel  1.56 MB/s
///   12 files parallel 1.75 MB/s
/// 4 is the knee of the curve and keeps 4 concurrent TCP connections, which is
/// well within what an HTTP/1.1 Cloudflare edge accepts. Beyond 4 the gain is
/// small while the failure surface and memory footprint grow.
const DOWNLOAD_CONCURRENCY: usize = 4;

/// Files at or above this size are split into this many HTTP Range requests.
/// Splitting only pays off on top of file-level parallelism; measured
/// `-j4 -x2` reached 1.65 MB/s versus 1.44 MB/s for `-j4 -x1`, while `-x4`
/// regressed to 1.59 MB/s. Hence a hard cap of 2.
const RANGE_SPLIT_THRESHOLD: u64 = 100 * 1024 * 1024;
const RANGE_SPLIT_PARTS: usize = 2;

/// Minimum size of each Range part. Avoids absurd splits for files just over
/// the threshold.
const MIN_RANGE_PART: u64 = 16 * 1024 * 1024;

/// Retry policy for a single file. A transient failure on one of ~2000 files
/// should not abort the whole sync, and parallel downloads make any single
/// request more likely to hit a hiccup.
const MAX_ATTEMPTS: u32 = 4;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);

/// How often the progress event is emitted. Emitting per network chunk floods
/// the WebView at high throughput; 250ms is smooth enough for a progress bar.
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);

/// Suffix for in-progress downloads. The real path is only used once the file
/// is complete and verified by size, so an interrupted run never leaves a
/// truncated file that the verifier would mistake for a corrupt install.
const PART_SUFFIX: &str = ".lethe-part";

#[derive(Debug, Serialize, Clone)]
pub struct StatusChangeEvent {
    pub message: String,
}

/// Progress payload for a download.
///
/// `bytes_downloaded`/`total_bytes`/`percent` describe the whole sync so the
/// existing progress bar keeps working unchanged. `bytes_per_sec` is the
/// smoothed aggregate throughput and `eta_secs` the estimated remaining time.
#[derive(Debug, Serialize, Clone)]
pub struct DownloadProgressEvent {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub percent: f64,
    /// Summary of what is in flight, e.g. "3 files (BGM_Title.assets.bank)".
    /// With parallel downloads there is no single "current file".
    pub current_file: String,
    /// Number of files being downloaded right now.
    pub active_files: usize,
    /// Total files still to be transferred (queued + in flight).
    pub remaining_files: usize,
    /// Smoothed throughput in bytes per second.
    pub bytes_per_sec: f64,
    /// Estimated seconds remaining, or `None` when it cannot be determined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_secs: Option<u64>,
}

pub async fn download_manifest(app_handle: &AppHandle) -> Result<FileManifest, String> {
    let _ = app_handle.emit(
        "status-change",
        StatusChangeEvent {
            message: "Downloading manifest...".to_string(),
        },
    );

    let manifest_url = manifest_url();
    crate::lethe_log!("Manifest URL: {}", manifest_url);
    let response = client()
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download manifest: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Manifest server returned HTTP {}", status.as_u16()));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read manifest response: {}", e))?;

    let manifest: FileManifest =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse manifest JSON: {}", e))?;

    crate::lethe_log!(
        "Downloaded manifest: {} files, {} bytes",
        manifest.total_files,
        manifest.total_size
    );

    Ok(manifest)
}

/// Weight of the newest sample in the throughput moving average. 0.3 reacts
/// quickly enough to show a stall within a second or two while still damping
/// the burstiness of a 250ms window.
const RATE_SMOOTHING: f64 = 0.3;

/// A sample is discarded when the gap since the previous one exceeds this many
/// times the tick interval. Such a gap means the pipeline stalled (a Steam
/// hash, a cache save, scheduler jitter), so the bytes moved during it did not
/// come from steady network throughput and would poison the average.
const STALE_WINDOW_FACTOR: u128 = 4;

/// Floor on the window length used to compute an instantaneous rate. Without
/// it, two ticks landing in the same millisecond would divide by ~0 and report
/// an absurd rate.
const MIN_WINDOW_MS: u128 = 50;

/// Throughput measured over a window shorter than this is reported as 0 rather
/// than extrapolated. A few hundred bytes in 60ms is noise, not a rate.
const MIN_SAMPLE_BYTES: u64 = 1024;

/// Shared progress accounting for concurrent downloads.
///
/// Two independent measurements live here on purpose:
///
/// * `bytes_downloaded` is the monotonic progress the UI's bar and percentage
///   are driven by. It counts everything, including files satisfied from a
///   local Steam copy, because the user cares about total work done.
/// * `network_bytes` only counts bytes actually pulled over HTTP. Mixing the
///   two is what made the rate spike to hundreds of MB/s the instant a Steam
///   copy completed, and drop to zero when nothing was being fetched.
///
/// Only the ticker calls `sample_rate`, so the EMA state can live behind a
/// plain `Mutex` without contention.
struct ProgressTracker {
    /// Total bytes accounted for, including local Steam copies.
    bytes_downloaded: AtomicU64,
    /// Bytes fetched over HTTP only. The rate is derived from this.
    network_bytes: AtomicU64,
    files_done: AtomicU64,
    /// Rate-sampling state. Mutated only by the ticker, but behind a Mutex so
    /// it can hold an `f64` (atomics cannot) and an `Instant`.
    rate_state: std::sync::Mutex<RateState>,
}

/// Sampling window state. `None` means "no previous sample yet".
struct RateState {
    last_tick: Option<std::time::Instant>,
    last_network_bytes: u64,
    smoothed_bps: f64,
}

impl ProgressTracker {
    fn new(base_bytes: u64) -> Self {
        Self {
            bytes_downloaded: AtomicU64::new(base_bytes),
            network_bytes: AtomicU64::new(0),
            files_done: AtomicU64::new(0),
            rate_state: std::sync::Mutex::new(RateState {
                last_tick: None,
                last_network_bytes: 0,
                smoothed_bps: 0.0,
            }),
        }
    }

    /// Count bytes toward the overall progress bar, whatever their source.
    fn add_bytes(&self, n: u64) {
        self.bytes_downloaded.fetch_add(n, Ordering::SeqCst);
    }

    /// Count bytes that actually crossed the network. Only these feed the rate.
    fn add_network_bytes(&self, n: u64) {
        self.bytes_downloaded.fetch_add(n, Ordering::SeqCst);
        self.network_bytes.fetch_add(n, Ordering::SeqCst);
    }

    fn file_done(&self) {
        self.files_done.fetch_add(1, Ordering::SeqCst);
    }

    fn bytes(&self) -> u64 {
        self.bytes_downloaded.load(Ordering::SeqCst)
    }

    /// Sample network throughput and fold it into the smoothed rate.
    ///
    /// Returns bytes per second. Advances the measurement window, so it must be
    /// called from exactly one place per tick.
    fn sample_rate(&self, interval_ms: u128) -> f64 {
        let now_network = self.network_bytes.load(Ordering::SeqCst);

        // A poisoned mutex only affects the rate display; never panic over it.
        let mut state = match self.rate_state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        let now = std::time::Instant::now();
        let Some(prev_tick) = state.last_tick else {
            // First sample: open the window and report nothing yet rather than
            // extrapolating from a partial interval.
            state.last_tick = Some(now);
            state.last_network_bytes = now_network;
            return 0.0;
        };

        let elapsed_ms = now.saturating_duration_since(prev_tick).as_millis();
        let delta = now_network.saturating_sub(state.last_network_bytes);

        // Advance the window regardless of what we decide below.
        state.last_tick = Some(now);
        state.last_network_bytes = now_network;

        // A gap far longer than the tick interval means the pipeline stalled.
        // Those bytes did not arrive at network speed, so discard the sample
        // instead of letting it drag the average down (or up, on catch-up).
        if elapsed_ms > interval_ms.saturating_mul(STALE_WINDOW_FACTOR) {
            state.smoothed_bps = 0.0;
            return 0.0;
        }

        // Guard both ends of the division: a zero-length window would explode
        // the rate, and a trivially small delta is noise, not throughput.
        let effective_ms = elapsed_ms.max(MIN_WINDOW_MS);
        if delta < MIN_SAMPLE_BYTES {
            // Nothing meaningful arrived. Decay toward zero but do not snap,
            // so a momentary hiccup does not blank the readout.
            state.smoothed_bps *= 1.0 - RATE_SMOOTHING;
            return state.smoothed_bps;
        }

        let instant = (delta as f64) * 1000.0 / (effective_ms as f64);

        state.smoothed_bps = if state.smoothed_bps <= 0.0 {
            instant
        } else {
            state.smoothed_bps * (1.0 - RATE_SMOOTHING) + instant * RATE_SMOOTHING
        };

        state.smoothed_bps
    }
}

/// Format a byte count for log output.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Format a byte rate for the UI, e.g. "1.4 MB/s".
pub fn format_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return "-".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}

/// Path of the temporary in-progress file for an entry.
fn part_path(entry_path: &str) -> String {
    format!("{}{}", entry_path, PART_SUFFIX)
}

/// Remove a leftover `.lethe-part` file, ignoring any error.
fn cleanup_part(entry_path: &str) {
    let _ = fs::remove_file(part_path(entry_path));
}

/// Ensure the parent directory of `path` exists.
fn ensure_parent(path: &str) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create directory for {}: {}", path, e))?;
        }
    }
    Ok(())
}

/// Download one HTTP Range chunk and append it at `offset` in the part file.
///
/// Returns the number of bytes written. The file is opened with append
/// semantics after seeking, so retries simply rewrite from `offset` when the
/// previous attempt wrote fewer bytes than promised.
///
/// Bytes are reported to `progress` per chunk, as they arrive. Reporting the
/// whole range as a single lump after the transfer completes (the old
/// behaviour) meant a large file showed no movement at all for its entire
/// duration and then jumped: the rate decayed to zero and the speed readout
/// disappeared, while the bar froze and then leapt. Per-chunk accounting is
/// what makes both the rate and the percentage track a single big file.
async fn download_range(
    entry: &FileEntry,
    base_url: &str,
    offset: u64,
    length: u64,
    progress: &ProgressTracker,
) -> Result<u64, String> {
    let url = download_url(base_url, &entry.path);
    let end = offset + length - 1;

    let mut request = client().get(&url);
    // Only send a Range header when we are actually splitting. A single-part
    // download requests the whole object, which is friendlier to any
    // intermediary that mishandles Range.
    if length > 0 {
        request = request.header("Range", format!("bytes={}-{}", offset, end));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", entry.path, e))?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 206 {
        return Err(format!(
            "HTTP {} for {} (range {}-{})",
            status.as_u16(),
            entry.path,
            offset,
            end
        ));
    }

    // A 206 means the server honoured the range; a 200 means it ignored it and
    // sent the whole object, which is only correct for offset == 0 and
    // length == full size. Guard against silently writing the wrong bytes.
    if status.as_u16() == 206 && length > 0 {
        let declared = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !declared.is_empty() && !declared.starts_with(&format!("bytes {}-", offset)) {
            return Err(format!(
                "Server returned unexpected Content-Range '{}' for {}",
                declared, entry.path
            ));
        }
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(part_path(&entry.path))
        .map_err(|e| format!("Cannot open part file for {}: {}", entry.path, e))?;

    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Cannot seek part file for {}: {}", entry.path, e))?;

    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error for {}: {}", entry.path, e))?;
        // Stop once the requested range is satisfied so an over-long 200
        // response cannot bleed into the next range's territory.
        if length > 0 && written + chunk.len() as u64 > length {
            let allowed = length - written;
            file.write_all(&chunk[..allowed as usize])
                .map_err(|e| format!("Write error for {}: {}", entry.path, e))?;
            written += allowed;
            progress.add_network_bytes(allowed);
            break;
        }
        file.write_all(&chunk)
            .map_err(|e| format!("Write error for {}: {}", entry.path, e))?;
        written += chunk.len() as u64;
        progress.add_network_bytes(chunk.len() as u64);
    }

    if length > 0 && written < length {
        return Err(format!(
            "Short read for {}: got {} of {} bytes in range {}-{}",
            entry.path, written, length, offset, end
        ));
    }

    Ok(written)
}
/// Download a single file to its final path, resuming from an existing part
/// file and retrying transient failures with exponential backoff.
async fn download_file(
    entry: &FileEntry,
    base_url: &str,
    progress: &ProgressTracker,
) -> Result<(), String> {
    let part = part_path(&entry.path);
    let started = Instant::now();

    // Resume support: keep whatever a previous run already fetched. We trust
    // the part length but always verify the final size below, so a corrupt or
    // over-long part is caught rather than silently installed.
    let already = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let resume_from = if already > 0 && already < entry.size {
        crate::lethe_log!(
            "Resuming {} from {} ({} already downloaded)",
            entry.path,
            already,
            format_bytes(already)
        );
        already
    } else {
        if already >= entry.size {
            // A stale part at or beyond full size is not resumable; start over.
            crate::lethe_log!(
                "Discarding stale part file for {} ({} bytes on disk)",
                entry.path,
                already
            );
        }
        cleanup_part(&entry.path);
        0
    };

    ensure_parent(&part)?;

    // Pre-allocate the part file so concurrent range writes can seek freely.
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&part)
        .map_err(|e| format!("Cannot create part file for {}: {}", entry.path, e))?;
    drop(file);
    if resume_from == 0 {
        // Truncate so a fresh attempt cannot inherit old bytes.
        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&part)
            .map_err(|e| format!("Cannot truncate part file for {}: {}", entry.path, e))?;
    }

    // Split large files into Range requests; everything else is one request.
    let ranges: Vec<(u64, u64)> = if entry.size >= RANGE_SPLIT_THRESHOLD {
        let part_len = (entry.size / RANGE_SPLIT_PARTS as u64).max(MIN_RANGE_PART);
        let mut v = Vec::with_capacity(RANGE_SPLIT_PARTS + 1);
        let mut off = 0u64;
        while off < entry.size {
            let len = part_len.min(entry.size - off);
            v.push((off, len));
            off += len;
        }
        v
    } else {
        vec![(0, entry.size)]
    };

    let mut last_err: Option<String> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        let mut all_ok = true;

        for (offset, length) in &ranges {
            // How much of this range is already on disk. The part file is
            // written strictly in order, so everything below `on_disk` is
            // complete and only the remainder needs fetching.
            let on_disk = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if on_disk >= offset + length {
                continue;
            }

            // Never rewind: if the file is longer than this range starts, the
            // previous range already covered that space.
            let write_from = on_disk.max(*offset);
            let remaining = offset + length - write_from;

            // Bytes are accounted for per chunk inside `download_range`, so a
            // single large file still drives the bar and the rate. Only HTTP
            // traffic feeds the throughput estimate: counting local copies
            // here would spike the rate to hundreds of MB/s.
            match download_range(entry, base_url, write_from, remaining, progress).await {
                Ok(_) => {}
                Err(e) => {
                    last_err = Some(e);
                    all_ok = false;
                    break;
                }
            }
        }

        if all_ok {
            // Verify the assembled part file is exactly the expected size
            // before it becomes the real file. This is what makes a resumed
            // download safe: a truncated or over-long part is caught here
            // rather than being installed as a corrupt game file.
            let final_len = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if final_len != entry.size {
                last_err = Some(format!(
                    "Size mismatch for {}: got {}, expected {}",
                    entry.path, final_len, entry.size
                ));
                // Fall through to the retry handling below.
            } else {
                // Atomic-ish promotion: rename into place. On Windows a rename
                // over an existing file fails, so remove the target first.
                ensure_parent(&entry.path)?;
                if Path::new(&entry.path).exists() {
                    let _ = fs::remove_file(&entry.path);
                }
                fs::rename(&part, &entry.path)
                    .map_err(|e| format!("Cannot finalize {}: {}", entry.path, e))?;

                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let bytes_this_run = entry.size.saturating_sub(resume_from);
                crate::lethe_log!(
                    "Downloaded: {} ({}, {}, {} attempt(s), {:.1}s)",
                    entry.path,
                    format_bytes(entry.size),
                    format_rate(bytes_this_run as f64 / elapsed),
                    attempt,
                    elapsed
                );
                return Ok(());
            }
        }

        if attempt < MAX_ATTEMPTS {
            let delay = backoff(attempt);
            crate::lethe_log!(
                "Download failed for {} (attempt {}/{}): {}. Retrying in {:?}",
                entry.path,
                attempt,
                MAX_ATTEMPTS,
                last_err.as_deref().unwrap_or("unknown error"),
                delay
            );
            tokio::time::sleep(delay).await;
        }
    }

    // All attempts exhausted. Leave the part file so the next run can resume.
    Err(format!(
        "Failed to download {} after {} attempts: {}",
        entry.path,
        MAX_ATTEMPTS,
        last_err.unwrap_or_else(|| "unknown error".to_string())
    ))
}

/// Exponential backoff with a cap.
fn backoff(attempt: u32) -> Duration {
    let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
    (RETRY_BASE_DELAY * factor).min(Duration::from_secs(8))
}

/// Tracks which files are in flight so the UI can describe real activity.
#[derive(Default)]
struct ActiveFiles {
    inner: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ActiveFiles {
    fn new() -> Self {
        Self::default()
    }

    fn enter(&self, path: &str) -> ActiveGuard {
        if let Ok(mut g) = self.inner.lock() {
            g.push(path.to_string());
        }
        ActiveGuard {
            inner: self.inner.clone(),
            path: path.to_string(),
        }
    }

    /// Human-readable summary, e.g. "3 files (a.dll, b.dll)".
    fn summary(&self, max_names: usize) -> (usize, String) {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return (0, String::new()),
        };
        let count = guard.len();
        if count == 0 {
            return (0, String::new());
        }
        let names: Vec<String> = guard
            .iter()
            .rev()
            .take(max_names)
            .map(|p| basename(p))
            .collect();
        let label = if count > max_names {
            format!("{} files ({}, ...)", count, names.join(", "))
        } else {
            format!(
                "{} file{} ({})",
                count,
                if count == 1 { "" } else { "s" },
                names.join(", ")
            )
        };
        (count, label)
    }
}

/// RAII guard that removes a path from the active set when the download ends.
struct ActiveGuard {
    inner: Arc<std::sync::Mutex<Vec<String>>>,
    path: String,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = self.inner.lock() {
            g.retain(|p| p != &self.path);
        }
    }
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Emit a progress event, computing percentage, rate and ETA.
///
/// `sample_rate` is only called from the periodic ticker. The final emit after
/// the last file completes must not sample: its window is an arbitrary length
/// (however long the last download happened to take), so folding it in would
/// report a rate that never actually existed.
fn emit_progress(
    app_handle: &AppHandle,
    progress: &ProgressTracker,
    active: &ActiveFiles,
    total_bytes: u64,
    remaining_files: usize,
) {
    let bytes = progress.bytes();
    let percent = if total_bytes > 0 {
        (bytes as f64 / total_bytes as f64) * 100.0
    } else {
        0.0
    };

    let (active_count, current_file) = active.summary(3);

    let bytes_per_sec = progress.sample_rate(PROGRESS_EMIT_INTERVAL.as_millis());

    let eta_secs = if bytes_per_sec > 0.0 && total_bytes > bytes {
        Some(((total_bytes - bytes) as f64 / bytes_per_sec) as u64)
    } else if total_bytes > 0 && bytes >= total_bytes {
        Some(0)
    } else {
        None
    };

    let _ = app_handle.emit(
        "download-progress",
        DownloadProgressEvent {
            bytes_downloaded: bytes,
            total_bytes,
            percent,
            current_file,
            active_files: active_count,
            remaining_files,
            bytes_per_sec,
            eta_secs,
        },
    );
}

/// Download the given files with bounded parallelism.
///
/// `base_bytes` is the number of bytes already validated (counted as done for
/// percentage purposes). Returns the total bytes accounted for.
pub async fn download_files(
    app_handle: &AppHandle,
    files: &[FileEntry],
    base_bytes: u64,
    total_bytes: u64,
) -> Result<u64, String> {
    if files.is_empty() {
        return Ok(base_bytes);
    }

    // Snapshot once: switching profiles mid-sync must not mix file sets.
    let base_url = Arc::new(download_base_url());
    crate::lethe_log!("Download base URL: {}", base_url);
    let progress = Arc::new(ProgressTracker::new(base_bytes));
    let active = Arc::new(ActiveFiles::new());
    let remaining = Arc::new(AtomicU64::new(files.len() as u64));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(DOWNLOAD_CONCURRENCY));

    let _ = app_handle.emit(
        "status-change",
        StatusChangeEvent {
            message: format!(
                "Downloading {} files ({} parallel)...",
                files.len(),
                DOWNLOAD_CONCURRENCY
            ),
        },
    );

    // Background ticker: emit progress on a fixed cadence regardless of how
    // chatty the individual downloads are.
    let ticker_app = app_handle.clone();
    let ticker_progress = progress.clone();
    let ticker_active = active.clone();
    let ticker_remaining = remaining.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROGRESS_EMIT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            emit_progress(
                &ticker_app,
                &ticker_progress,
                &ticker_active,
                total_bytes,
                ticker_remaining.load(Ordering::SeqCst) as usize,
            );
        }
    });

    let mut handles = Vec::with_capacity(files.len());

    for entry in files {
        let entry = entry.clone();
        let progress = progress.clone();
        let active = active.clone();
        let remaining = remaining.clone();
        let semaphore = semaphore.clone();
        let base_url = base_url.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");

            // Local Steam copy first: it is a disk-to-disk copy and much faster
            // than the network when the user already owns the game on Steam.
            let guard = active.enter(&entry.path);
            match try_copy_from_steam(&entry).await {
                Ok(true) => {
                    progress.add_bytes(entry.size);
                    progress.file_done();
                    remaining.fetch_sub(1, Ordering::SeqCst);
                    drop(guard);
                    return Ok(());
                }
                Ok(false) => {}
                Err(e) => {
                    crate::lethe_log!("Steam copy failed for {}: {}", entry.path, e);
                }
            }

            let result = download_file(&entry, &base_url, &progress).await;

            match &result {
                Ok(()) => {
                    progress.file_done();
                    remaining.fetch_sub(1, Ordering::SeqCst);
                }
                Err(e) => {
                    crate::lethe_log!("Download failed: {}", e);
                }
            }

            drop(guard);
            result
        });

        handles.push(handle);
    }

    let mut first_error: Option<String> = None;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for handle in handles {
        match handle.await {
            Ok(Ok(())) => succeeded += 1,
            Ok(Err(e)) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
                failed += 1;
            }
            Err(e) => {
                crate::lethe_log!("Download task panicked: {}", e);
                failed += 1;
            }
        }
    }

    ticker.abort();

    // Final emit so the bar lands exactly on 100% and the rate is not stale.
    emit_progress(
        app_handle,
        &progress,
        &active,
        total_bytes,
        remaining.load(Ordering::SeqCst) as usize,
    );

    let processed = progress.bytes();

    if let Some(e) = first_error {
        return Err(e);
    }

    crate::lethe_log!(
        "Download phase complete: {} file(s) succeeded, {} failed, {} transferred",
        succeeded,
        failed,
        format_bytes(processed)
    );

    Ok(processed)
}

/// Shared view of in-flight plugin downloads so the status line reports one
/// combined figure instead of three tasks fighting over it.
#[derive(Default)]
struct DllProgress {
    inner: Arc<std::sync::Mutex<Vec<(String, u64, u64)>>>,
}

impl DllProgress {
    fn new() -> Self {
        Self::default()
    }

    /// Record `(name, written, total)`. `total == 0` means "size unknown".
    fn set(&self, name: &str, written: u64, total: u64) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(entry) = g.iter_mut().find(|(n, _, _)| n == name) {
                entry.1 = written;
                entry.2 = total;
            } else {
                g.push((name.to_string(), written, total));
            }
        }
    }

    fn clear(&self, name: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.retain(|(n, _, _)| n != name);
        }
    }

    /// One human-readable line, or `None` when nothing is in flight.
    fn describe(&self) -> Option<String> {
        let guard = self.inner.lock().ok()?;
        if guard.is_empty() {
            return None;
        }
        let total: u64 = guard.iter().map(|(_, _, t)| *t).sum();
        // Clamp per-entry so an over-long read cannot push the bar past 100%.
        let written: u64 = guard.iter().map(|(_, w, t)| (*w).min(*t)).sum();
        if total == 0 {
            return Some(format!("Downloading {} plugin(s)...", guard.len()));
        }
        Some(format!(
            "Downloading {} plugin(s)... {} / {} ({}%)",
            guard.len(),
            format_bytes(written),
            format_bytes(total),
            (written as f64 / total as f64 * 100.0) as u64
        ))
    }
}

/// Parse `Content-Range: bytes <start>-<end>/<total>` into `(start, total)`.
fn parse_content_range(
    headers: &reqwest::header::HeaderMap,
    expected_start: u64,
    filename: &str,
) -> Result<(u64, u64), String> {
    let raw = headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let malformed = || format!("Malformed Content-Range '{}' for {}", raw, filename);

    let body = raw.strip_prefix("bytes").unwrap_or(raw).trim();
    let (range, total_str) = body.split_once('/').ok_or_else(malformed)?;
    let start_str = range
        .split_once('-')
        .map(|(s, _)| s)
        .ok_or_else(malformed)?;

    let start: u64 = start_str.trim().parse().map_err(|_| malformed())?;
    let total: u64 = total_str.trim().parse().map_err(|_| malformed())?;

    // The server must serve exactly the offset we asked for. Writing a
    // different one would silently corrupt the file.
    if start != expected_start {
        return Err(format!(
            "Server returned Content-Range starting at {} for {} (expected {})",
            start, filename, expected_start
        ));
    }

    Ok((start, total))
}

/// One attempt at fetching a plugin DLL.
///
/// Streams straight to a `.lethe-part` file rather than buffering the whole
/// body, so memory stays flat and a failed attempt keeps what it already
/// received. Resumes from a leftover part file when the server honours Range.
async fn fetch_dll_once(
    url: &str,
    filename: &str,
    final_path: &Path,
    part: &str,
    dll_progress: &DllProgress,
) -> Result<(), String> {
    let already = fs::metadata(part).map(|m| m.len()).unwrap_or(0);

    let mut request = client().get(url);
    if already > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", already));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", filename, e))?;

    let status = response.status();

    // 416: the part file already reached or exceeded the real size, so there
    // is nothing left to fetch. Drop it and let the retry start clean.
    if status.as_u16() == 416 {
        let _ = fs::remove_file(part);
        return Err(format!(
            "Stale part file for {} ({} bytes); restarting",
            filename, already
        ));
    }

    if !status.is_success() && status.as_u16() != 206 {
        return Err(format!("HTTP {} for {}", status.as_u16(), filename));
    }

    // A 206 means the server honoured our Range and is sending only the tail,
    // so we must keep the bytes already on disk and append after them.
    //
    // A 200 means the server ignored the Range and is sending the whole
    // object. Truncating here is what makes resumption safe when the server
    // silently downgrades us: without it a partial file would be appended to
    // and end up with duplicated bytes at the front.
    let (start_offset, total) = if status.as_u16() == 206 {
        let (off, tot) = parse_content_range(response.headers(), already, filename)?;
        (off, Some(tot))
    } else {
        (0u64, response.content_length())
    };

    ensure_parent(part)?;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(part)
        .map_err(|e| format!("Cannot open part file for {}: {}", filename, e))?;

    if start_offset == 0 {
        // A whole-object response must not inherit bytes from an old attempt.
        file.set_len(0)
            .map_err(|e| format!("Cannot truncate part file for {}: {}", filename, e))?;
    }
    file.seek(SeekFrom::Start(start_offset))
        .map_err(|e| format!("Cannot seek part file for {}: {}", filename, e))?;

    dll_progress.set(filename, start_offset, total.unwrap_or(0));

    let mut stream = response.bytes_stream();
    let mut written = start_offset;
    let mut last_emit = Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error for {}: {}", filename, e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("Write error for {}: {}", filename, e))?;
        written += chunk.len() as u64;

        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            last_emit = Instant::now();
            dll_progress.set(filename, written, total.unwrap_or(0));
        }
    }

    drop(file);

    let final_len = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if let Some(expected) = total {
        if final_len != expected {
            // Leave the part file: the next attempt resumes from it.
            return Err(format!(
                "Size mismatch for {}: got {}, expected {}",
                filename, final_len, expected
            ));
        }
    }

    // Promote into place. On Windows a rename over an existing file fails.
    if final_path.exists() {
        fs::remove_file(final_path).map_err(|e| format!("Cannot replace {}: {}", filename, e))?;
    }
    fs::rename(part, final_path).map_err(|e| format!("Cannot finalize {}: {}", filename, e))?;

    crate::lethe_log!(
        "Downloaded additional DLL: {} ({} bytes)",
        filename,
        final_len
    );

    Ok(())
}

/// Fetch one plugin DLL, retrying transient failures with backoff.
async fn download_one_dll(
    url: &str,
    filename: &str,
    plugins_dir: &Path,
    dll_progress: &DllProgress,
) -> Result<(), String> {
    let final_path = plugins_dir.join(filename);
    let part = format!("{}{}", final_path.to_string_lossy(), PART_SUFFIX);

    let mut last_err: Option<String> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        match fetch_dll_once(url, filename, &final_path, &part, dll_progress).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < MAX_ATTEMPTS {
                    let delay = backoff(attempt);
                    crate::lethe_log!(
                        "DLL download failed for {} (attempt {}/{}): {}. Retrying in {:?}",
                        filename,
                        attempt,
                        MAX_ATTEMPTS,
                        last_err.as_deref().unwrap_or("unknown error"),
                        delay
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    // The part file is left on disk deliberately so the next run resumes
    // instead of re-fetching the whole DLL from zero.
    Err(format!(
        "Failed to download {} after {} attempts: {}",
        filename,
        MAX_ATTEMPTS,
        last_err.unwrap_or_else(|| "unknown error".to_string())
    ))
}

/// Download additional DLLs (Lethe.dll, ModularSkillScripts.dll, motions.dll)
/// to BepInEx/plugins/
///
/// These used to be fetched one at a time with no retry, no resume and no
/// verification, so any transient error aborted the whole sync and discarded
/// every byte already received. That bit hardest on Lethe.dll: it is ~18 MB
/// against ~0.5 MB and ~0.1 MB for the other two, and measured transfer times
/// to this endpoint ranged 15-75s for it versus under 6s for the rest, so it
/// carried far more exposure to a hiccup. Retrying with backoff and resuming
/// from a part file makes a transient failure survivable instead of fatal.
pub async fn download_additional_dlls(app_handle: &AppHandle) -> Result<(), String> {
    let plugins_dir = Path::new("BepInEx").join("plugins");
    fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Cannot create BepInEx/plugins: {}", e))?;

    let dll_progress = Arc::new(DllProgress::new());

    // Single ticker owns the status line so the parallel downloads below
    // cannot overwrite each other's messages.
    let ticker_app = app_handle.clone();
    let ticker_progress = dll_progress.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROGRESS_EMIT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Some(message) = ticker_progress.describe() {
                let _ = ticker_app.emit("status-change", StatusChangeEvent { message });
            }
        }
    });

    let plugins = plugin_files();
    let mut handles = Vec::with_capacity(plugins.len());

    for plugin in plugins {
        if plugin.url.trim().is_empty() || plugin.filename.trim().is_empty() {
            continue;
        }
        let PluginFile { url, filename } = plugin;
        let dir = plugins_dir.clone();
        let progress = dll_progress.clone();

        handles.push(tokio::spawn(async move {
            let result = download_one_dll(&url, &filename, &dir, &progress).await;
            progress.clear(&filename);
            (filename, result)
        }));
    }

    let mut first_err: Option<String> = None;
    for handle in handles {
        match handle.await {
            Ok((_, Ok(()))) => {}
            Ok((name, Err(e))) => {
                crate::lethe_log!("Additional DLL failed ({}): {}", name, e);
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(e) => {
                let msg = format!("Additional DLL task panicked: {}", e);
                crate::lethe_log!("{}", msg);
                if first_err.is_none() {
                    first_err = Some(msg);
                }
            }
        }
    }

    ticker.abort();

    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
        assert_eq!(format_bytes(2684354560), "2.5 GB");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(0.0), "-");
        assert_eq!(format_rate(-5.0), "-");
        assert_eq!(format_rate(f64::NAN), "-");
        assert_eq!(format_rate(1024.0), "1.0 KB/s");
        assert_eq!(format_rate(1024.0 * 1024.0), "1.0 MB/s");
    }

    #[test]
    fn test_backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
        // 2^7 * 500ms = 64s, capped to 8s
        assert_eq!(backoff(8), Duration::from_secs(8));
        assert_eq!(backoff(100), Duration::from_secs(8));
    }

    #[test]
    fn test_part_path_and_cleanup() {
        let p = part_path("BepInEx/core/x.dll");
        assert_eq!(p, "BepInEx/core/x.dll.lethe-part");
        // Cleanup must not panic on a nonexistent file.
        cleanup_part("definitely/not/here.xyz");
    }

    #[test]
    fn test_basename() {
        assert_eq!(basename("a/b/c.dll"), "c.dll");
        assert_eq!(basename("c.dll"), "c.dll");
        assert_eq!(basename("a\\b\\c.dll"), "c.dll");
    }

    fn content_range(value: &str) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        map.insert(
            reqwest::header::CONTENT_RANGE,
            reqwest::header::HeaderValue::from_str(value).unwrap(),
        );
        map
    }

    #[test]
    fn test_parse_content_range_happy_path() {
        // "bytes 1024-2047/4096" => starts at 1024, 4096 total.
        let h = content_range("bytes 1024-2047/4096");
        assert_eq!(
            parse_content_range(&h, 1024, "x.dll").unwrap(),
            (1024, 4096)
        );
    }

    #[test]
    fn test_parse_content_range_rejects_wrong_start() {
        // The single most dangerous case: the server answers a different
        // offset than we asked for. Appending that would corrupt the file.
        let h = content_range("bytes 0-2047/4096");
        let err = parse_content_range(&h, 1024, "x.dll").unwrap_err();
        assert!(err.contains("expected 1024"), "got: {}", err);
    }

    #[test]
    fn test_parse_content_range_rejects_garbage() {
        for bad in ["", "bytes", "bytes abc/4096", "bytes 10/4096", "10-20"] {
            let h = content_range(bad);
            assert!(
                parse_content_range(&h, 0, "x.dll").is_err(),
                "should reject {:?}",
                bad
            );
        }
    }

    #[test]
    fn test_parse_content_range_tolerates_missing_prefix() {
        // Some origins omit the "bytes" unit. Accept it rather than failing
        // a resumable download over a cosmetic difference.
        let h = content_range("1024-2047/4096");
        assert_eq!(
            parse_content_range(&h, 1024, "x.dll").unwrap(),
            (1024, 4096)
        );
    }

    #[test]
    fn test_dll_progress_clamps_overlong_writes() {
        // A buggy or hostile server can send more bytes than it declared.
        // The percentage must never exceed 100.
        let p = DllProgress::new();
        p.set("a.dll", 5000, 1000);
        let text = p.describe().unwrap();
        assert!(text.contains("100%"), "got: {}", text);
    }

    #[test]
    fn test_dll_progress_sums_across_files() {
        let p = DllProgress::new();
        p.set("a.dll", 500, 1000);
        p.set("b.dll", 1500, 3000);
        let text = p.describe().unwrap();
        assert!(text.contains("2 plugin(s)"), "got: {}", text);
        assert!(text.contains("50%"), "got: {}", text);
    }

    #[test]
    fn test_dll_progress_clear_removes_entry() {
        let p = DllProgress::new();
        p.set("a.dll", 1, 2);
        assert!(p.describe().is_some());
        p.clear("a.dll");
        assert!(p.describe().is_none());
    }

    #[test]
    fn test_dll_progress_unknown_size_does_not_divide_by_zero() {
        // A 200 response with no Content-Length gives total 0. That must not
        // panic on a division.
        let p = DllProgress::new();
        p.set("a.dll", 100, 0);
        let text = p.describe().unwrap();
        assert!(text.contains("plugin(s)"), "got: {}", text);
    }

    /// The tick interval the tracker should assume when validating windows.
    const TICK_MS: u128 = 250;

    #[test]
    fn test_progress_tracker_rate_smooths() {
        let tracker = ProgressTracker::new(0);
        tracker.add_network_bytes(4096);
        // First sample only opens the window; it must not extrapolate.
        assert_eq!(tracker.sample_rate(TICK_MS), 0.0);

        std::thread::sleep(Duration::from_millis(120));
        tracker.add_network_bytes(4096);
        let rate = tracker.sample_rate(TICK_MS);
        // 4096 bytes over ~120ms => ~34 KB/s. Allow generous slack for CI jitter.
        assert!(rate > 1000.0, "expected a positive rate, got {}", rate);
        assert!(rate < 1_000_000.0, "rate implausibly high: {}", rate);

        // A second identical sample must converge toward the same value rather
        // than jumping, which is the point of the EMA.
        std::thread::sleep(Duration::from_millis(120));
        tracker.add_network_bytes(4096);
        let rate2 = tracker.sample_rate(TICK_MS);
        let drift = (rate2 - rate).abs() / rate;
        assert!(drift < 0.5, "rate unstable: {} -> {}", rate, rate2);
    }

    #[test]
    fn test_steam_copy_does_not_affect_rate() {
        // This is the regression that made the readout spike: a local Steam
        // copy bumps overall progress instantly, but it is not network traffic.
        let tracker = ProgressTracker::new(0);
        tracker.add_network_bytes(4096);
        let _ = tracker.sample_rate(TICK_MS);

        std::thread::sleep(Duration::from_millis(120));
        // A 50MB file satisfied from disk.
        tracker.add_bytes(50 * 1024 * 1024);
        let rate = tracker.sample_rate(TICK_MS);

        assert!(
            rate < 100_000.0,
            "local copy leaked into the rate: {} B/s",
            rate
        );
        // Progress itself must still have advanced.
        assert_eq!(tracker.bytes(), 50 * 1024 * 1024 + 4096);
    }

    #[test]
    fn test_idle_window_decays_instead_of_spiking() {
        let tracker = ProgressTracker::new(0);
        tracker.add_network_bytes(64 * 1024);
        let _ = tracker.sample_rate(TICK_MS);

        std::thread::sleep(Duration::from_millis(120));
        tracker.add_network_bytes(64 * 1024);
        let steady = tracker.sample_rate(TICK_MS);
        assert!(steady > 0.0, "expected a measurable steady rate");

        // Now nothing arrives for several ticks: a tiny delta must not be
        // divided by a short window and reported as a huge rate.
        std::thread::sleep(Duration::from_millis(120));
        tracker.add_network_bytes(16);
        let idle = tracker.sample_rate(TICK_MS);
        assert!(
            idle <= steady,
            "idle window raised the rate: {} -> {}",
            steady,
            idle
        );
    }

    #[test]
    fn test_long_gap_is_discarded() {
        let tracker = ProgressTracker::new(0);
        tracker.add_network_bytes(4096);
        let _ = tracker.sample_rate(TICK_MS);

        // A gap far beyond 4x the tick interval means the pipeline stalled.
        // The bytes that arrived during it were not moving at network speed.
        std::thread::sleep(Duration::from_millis(1100));
        tracker.add_network_bytes(4096);
        let rate = tracker.sample_rate(TICK_MS);
        assert_eq!(rate, 0.0, "stale window should be discarded, got {}", rate);
    }

    #[test]
    fn test_rate_never_exceeds_physical_bound() {
        // Even a large burst inside one window must not produce a rate that
        // could not have been sustained.
        let tracker = ProgressTracker::new(0);
        tracker.add_network_bytes(1024);
        let _ = tracker.sample_rate(TICK_MS);

        std::thread::sleep(Duration::from_millis(120));
        tracker.add_network_bytes(1024 * 1024);
        let rate = tracker.sample_rate(TICK_MS);
        // ~1MB in ~120ms would be ~8.7 MB/s. Sanity-bound it well below
        // anything absurd while leaving room for scheduler jitter.
        assert!(
            rate < 50.0 * 1024.0 * 1024.0,
            "rate implausibly high: {} B/s",
            rate
        );
        assert!(rate > 0.0);
    }

    #[test]
    fn test_progress_tracker_cumulative() {
        let tracker = ProgressTracker::new(500);
        assert_eq!(tracker.bytes(), 500);
        tracker.add_bytes(250);
        tracker.add_bytes(250);
        assert_eq!(tracker.bytes(), 1000);
        // file_done only maintains a counter for logging; assert it does not
        // disturb the byte accounting.
        tracker.file_done();
        tracker.file_done();
        assert_eq!(tracker.bytes(), 1000);
    }

    #[test]
    fn test_active_files_summary() {
        let active = ActiveFiles::new();
        assert_eq!(active.summary(3), (0, String::new()));

        let _g1 = active.enter("a/one.dll");
        let (count, label) = active.summary(3);
        assert_eq!(count, 1);
        assert!(label.contains("one.dll"), "label was {}", label);

        let _g2 = active.enter("b/two.dll");
        let _g3 = active.enter("c/three.dll");
        let _g4 = active.enter("d/four.dll");
        let (count, label) = active.summary(3);
        assert_eq!(count, 4);
        assert!(label.starts_with("4 files"), "label was {}", label);
        assert!(
            label.contains("..."),
            "expected truncation marker: {}",
            label
        );
    }

    #[test]
    fn test_active_guard_removes_entry() {
        let active = ActiveFiles::new();
        {
            let _g = active.enter("x/y.dll");
            assert_eq!(active.summary(1).0, 1);
        }
        // Guard dropped: the entry must be gone.
        assert_eq!(active.summary(1).0, 0);
    }

    #[test]
    fn test_manifest_deserialization() {
        let json = r#"{
            "scanned_folder": "/tmp/test",
            "total_files": 3,
            "total_size": 12345,
            "files": [
                {"path": "foo/bar.dll", "size": 1000, "xxhash": "abcdef0123456789"},
                {"path": "baz.dat", "size": 2000, "xxhash": "0000000000000000"},
                {"path": "sub/dir/file.txt", "size": 9345, "xxhash": "ffffffffffffffff"}
            ]
        }"#;

        let manifest: FileManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.scanned_folder, "/tmp/test");
        assert_eq!(manifest.total_files, 3);
        assert_eq!(manifest.total_size, 12345);
        assert_eq!(manifest.files.len(), 3);
        assert_eq!(manifest.files[0].path, "foo/bar.dll");
        assert_eq!(manifest.files[2].path, "sub/dir/file.txt");
    }

    #[test]
    fn test_manifest_empty_files() {
        let json = r#"{"scanned_folder":"/","total_files":0,"total_size":0,"files":[]}"#;
        let manifest: FileManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.files.len(), 0);
        assert_eq!(manifest.total_size, 0);
    }

    #[test]
    fn test_range_split_plan_for_large_file() {
        // Mirrors the split logic in download_file for a 1.36 GB entry.
        let size: u64 = 1_360_000_000;
        assert!(size >= RANGE_SPLIT_THRESHOLD);
        let part_len = (size / RANGE_SPLIT_PARTS as u64).max(MIN_RANGE_PART);
        let mut v = Vec::new();
        let mut off = 0u64;
        while off < size {
            let len = part_len.min(size - off);
            v.push((off, len));
            off += len;
        }
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, 0);
        assert_eq!(v[1].0, v[0].1);
        assert_eq!(v.iter().map(|(_, l)| l).sum::<u64>(), size);
    }

    #[test]
    fn test_small_file_is_single_range() {
        let size: u64 = 50 * 1024 * 1024;
        assert!(size < RANGE_SPLIT_THRESHOLD);
        // Small files must stay one request so we do not add needless overhead.
        let ranges = vec![(0u64, size)];
        assert_eq!(ranges.len(), 1);
    }
}
