//! `aiw_inspect`: the Assistant reads the state of the app before it changes anything.

use crate::Engine;
use aiw_tools::Tool;
use std::sync::Arc;

pub(crate) fn tools(_e: &Engine) -> Vec<Arc<dyn Tool>> {
    Vec::new()
}
