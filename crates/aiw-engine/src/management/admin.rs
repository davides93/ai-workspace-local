//! Management tools for settings, connectors, agents and memories (`aiw_setting_*`,
//! `aiw_connector_*`, `aiw_agent_*`, `aiw_memory_*`). Filled in by the `mgmt-admin` work item.

use crate::Engine;
use aiw_tools::Tool;
use std::sync::Arc;

pub(crate) fn tools(_e: &Engine) -> Vec<Arc<dyn Tool>> {
    Vec::new()
}

#[cfg(test)]
mod tests {}
