//! The call pattern shared by the Swift pieces linked into the sandboxed macOS app (the browser, the
//! folders): a JSON request, a C callback with the JSON answer, a worker thread waiting for it. Request ids
//! let a late answer arrive safely after a timeout.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

/// `@_cdecl` entry point of a Swift piece.
pub(crate) type Abi = unsafe extern "C" fn(u64, *const c_char, extern "C" fn(u64, *const c_char));

/// Why a call did not return a value.
pub(crate) enum CallError {
    /// The Swift side did not answer in time.
    Timeout,
    /// It answered with an error, or with something unreadable.
    Failed(String),
}

type Reply = mpsc::SyncSender<std::result::Result<Value, String>>;
static PENDING: OnceLock<Mutex<HashMap<u64, Reply>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

extern "C" fn reply(id: u64, response: *const c_char) {
    if response.is_null() {
        return;
    }
    // Swift lends a NUL-terminated UTF-8 string for the duration of this callback only.
    let result = unsafe { CStr::from_ptr(response) }.to_str().map_err(|e| e.to_string()).and_then(|s| {
        let v: Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
        match v["error"].as_str() {
            Some(e) => Err(e.to_string()),
            None => Ok(v["result"].clone()),
        }
    });
    if let Some(tx) = PENDING.get_or_init(Default::default).lock().unwrap().remove(&id) {
        let _ = tx.send(result);
    }
}

pub(crate) fn call(abi: Abi, method: &str, payload: Value, timeout: Duration) -> Result<Value, CallError> {
    let request = CString::new(json!({"method": method, "payload": payload}).to_string()).map_err(|e| CallError::Failed(e.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::sync_channel(1);
    let pending = PENDING.get_or_init(Default::default);
    pending.lock().unwrap().insert(id, tx);
    // Swift copies/parses the request before returning, then replies (at once, or later from the main queue).
    unsafe { abi(id, request.as_ptr(), reply) };
    let result = rx.recv_timeout(timeout);
    pending.lock().unwrap().remove(&id);
    match result {
        Ok(r) => r.map_err(CallError::Failed),
        Err(_) => Err(CallError::Timeout),
    }
}
