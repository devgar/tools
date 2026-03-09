import React, { useEffect, useState, useMemo } from "react";
import { getComposeServices, executeContainerAction, type DockerContainer } from "../core/docker";
import { tailContainerLogs, type LogLine } from "../core/logs-stream";
import { getSlotIdForTimestamp, parseSeverity, getHighestSeverity, type ServiceState, type LogSeverity } from "../core/log-parser";

type ServiceSlotSelection = {
  service: string;
  slot_id?: string;
};

type AppContextType = {
  refreshServices: () => Promise<void>;
  toggleServiceState: (serviceName: string) => Promise<void>;
  services: ServiceState[];
  getServiceStats: (serviceName: string) => ServiceState | undefined;
}

type FocusedContextType = null | {
  services: string[];
  slot_id: string[];
  selection?: ServiceSlotSelection;
};

export const AppContext = React.createContext<AppContextType | undefined>(undefined);
export const FocusedContext = React.createContext<{ 
  focused: FocusedContextType; 
  isSelected: (service: string, slot_id?: string) => boolean; 
  setService: (service: string, log_id?: string, toggle?: boolean) => void; 
  setLogId: (log_id: string, toggle?: boolean) => void 
} | undefined>(undefined);


export function useApp() {
  const context = React.useContext(AppContext);
  if (context === undefined) throw new Error("useApp must be used within AppProvider");
  return context;
}

export function useFocused() {
  const context = React.useContext(FocusedContext);
  if (context === undefined) throw new Error("useFocused must be used within FocusedProvider");
  return context;
}

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [containers, setContainers] = useState<DockerContainer[]>([]);
  const [servicesMap, setServicesMap] = useState<Record<string, ServiceState>>({});

  const loadServices = async () => {
    try {
      const data = await getComposeServices();
      setContainers(data);

      setServicesMap(prev => {
        const next = { ...prev };
        for (const c of data) {
          // El nombre usualmente es "/proyecto_servicio_1"
          const nameRaw = c.Names[0] || "";
          // Extraemos "_servicio_" u obtenemos el label `com.docker.compose.service`
          const composeService = c.Labels["com.docker.compose.service"] || nameRaw.replace(/^\//, "");

          if (!next[composeService]) {
            next[composeService] = {
              name: composeService,
              containerId: c.Id,
              status: c.State,
              slots: {}
            };
          } else {
             // Actualizar status si ya existe
             next[composeService].status = c.State;
             next[composeService].containerId = c.Id;
          }
        }
        return next;
      });
    } catch (err) {
      console.error("Failed to load compose services", err);
    }
  };

  useEffect(() => {
    loadServices();
    // Refresco cada 5 segundos del estado general
    const interval = setInterval(loadServices, 5000);
    return () => clearInterval(interval);
  }, []);

  // Suscripción a Logs en tiempo real
  useEffect(() => {
    const unsubscribes: (() => void)[] = [];

    // Por cada container running, enganchamos el tail
    Object.values(servicesMap).forEach(service => {
      if (service.status === "running") {
        const unsub = tailContainerLogs(service.containerId, (line) => {
          setServicesMap((prev) => {
            const next = { ...prev };
            const s = next[service.name];
            if (!s) return prev;

            // Clonamos el servicio para mutabilidad React-friendly
            const updatedService = { ...s, slots: { ...s.slots } };
            const slotId = getSlotIdForTimestamp(line.timestamp);
            const severity = parseSeverity(line.content, line.streamType);

            if (!updatedService.slots[slotId]) {
              updatedService.slots[slotId] = { id: slotId, count: 0, highestSeverity: "info", lines: [] };
            }

            const slot = { ...updatedService.slots[slotId] };
            slot.count += 1;
            slot.highestSeverity = getHighestSeverity(slot.highestSeverity, severity);
            // Limitamos a 500 líneas por slot para no reventar la memoria
            if (slot.lines.length < 500) {
              slot.lines.push(line);
            }
            
            updatedService.slots[slotId] = slot;
            next[service.name] = updatedService;
            
            return next;
          });
        });
        unsubscribes.push(unsub);
      }
    });

    return () => {
      unsubscribes.forEach(u => u());
    };
  // Solo re-suscribimos si cambia el número de contenedores o status clave
  }, [containers.map(c => `${c.Id}-${c.State}`).join(",")]);


  const toggleServiceState = async (serviceName: string) => {
    const s = servicesMap[serviceName];
    if (!s) return;
    
    const action = s.status === "running" ? "stop" : "start";
    
    // Optimistic UI update
    setServicesMap(prev => ({
        ...prev,
        [serviceName]: { ...s, status: action === "start" ? "running" : "exited" }
    }));

    try {
      await executeContainerAction(s.containerId, action);
      await loadServices();
    } catch (e) {
      console.error(`Failed to ${action} ${serviceName}`, e);
      // Revertir optimistic en el próximo tick o loadServices
    }
  };

  const services = useMemo(() => Object.values(servicesMap), [servicesMap]);
  const getServiceStats = (name: string) => servicesMap[name];

  return (
    <AppContext.Provider value={useMemo(() => ({ refreshServices: loadServices, toggleServiceState, services, getServiceStats }), [services])}>
      {children}
    </AppContext.Provider>
  );
}

export function FocusedProvider({ children }: { children: React.ReactNode }) {
  const [focused, setFocused] = React.useState<FocusedContextType>(null);
  
  const setService = (service: string, slot_id?: string, toggle: boolean = false) => {
    setFocused((prev) => {
      let nextServices = [...(prev?.services || [])];
      let nextSlots = [...(prev?.slot_id || [])];

      if (toggle) {
        if (nextServices.includes(service)) {
          // Si ya está el servicio, y pinchamos en un slot, gestionamos el slot
          if (slot_id) {
            if (nextSlots.includes(slot_id)) {
              nextSlots = nextSlots.filter(id => id !== slot_id);
            } else {
              nextSlots.push(slot_id);
            }
          } else {
            // Si pinchamos solo el servicio, lo quitamos
            nextServices = nextServices.filter(s => s !== service);
          }
        } else {
          nextServices.push(service);
          if (slot_id) nextSlots.push(slot_id);
        }
      } else {
        // Selección simple: reemplaza todo
        nextServices = [service];
        nextSlots = slot_id ? [slot_id] : [];
      }

      return { services: nextServices, slot_id: nextSlots };
    });
  };
  
  const setLogId = (slot_id: string, toggle: boolean = false) => {
    setFocused((prev) => {
      let nextSlots = [...(prev?.slot_id || [])];
      if (toggle) {
        if (nextSlots.includes(slot_id)) {
          nextSlots = nextSlots.filter(id => id !== slot_id);
        } else {
          nextSlots.push(slot_id);
        }
      } else {
        nextSlots = [slot_id];
      }
      return { services: prev?.services || [], slot_id: nextSlots };
    });
  };

  const isSelected = (service: string, slot_id?: string) => {
    if (!focused) return false;
    if (!focused.services.includes(service)) return false;
    if (slot_id && focused.slot_id.length > 0) {
      return focused.slot_id.includes(slot_id);
    }
    return true;
  };

  return (
    <FocusedContext.Provider value={React.useMemo(() => ({ focused, isSelected, setService, setLogId }), [focused])}>
      {children}
    </FocusedContext.Provider>
  );
}
