import React from "react";

type Service = {
    name: string;
}

type ServiceLogSlot = {
    service: string;
    slot: [Date, Date];
    id: string;
}

const START = new Date();
const AGO15 = new Date(START.getTime() -  15 * 1000);
const AGO30 = new Date(START.getTime() -  30 * 1000);

const buildServiceLogSlot = (service: string, slot: [Date, Date]): ServiceLogSlot => ({ service, slot, id: crypto.randomUUID() });


export const ServicesContext = React.createContext<{ services: Service[]; logSlots: ServiceLogSlot[] } | undefined>(undefined);

export function useServices() {
    const context = React.useContext(ServicesContext);
    if (context === undefined) {
        throw new Error("useServices must be used within a ServicesProvider");
    }
    return context;
}

export function ServicesProvider({ children }: { children: React.ReactNode }) {
    const [services, setServices] = React.useState<Service[]>([]);
    const [logSlots, setLogSlots] = React.useState<ServiceLogSlot[]>([]);
    // Mock data for demonstration
    React.useEffect(() => {
        setServices([
            { name: "AuthService" },
            { name: "PaymentService" },
            { name: "NotificationService" },
        ]);
        setLogSlots([
            buildServiceLogSlot("AuthService", [AGO30, AGO15]),
            buildServiceLogSlot("AuthService", [AGO15, START]),
            buildServiceLogSlot("PaymentService", [AGO30, AGO15]),
            buildServiceLogSlot("PaymentService", [AGO15, START]),
            buildServiceLogSlot("NotificationService", [AGO30, AGO15]),
            buildServiceLogSlot("NotificationService", [AGO15, START]),
        ]);
        setInterval(() => {
            const now = new Date();
            const newLogSlots = [
                buildServiceLogSlot("AuthService", [new Date(now.getTime() - 30 * 1000), now]),
                buildServiceLogSlot("PaymentService", [new Date(now.getTime() - 30 * 1000), now]),
                buildServiceLogSlot("NotificationService", [new Date(now.getTime() - 30 * 1000), now]),
            ];
            setLogSlots(newLogSlots);
        }, 15 * 1000);
    }, []);



    return (
        <ServicesContext.Provider value={{ services, logSlots }}>
            {children}
        </ServicesContext.Provider>
    );
}