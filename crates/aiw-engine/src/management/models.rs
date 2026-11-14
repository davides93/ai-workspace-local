//! Management tools for models and runtimes (`aiw_model_*`, `aiw_runtime_*`). Filled in by the
//! `mgmt-models` work item.

use crate::Engine;
use aiw_tools::Tool;
use std::sync::Arc;

pub(crate) fn tools(_e: &Engine) -> Vec<Arc<dyn Tool>> {
    Vec::new()
}

#[cfg(test)]
mod tests {}
