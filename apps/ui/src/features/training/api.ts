import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { AdapterInfo, DatasetInfo, DatasetSpec, TrainJob, TrainJobRequest } from "../../shared/api/types";
import { isActive, TRAIN_POLL_MS } from "./model/training";

import { t } from "../../shared/i18n";
export function useTrainDoctor() {
  const api = useApi();
  return useQuery({ queryKey: qk.training.doctor(), queryFn: () => api.trainDoctor() });
}

export function useInstallTrainer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await api.installTrainer(id);
      for (;;) {
        await new Promise((r) => setTimeout(r, 1000));
        const job = await api.trainerInstall(id);
        if (!job) throw new Error(t("training.installazione_non_avviata"));
        if (job.state === "failed") throw new Error(job.error ?? t("training.installazione_fallita"));
        if (job.state === "done") return job;
      }
    },
    meta: { handled: true },
    onSuccess: () => {
      toast(t("training.trainer_installato"));
      void qc.invalidateQueries({ queryKey: qk.training.doctor() });
    },
  });
}

export function useDatasets() {
  const api = useApi();
  return useQuery({ queryKey: qk.training.datasets(), queryFn: () => api.datasets() });
}

/** The reason the engine refuses a dataset (too few examples…) is shown next to the form. */
export function useBuildDataset() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (spec: DatasetSpec) => api.buildDataset(spec),
    meta: { handled: true },
    onSuccess: (d) => {
      toast(t("training.dataset_creato", { name: d.name }));
      qc.setQueryData<DatasetInfo[]>(qk.training.datasets(), (list) => [d, ...(list ?? []).filter((x) => x.id !== d.id)]);
      void qc.invalidateQueries({ queryKey: qk.training.datasets() });
    },
  });
}

export function useDeleteDataset() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteDataset(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast(t("training.dataset_eliminato"));
      qc.setQueryData<DatasetInfo[]>(qk.training.datasets(), (list) => list?.filter((d) => d.id !== id));
      void qc.invalidateQueries({ queryKey: qk.training.datasets() });
    },
  });
}

/** Jobs, most recent first; refreshed while one of them is going. */
export function useTrainJobs() {
  const api = useApi();
  return useQuery({
    queryKey: qk.training.jobs(),
    queryFn: () => api.trainJobs(),
    refetchInterval: (q) => (q.state.data?.some(isActive) ? TRAIN_POLL_MS : 10_000),
  });
}

export function useStartTrainJob() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: TrainJobRequest) => api.startTrainJob(req),
    meta: { handled: true },
    onSuccess: (job) => {
      toast(t("training.addestramento_avviato"));
      qc.setQueryData<TrainJob[]>(qk.training.jobs(), (list) => [job, ...(list ?? []).filter((j) => j.id !== job.id)]);
      void qc.invalidateQueries({ queryKey: qk.training.jobs() });
    },
  });
}

export function useCancelTrainJob() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.cancelTrainJob(id),
    onSuccess: () => {
      toast(t("training.interruzione_richiesta"));
      void qc.invalidateQueries({ queryKey: qk.training.jobs() });
    },
  });
}

/** The tail of the log of a job; refreshed while the job is going. */
export function useTrainLog(id: string | undefined, active: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: qk.training.log(id ?? ""),
    queryFn: () => api.trainJobLog(id!, 200),
    enabled: !!id,
    refetchInterval: active ? TRAIN_POLL_MS : false,
  });
}

export function useAdapters() {
  const api = useApi();
  return useQuery({ queryKey: qk.training.adapters(), queryFn: () => api.adapters() });
}

export function useDeleteAdapter() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteAdapter(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast(t("training.adapter_eliminato"));
      qc.setQueryData<AdapterInfo[]>(qk.training.adapters(), (list) => list?.filter((a) => a.id !== id));
      void qc.invalidateQueries({ queryKey: qk.training.adapters() });
    },
  });
}
