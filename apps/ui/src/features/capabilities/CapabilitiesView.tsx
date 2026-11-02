import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { McpServers } from "./components/McpServers";
import { isBrowserTool, ToolsTable } from "./components/ToolsTable";

type Tab = "tools" | "mcp" | "browser";
const TABS: Tab[] = ["tools", "mcp", "browser"];

export function CapabilitiesView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "tools";
  return (
    <Page>
      <PageHeader
        eyebrow="Capacità" title="Tool, MCP e browser"
        subtitle="Cosa possono fare gli agenti. Il rischio decide quando serve una conferma."
      />
      <Tabs
        idPrefix="cap" label="Capacità" value={tab} onChange={(t) => navigate(t === "tools" ? "/capabilities" : `/capabilities/${t}`)}
        tabs={[{ id: "tools", label: "Tool" }, { id: "mcp", label: "Server MCP" }, { id: "browser", label: "Browser e computer-use" }]}
      />
      <TabPanel idPrefix="cap" id={tab}>
        {tab === "tools" && (
          <Section title="Tool disponibili" hint="Disattiva un tool per toglierlo a tutti gli agenti.">
            <ToolsTable />
          </Section>
        )}
        {tab === "mcp" && (
          <Section title="Server MCP" hint={<>stdio (solo desktop) o streamable HTTP; i loro tool diventano <code>mcp__server__tool</code>.</>}>
            <McpServers />
          </Section>
        )}
        {tab === "browser" && (
          <Section title="Browser e computer-use" hint="Gli agenti che controllano il browser o il desktop chiedono sempre conferma; il kill switch li ferma subito.">
            <ToolsTable filter={isBrowserTool} empty="Nessun tool di browser o computer-use disponibile." />
          </Section>
        )}
      </TabPanel>
    </Page>
  );
}
