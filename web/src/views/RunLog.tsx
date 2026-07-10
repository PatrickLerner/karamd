import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage } from "../api";
import { taskHref } from "../router";

// Simplified live log view for an ongoing `karamd run` execution (#046): polls
// the tail of the run's captured output and offers a cancel button.
export function RunLog({ id, tab }: { id: string; tab: string }) {
  const queryClient = useQueryClient();
  const logQ = useQuery({
    queryKey: ["runlog", id],
    queryFn: () => api.runLog(id),
    refetchInterval: 2000,
  });
  const cancel = useMutation({
    mutationFn: () => api.cancelRun(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["runs"] });
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
    },
  });

  const log = logQ.data?.log ?? "";
  const error = logQ.error
    ? errorMessage(logQ.error)
    : cancel.error
      ? errorMessage(cancel.error)
      : null;

  return (
    <div className="view terminal-view">
      <div className="terminal-bar">
        <a href={taskHref(tab, id)}>← Back to task</a>
        <button
          type="button"
          className="run-cancel"
          disabled={cancel.isPending}
          onClick={() => cancel.mutate()}
        >
          Cancel run
        </button>
      </div>
      {error && <p className="muted terminal-status">{error}</p>}
      <pre className="runlog-output">
        {log === "" ? "waiting for output…" : log}
      </pre>
    </div>
  );
}
