import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { useMcpServers, useTools } from "./api";
import { BrowserTab } from "./components/BrowserTab";
import { McpTab } from "./components/McpTab";
import { ToolsTab } from "./components/ToolsTab";
import { isBrowserTool, isComputerTool } from "./model/tools";
import "./capabilities.css";

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
        eyebrow="Capacità" title="Tool, MCP e browser"
        subtitle="Cosa possono fare gli agenti. Il rischio di ogni tool decide quando serve una conferma."
      />
      <Tabs
        idPrefix="cap" label="Capacità" value={tab} onChange={(t) => navigate(t === "tools" ? "/capabilities" : `/capabilities/${t}`)}
        tabs={[
          { id: "tools", label: "Tool", count: tools?.length },
          { id: "mcp", label: "Server MCP", count: servers?.length },
          { id: "browser", label: "Browser e computer-use", count: tools?.filter((t) => isBrowserTool(t) || isComputerTool(t)).length },
        ]}
      />
      <TabPanel idPrefix="cap" id={tab}>
        {tab === "tools" && (
          <Section title="Tool disponibili" hint="Disattiva un tool per toglierlo a tutti gli agenti.">
            <ToolsTab />
          </Section>
        )}
        {tab === "mcp" && <McpTab />}
        {tab === "browser" && <BrowserTab />}
      </TabPanel>
    </Page>
  );
}
