//! Shared HTTP client and URL helpers for the download pipeline.
//!
//! Rationale (measured, not assumed):
//! - A single shared `reqwest::Client` reuses TCP + TLS connections. The manifest
//!   has ~2000 files; creating a `Client` per file paid a fresh ~0.6s TLS
//!   handshake each time (measured RTT 164-269ms to the Cloudflare edge).
//! - The download endpoint is HTTP/1.1 only (no `h2` in the dependency graph),
//!   so connection reuse is the only multiplexing available.
//! - Trust comes from the **native OS root store**, not a baked-in CA bundle.
//!   The plugin DLLs are served from `api.lethelc.site`, which 307-redirects to
//!   a signed `cdn.discordapp.com` URL. On machines in mainland China that host
//!   is commonly remapped in `hosts` to a local TLS-intercepting proxy
//!   (steamcommunity_302 and friends) whose root CA is installed into the
//!   Windows store. `rustls-tls` alone embeds Mozilla's roots only, so every
//!   such request died with `invalid peer certificate: UnknownIssuer` — which
//!   is why only the DLLs failed while `files.lethelc.site` (Fly.io) worked.
//!   Reading the native store makes the client honour the same trust decisions
//!   as the rest of the system, including corporate/school roots.

use std::sync::OnceLock;
use std::time::Duration;

/// Global shared client. Built once on first use.
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Connect timeout. Kept low so a dead endpoint fails fast into the retry path
/// rather than stalling the whole sync.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Overall per-request timeout. Generous because the largest manifest entry is
/// 1.36 GB and the observed throughput floor was ~1 MB/s.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Idle connection kept in the pool. Matches the sequential download pattern
/// where consecutive files hit the same host.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Get the process-wide shared `reqwest::Client`, initialising it on first use.
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            // HTTP/1.1 only: the server does not negotiate h2 and the crate
            // graph has no h2. Setting this explicitly avoids probing for h2 on
            // every new connection.
            .http1_only()
            // Trust the operating system's root store rather than a bundle
            // compiled into the binary. The `rustls-tls-native-roots` feature
            // wires `hyper-rustls` to the platform verifier; spelling it out
            // here documents *why* it must not be swapped back to plain
            // `rustls-tls` (see the module note above).
            .tls_built_in_root_certs(true)
            .build()
            .unwrap_or_else(|e| {
                // The builder can only fail on TLS backend init. Fall back to a
                // default client rather than panicking at download time.
                crate::lethe_log!("Failed to build tuned HTTP client ({}), using default", e);
                reqwest::Client::new()
            })
    })
}

/// Percent-encode a single path segment, preserving `/` separators.
///
/// The manifest contains paths with spaces (e.g.
/// `LimbusCompany_Data/Resources/unity default resources`). The server happens
/// to accept a raw space, but `reqwest`/`url` do not guarantee that, so we
/// encode defensively. Only characters that must be escaped are touched, so
/// hashes and ordinary filenames stay human-readable in logs.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            // Unreserved characters plus the sub-delims that are valid in a path.
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => out.push(c),
            // Space is the only character that actually appears in this
            // manifest, but encode the rest of the unsafe set too.
            _ => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    out.push('%');
                    out.push_str(&format!("{:02X}", byte));
                }
            }
        }
    }
    out
}

/// Build the absolute download URL for a manifest entry path.
pub fn download_url(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url, encode_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_path_preserves_ordinary_names() {
        assert_eq!(
            encode_path("BepInEx/core/0Harmony.dll"),
            "BepInEx/core/0Harmony.dll"
        );
    }

    #[test]
    fn test_encode_path_encodes_spaces() {
        assert_eq!(
            encode_path("LimbusCompany_Data/Resources/unity default resources"),
            "LimbusCompany_Data/Resources/unity%20default%20resources"
        );
    }

    #[test]
    fn test_encode_path_encodes_other_unsafe_chars() {
        // '#' would truncate a URL fragment, '?' would start a query string.
        assert_eq!(encode_path("a#b"), "a%23b");
        assert_eq!(encode_path("a?b"), "a%3Fb");
        // Percent itself must be escaped so decoding is unambiguous.
        assert_eq!(encode_path("a%b"), "a%25b");
    }

    #[test]
    fn test_encode_path_is_idempotent_for_clean_input() {
        let clean =
            "LimbusCompany_Data/StreamingAssets/Assets/Sound/FMODBuilds/Desktop/2D306B.assets.bank";
        assert_eq!(encode_path(clean), clean);
    }

    #[test]
    fn test_download_url_composition() {
        let url = download_url("https://files.lethelc.site/download/", "a b.dll");
        assert_eq!(url, "https://files.lethelc.site/download/a%20b.dll");
    }

    #[test]
    fn test_client_is_reused() {
        // The whole point of the shared client: the same instance every time.
        let a = client() as *const reqwest::Client;
        let b = client() as *const reqwest::Client;
        assert_eq!(a, b);
    }
}
