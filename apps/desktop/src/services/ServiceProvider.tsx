import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { createServiceClient, type ConnectionState, type ServiceClient } from "./service-client";

type ServiceState = { status: "checking" } | ConnectionState;

const ServiceContext = createContext<ServiceState>({ status: "checking" });

export function ServiceProvider({
  children,
  client: injected,
}: {
  children: ReactNode;
  /** Tests inject a fake; production uses the default client. */
  client?: ServiceClient;
}) {
  const [client] = useState(() => injected ?? createServiceClient());
  const [state, setState] = useState<ServiceState>({ status: "checking" });
  useEffect(() => {
    let active = true;
    client.connect().then((next) => {
      if (active) setState(next);
    });
    return () => {
      active = false;
    };
  }, [client]);
  return <ServiceContext.Provider value={state}>{children}</ServiceContext.Provider>;
}

export function useServiceState(): ServiceState {
  return useContext(ServiceContext);
}
