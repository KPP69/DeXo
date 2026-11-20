import { useEffect, useState } from "react";
import { fetchAppStatus, type AppStatus } from "../services/appStatus";

interface AppStatusState {
  status: AppStatus | null;
  error: string | null;
}

/** Loads the backend startup status once on mount. */
export function useAppStatus(): AppStatusState {
  const [state, setState] = useState<AppStatusState>({
    status: null,
    error: null,
  });

  useEffect(() => {
    let cancelled = false;
    fetchAppStatus()
      .then((status) => {
        if (!cancelled) {
          setState({ status, error: null });
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setState({ status: null, error: String(err) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return state;
}
