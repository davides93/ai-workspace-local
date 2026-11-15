//! C ABI to the native WKWebView. Swift dispatches to the main queue; callers wait on a
//! worker thread. Request IDs let late callbacks arrive safely after a timeout.
use crate::native_call::{self, CallError};
use aiw_core::{Error, Result};
use serde_json::Value;
use std::ffi::c_char;
use std::time::Duration;

extern "C" {
    fn aiw_browser_call(id: u64, request: *const c_char, reply: extern "C" fn(u64, *const c_char));
}

pub fn call(method: &str, payload: Value) -> Result<Value> {
    native_call::call(aiw_browser_call, method, payload, Duration::from_secs(50)).map_err(|e| match e {
        CallError::Timeout => Error::Runtime("Il browser interno non ha risposto in tempo".into()),
        CallError::Failed(m) => Error::Runtime(m),
    })
}
