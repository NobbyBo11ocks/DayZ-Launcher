//! Shared HTTP helpers (D-160).
//!
//! `reqwest` is built with `gzip`, so a few hundred kilobytes on the wire can
//! decompress to gigabytes. `Response::bytes()` buffers all of it, and with
//! `panic = "abort"` an allocation failure kills the launcher outright, so every
//! body this app reads goes through a cap instead.

use reqwest::Response;

/// Reads a response body, failing as soon as it passes `max` bytes. `what` names
/// the download in the error the user sees.
pub async fn body_capped(mut resp: Response, max: usize, what: &str) -> Result<Vec<u8>, String> {
    let too_big = || format!("the {what} is larger than {} MB", max / (1024 * 1024));
    // A truthful Content-Length saves downloading anything at all; a lying one is
    // caught by the running total below.
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(too_big());
    }
    let hint = resp.content_length().unwrap_or(0).min(max as u64) as usize;
    let mut out = Vec::with_capacity(hint);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("{what} download failed: {e}"))?
    {
        if out.len() + chunk.len() > max {
            return Err(too_big());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
