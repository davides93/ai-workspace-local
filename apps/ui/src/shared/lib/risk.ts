import type { ApprovalRequest } from "../api/types";
import type { PillTone } from "../ui/Pill";

export const RISK_LABEL: Record<ApprovalRequest["risk"], string> = { low: "basso", medium: "medio", high: "alto" };
export const RISK_TONE: Record<ApprovalRequest["risk"], PillTone> = { low: "info", medium: "warn", high: "crit" };
