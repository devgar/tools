import { useMemo, useRef, useEffect } from "react";
import { useFocused, useApp } from "../context/app-state";

export function LogViewer() {
    const { focused } = useFocused();
    const { getServiceStats } = useApp();

    const logsToDisplay = useMemo(() => {
        if (!focused || focused.services.length === 0) return [];

        let linesToRender: Array<{ color: string, time: string, content: string, source: string, timestamp: Date }> = [];

        for (const svcName of focused.services) {
            const svc = getServiceStats(svcName);
            if (!svc) continue;

            // Si hay slots seleccionados, mostramos esos. De lo contrario, todos los logs que existan en el buffer.
            let slots = focused.slot_id.length > 0
                ? focused.slot_id.map(id => svc.slots[id]).filter(Boolean)
                : Object.values(svc.slots);

            for (const slot of slots) {
                for (const line of slot.lines) {
                    // Aplicamos la misma lógica pseudo-color as parser para el render raw
                    let color = "white";
                    const contentCheck = line.content.toLowerCase();
                    if (contentCheck.includes("error") || contentCheck.includes("err!")) color = "brightRed";
                    else if (contentCheck.includes("warn")) color = "yellow";
                    else if (contentCheck.includes("debug")) color = "gray";

                    linesToRender.push({
                        time: line.timestamp.toISOString().substring(11, 19),
                        content: line.content.trim(),
                        color,
                        source: svcName,
                        timestamp: line.timestamp
                    });
                }
            }
        }

        // Ordenamos por timestamp real para mezclar logs de varios servicios
        return linesToRender.sort((a, b) => a.timestamp.getTime() - b.timestamp.getTime());

    }, [focused, getServiceStats]);

    if (!focused || focused.services.length === 0) {
        return (
            <box style={{ height: "100%", width: "100%", justifyContent: "center", alignItems: "center" }}>
                <text style={{ opacity: 0.5 }}>Selecciona un servicio para ver sus logs</text>
            </box>
        );
    }

    return (
        <box style={{ flexDirection: "column", width: "100%", height: "100%" }}>
            <text style={{ bold: true, fg: "cyan", marginBottom: 1 }}>Logs: {focused.services.join(", ")} {focused.slot_id.length > 0 ? ` [Slot: ${focused.slot_id.join(", ")}]` : ""}</text>
            <scrollbox style={{ height: "100%", width: "100%" }} flexDirection="column">
                {logsToDisplay.length === 0 ? (
                    <text style={{ opacity: 0.5 }}>No hay logs recolectados aún para la selección.</text>
                ) : (
                    logsToDisplay.map((log, i) => (
                        <box key={i} flexDirection="row" gap={1} style={{ width: "100%" }}>
                            <text style={{ fg: "gray", width: 8 }}>{log.time}</text>
                            <text style={{ fg: "cyan", width: 10 }}>[{log.source}]</text>
                            <text style={{ fg: log.color, wrap: "wrap" }}>{log.content}</text>
                        </box>
                    ))
                )}
            </scrollbox>
        </box>
    );
}
