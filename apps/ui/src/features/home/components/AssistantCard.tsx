import { Link } from "../../../app/router";
import { Panel } from "../../../shared/ui";

/** Simple mode: which model answers, by its human name, and a way to change it. */
export function AssistantCard({ name }: { name: string }) {
  return (
    <Panel title="Il tuo assistente" icon="cube">
      <div className="hm-assistant">
        <span className="hm-assistant-name" title={name}>{name}</span>
        <Link to="/models" className="btn sm ghost">Cambia</Link>
      </div>
    </Panel>
  );
}
