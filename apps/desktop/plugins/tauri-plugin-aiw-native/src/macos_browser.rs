//! C ABI to the native WKWebView. Swift dispatches to the main queue; callers wait on a
//! worker thread. Request IDs let late callbacks arrive safely after a timeout.
use aiw_core::{Error, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

type Reply = mpsc::SyncSender<std::result::Result<Value, String>>;
static PENDING: OnceLock<Mutex<HashMap<u64, Reply>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

extern "C" {
    fn aiw_browser_call(id: u64, request: *const c_char, reply: extern "C" fn(u64, *const c_char));
}

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

pub fn call(method: &str, payload: Value) -> Result<Value> {
    let request = CString::new(json!({"method": method, "payload": payload}).to_string()).map_err(Error::other)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::sync_channel(1);
    let pending = PENDING.get_or_init(Default::default);
    pending.lock().unwrap().insert(id, tx);
    // Swift copies/parses the request before returning, then replies asynchronously.
    unsafe { aiw_browser_call(id, request.as_ptr(), reply) };
    let result = rx.recv_timeout(Duration::from_secs(50));
    pending.lock().unwrap().remove(&id);
    result.map_err(|_| Error::Runtime("Il browser interno non ha risposto in tempo".into()))?.map_err(Error::Runtime)
}
