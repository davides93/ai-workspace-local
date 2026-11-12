import { useKbCollections } from "../../documents/api";
import { Link } from "../../../app/router";
import type { AgentProfile } from "../../../shared/api/types";
import { withKnowledge } from "../model/agents";

interface Props {
  agent: AgentProfile;
  onChange: (next: AgentProfile) => void;
}

/**
 * "Documenti" (developer mode only): which collections this agent may search with `kb_search`.
 * None ticked means every collection, like the tool does by default.
 */
export function KnowledgeSection({ agent, onChange }: Props) {
  const collections = useKbCollections();
  const chosen = agent.knowledge ?? [];
  const list = collections.data ?? [];
  const gone = chosen.filter((id) => !list.some((c) => c.id === id));
  if (list.length === 0 && gone.length === 0) {
    return <p className="faint text-sm">Non c'è ancora nessuna raccolta. Creane una in <Link to="/documents">Documenti</Link>.</p>;
  }
  return (
    <fieldset className="kb-know">
      <legend className="field-label">Raccolte che può consultare</legend>
      {list.map((c) => (
        <label key={c.id}>
          <input type="checkbox" checked={chosen.includes(c.id)} onChange={(e) => onChange(withKnowledge(agent, c.id, e.target.checked))} />
          <span><b>{c.name}</b>{c.description && <span className="faint text-xs"> — {c.description}</span>}</span>
        </label>
      ))}
      {gone.map((id) => (
        <label key={id}>
          <input type="checkbox" checked onChange={() => onChange(withKnowledge(agent, id, false))} />
          <span className="faint">Raccolta non più disponibile ({id})</span>
        </label>
      ))}
      <p className="faint text-xs">Se non ne scegli nessuna, può consultarle tutte.</p>
    </fieldset>
  );
}
