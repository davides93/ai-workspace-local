import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { useMcpServers, useTools } from "./api";
import { BrowserTab } from "./components/BrowserTab";
import { McpTab } from "./components/McpTab";
import { ToolsTab } from "./components/ToolsTab";
import { isBrowserTool, isComputerTool } from "./model/tools";
import "./capabilities.css";

import { t as tr } from "../../shared/i18n";
type Tab = "tools" | "mcp" | "browser";
const TABS: Tab[] = ["tools", "mcp", "browser"];

/** `#/capabilities/:tab?` — what the agents can do: tools, MCP servers, browser and computer-use. */
export function CapabilitiesView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "tools";
  const tools = useTools().data;
  const servers = useMcpServers().data;
  return (
    <Page>
      <PageHeader
        eyebrow={tr("nav.group_capabilities")} title={tr("capabilities.tool_mcp_e_browser")}
        subtitle={tr("capabilities.cosa_possono_fare_gli_agenti")}
      />
      <Tabs
        idPrefix="cap" label={tr("capabilities.capacita")} value={tab} onChange={(t) => navigate(t === "tools" ? "/capabilities" : `/capabilities/${t}`)}
        tabs={[
          { id: "tools", label: tr("capabilities.tool"), count: tools?.length },
          { id: "mcp", label: tr("capabilities.server_mcp"), count: servers?.length },
          { id: "browser", label: tr("capabilities.browser_e_computer_use"), count: tools?.filter((t) => isBrowserTool(t) || isComputerTool(t)).length },
        ]}
      />
      <TabPanel idPrefix="cap" id={tab}>
        {tab === "tools" && (
          <Section title={tr("capabilities.tool_disponibili")} hint={tr("capabilities.disattiva_un_tool_per_toglierlo")}>
            <ToolsTab />
          </Section>
        )}
        {tab === "mcp" && <McpTab />}
        {tab === "browser" && <BrowserTab />}
      </TabPanel>
    </Page>
  );
}
