import { useMemo } from "react";
import { useFocused, useApp } from "../context/app-state";
import type { ServiceState, LogSeverity } from "../core/log-parser";

type Props = {
  service: ServiceState;
  width: number;
  opacity?: number;
};

// Mapa de colores base para Opentui (chalk equivalent)
const severityColors: Record<LogSeverity, string> = {
  error: "brightRed",
  warn: "yellow",
  info: "white",
  debug: "gray",
};

export function ServiceListItem({ service, width, opacity = 0.7 }: Props) {
  const { focused, setService, isSelected } = useFocused();
  const { toggleServiceState } = useApp();

  const titleProps = useMemo(() => ({
    selectable: true,
    onclick: () => setService(service.name),
    opacity: (focused && focused.services.includes(service.name)) ? 1 : opacity,
    onMouseDown: () => setService(service.name),
  }), [service.name, setService, focused]);

  const itemTextStyle = useMemo(
    () => (color?: string, slotId?: string) =>
      isSelected(service.name, slotId)
        ? {
          fg: "black",
          bg: color,
          opacity: 1,
        }
        : {
          fg: color,
          opacity,
        },
    [service.name, focused, opacity, isSelected],
  );

  // Convertimos los slots del objeto a un array predecible por orden cronológico (últimos 15 minutos e.g. o simplemente los que hay)
  // Como la app recoge logs en streaming, mostraremos los últimos 8 slots que se han detectado.
  const displaySlots = useMemo(() => {
    return Object.values(service.slots)
      .sort((a, b) => a.id.localeCompare(b.id))
      .slice(-8); // Muestra los últimos 8 minutos/ventanas
  }, [service.slots]);

  return (
    <box style={{ flexDirection: "row", gap: 0 }}>
      <box style={{ width: 6, flexDirection: "row" }}>
        <text
          style={{ fg: "brightGreen", marginRight: 0 }}
          onMouseDown={() => toggleServiceState(service.name)}
        >[S]</text>
        <text
          style={{ fg: "brightRed" }}
          onMouseDown={() => toggleServiceState(service.name)}
        >[X]</text>
      </box>
      <box style={{ width }} {...titleProps} onMouseDown={(e) => setService(service.name, undefined, e.modifiers.shift)}>
        <text style={{ fg: service.status === "running" ? "brightGreen" : "white" }}>
          {service.status === "running" ? "▶ " : "⏹ "}
          {service.name}
        </text>
      </box>
      <box flexDirection="row" gap={1}>
        {displaySlots.map((slot) => {
          const color = severityColors[slot.highestSeverity];
          const label = slot.count > 99 ? "+99" : slot.count.toString().padStart(2, "0");

          return (
            <text
              key={slot.id}
              onMouseDown={(e) => setService(service.name, slot.id, e.modifiers.shift)}
              style={itemTextStyle(color, slot.id)}
            >
              {label}
            </text>
          )
        })}
        {displaySlots.length === 0 && <text style={{ opacity: 0.5 }}>- -</text>}
      </box>
    </box>
  );
}
