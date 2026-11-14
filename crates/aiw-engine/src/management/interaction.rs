//! Tools that talk to the person (`ask_user`, `aiw_secret_request`, `aiw_secret_clear`). Filled
//! in by the `interaction` work item.

use crate::Engine;
use aiw_tools::Tool;
use std::sync::Arc;

pub(crate) fn tools(_e: &Engine) -> Vec<Arc<dyn Tool>> {
    Vec::new()
}

#[cfg(test)]
mod tests {}
