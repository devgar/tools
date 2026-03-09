import type { BoxRenderable, MouseEvent } from "@opentui/core";
import { useMemo } from "react";
import { useFocused, useApp } from "../context/app-state";
import { ServiceListItem } from "./service-list-item";


export function ServiceList() {
  const { services } = useApp();
  const { focused } = useFocused();

  // Ordenar alfabéticamente para mantener consistencia
  const sortedServices = useMemo(() => {
     return [...services].sort((a,b) => a.name.localeCompare(b.name));
  }, [services]);

  return (
    <box style={{flexDirection: "column", height: "100%", width: "100%"}}>
      <scrollbox style={{ height: "100%", width: "100%" }} flexDirection="column">
        {sortedServices.map((service) => (
          <ServiceListItem key={service.name} service={service} width={25} />
        ))}
        {sortedServices.length === 0 && <text style={{opacity: 0.5}}>Buscando contenedores Compose en CWD...</text>}
      </scrollbox>
    </box>
  );
}
