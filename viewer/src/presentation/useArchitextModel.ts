import { useEffect, useState } from "react";
import { loadArchitectureModel } from "../adapters/fetchArchitectureData.js";
import { mutationFetch } from "../adapters/mutationAuth.js";
import { subscribeToDataEvents } from "../adapters/dataEvents.js";
import type { Model } from "../domain/architectureTypes.js";
import { dataRefreshNoticeForDirtyEditors } from "./releasePlanningModel.js";

const DATA_NOTICE_DISMISS_MS = 2600;

export type RecoveryResult = {
  ok?: boolean;
  mode?: string;
  output?: string;
  error?: string;
  reload?: boolean;
  repairs?: Array<{ summary?: string; file?: string; category?: string }>;
  validation?: { ok?: boolean; output?: string };
  status?: {
    installed?: boolean;
    needsMigration?: boolean;
    doctorRepairs?: Array<{ summary?: string; file?: string; category?: string }>;
    validation?: { ok?: boolean; output?: string };
  };
};

type UseArchitextModelOptions = {
  releasePlanningDirty: boolean;
  rulesEditorDirty: boolean;
  onModelLoaded: (loaded: Model, resetSelection: boolean) => void;
};

export function useArchitextModel({
  releasePlanningDirty,
  rulesEditorDirty,
  onModelLoaded
}: UseArchitextModelOptions) {
  const [model, setModel] = useState<Model | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState<string | null>(null);
  const [recoveryResult, setRecoveryResult] = useState<RecoveryResult | null>(null);
  const [dataNotice, setDataNotice] = useState<string | null>(null);
  const [dataIssue, setDataIssue] = useState<{ message: string; waiting: boolean } | null>(null);

  const applyLoadedModel = (loaded: Model, resetSelection = true) => {
    setModel(loaded);
    onModelLoaded(loaded, resetSelection);
    setError(null);
    setRecoveryResult(null);
  };

  const reloadFromRecovery = async () => {
    setRecoveryBusy("Reloading architecture data...");
    try {
      const loaded = await loadArchitectureModel();
      applyLoadedModel(loaded, !model);
    } catch (loadError) {
      const message = loadError instanceof Error ? loadError.message : String(loadError);
      setError(message);
      setRecoveryResult({ error: message });
    } finally {
      setRecoveryBusy(null);
    }
  };

  const requestRecoveryAction = async (action: "reload" | "status" | "doctor-dry-run" | "doctor-apply" | "sync-repair") => {
    if (action === "reload") {
      await reloadFromRecovery();
      return;
    }

    const labels = {
      status: "Checking Architext status...",
      "doctor-dry-run": "Checking deterministic doctor repairs...",
      "doctor-apply": "Applying deterministic doctor repairs...",
      "sync-repair": "Running constrained sync repair..."
    };
    setRecoveryBusy(labels[action]);
    try {
      const response = action === "status"
        ? await fetch("/api/status")
        : await mutationFetch(action === "sync-repair" ? "/api/sync-repair" : "/api/doctor", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(action === "doctor-apply" ? { apply: true } : action === "doctor-dry-run" ? { apply: false } : {})
        });
      const text = await response.text();
      const payload = text ? JSON.parse(text) as RecoveryResult : {};
      payload.mode = payload.mode ?? action;
      if (!response.ok && !payload.error) payload.error = `Recovery request failed: ${response.status} ${response.statusText}`;
      setRecoveryResult(payload);
      if (payload.reload && payload.ok) await reloadFromRecovery();
    } catch (recoveryError) {
      setRecoveryResult({ mode: action, error: recoveryError instanceof Error ? recoveryError.message : String(recoveryError) });
    } finally {
      setRecoveryBusy(null);
    }
  };

  const reloadArchitectureData = async () => {
    const loaded = await loadArchitectureModel();
    applyLoadedModel(loaded, false);
  };

  const reloadInvalidDataNow = async () => {
    try {
      await reloadArchitectureData();
      setDataIssue(null);
      setDataNotice("Architext data refreshed.");
    } catch (loadError) {
      const message = loadError instanceof Error ? loadError.message : String(loadError);
      setDataIssue({ message, waiting: true });
      setDataNotice(null);
    }
  };

  useEffect(() => {
    loadArchitectureModel()
      .then((loaded: Model) => applyLoadedModel(loaded))
      .catch((loadError: unknown) => {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      });
  }, []);

  useEffect(() => subscribeToDataEvents({
    onValid: async () => {
      const dirtyNotice = dataRefreshNoticeForDirtyEditors({ releasePlanningDirty, rulesEditorDirty });
      if (dirtyNotice) {
        setDataNotice(dirtyNotice);
        return;
      }
      try {
        const loaded = await loadArchitectureModel();
        applyLoadedModel(loaded, false);
        setDataIssue(null);
        setDataNotice("Architext data refreshed.");
      } catch (loadError) {
        setDataNotice(loadError instanceof Error ? loadError.message : String(loadError));
      }
    },
    onInvalid: async (payload: { output?: string }) => {
      const message = payload.output ?? "The JSON data was updated and left in an invalid state.";
      setDataIssue({ message, waiting: true });
      setDataNotice(null);
      setRecoveryResult({ validation: { ok: false, output: message }, reload: false });
    }
  }), [releasePlanningDirty, rulesEditorDirty]);

  useEffect(() => {
    if (!dataNotice) return undefined;
    const timeout = window.setTimeout(() => setDataNotice(null), DATA_NOTICE_DISMISS_MS);
    return () => window.clearTimeout(timeout);
  }, [dataNotice]);

  return {
    applyLoadedModel,
    dataIssue,
    dataNotice,
    error,
    model,
    recoveryBusy,
    recoveryResult,
    reloadArchitectureData,
    reloadInvalidDataNow,
    requestRecoveryAction,
    setDataIssue,
    setDataNotice,
    setModel
  };
}
