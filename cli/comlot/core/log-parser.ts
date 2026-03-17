import type { LogLine } from "./logs-stream";

export type LogSeverity = "error" | "warn" | "info" | "debug";

export interface LogSlot {
  id: string; // Identifier para el slot (e.g. "10:15" o el index)
  count: number;
  highestSeverity: LogSeverity;
  lines: LogLine[];
}

export interface ServiceState {
  name: string;
  status: string;        // "running", "exited", etc
  containerId: string;
  slots: Record<string, LogSlot>;
}

// Configuración de la "ventana" de tiempo
// Por simplicidad, cortaremos los slots por minutos.
export function getSlotIdForTimestamp(date: Date): string {
  const h = date.getHours().toString().padStart(2, "0");
  const m = date.getMinutes().toString().padStart(2, "0");
  return `${h}:${m}`;
}

export function parseSeverity(content: string, streamType: string): LogSeverity {
  const lower = content.toLowerCase();
  
  if (streamType === "stderr") {
    // A veces stderr se usa para info, pero asumimos warn minimo si no hay un flag explicito
    if (lower.includes("error") || lower.includes("err!") || lower.includes("fatal") || lower.includes("panic")) {
      return "error";
    }
    return "warn";
  }

  if (lower.includes("error") || lower.includes("err!") || lower.includes("exception")) return "error";
  if (lower.includes("warn")) return "warn";
  if (lower.includes("debug") || lower.includes("trace")) return "debug";
  
  return "info";
}

// Helper para determinar qué severidad "gana"
const severityWeight: Record<LogSeverity, number> = {
  error: 4,
  warn: 3,
  info: 2,
  debug: 1,
};

export function getHighestSeverity(a: LogSeverity, b: LogSeverity): LogSeverity {
  return severityWeight[a] > severityWeight[b] ? a : b;
}
