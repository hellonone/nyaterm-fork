import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { invoke } from "@/lib/invoke";

export interface SshAgentAuthRequest {
  requestId: string;
  connectionName: string;
  username: string;
  endpoint: string;
  state: "pending" | "failed";
  error?: string | null;
  targetWindowLabel?: string | null;
}

interface SshAgentAuthDialogProps {
  request: SshAgentAuthRequest | null;
  onDone: (requestId: string) => void;
}

export function SshAgentAuthDialog({ request, onDone }: SshAgentAuthDialogProps) {
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!request) {
      setVisible(false);
      setSubmitError(null);
      return;
    }
    setSubmitError(null);
    const timer = window.setTimeout(() => setVisible(true), 300);
    return () => window.clearTimeout(timer);
  }, [request]);

  if (!request || !visible) return null;

  const respond = async (action: "retry" | "cancel") => {
    setSubmitting(true);
    setSubmitError(null);
    try {
      await invoke("respond_ssh_agent_auth", {
        requestId: request.requestId,
        action,
      });
      onDone(request.requestId);
    } catch {
      setSubmitError(
        t("sshAgentAuth.responseError", "The SSH Agent request is no longer available."),
      );
    } finally {
      setSubmitting(false);
    }
  };

  const pending = request.state === "pending";
  return (
    <AlertDialog open onOpenChange={() => undefined}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {pending
              ? t("sshAgentAuth.waitingTitle", "SSH Agent requires confirmation")
              : t("sshAgentAuth.failedTitle", "SSH Agent authentication failed")}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {pending
              ? t(
                  "sshAgentAuth.waitingDescription",
                  "Confirm the hardware key, enter its PIN, or approve the request in your SSH Agent.",
                )
              : request.error ||
                t("sshAgentAuth.failedDescription", "The SSH Agent request failed.")}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="space-y-1 rounded-md border bg-muted/30 px-3 py-2 text-xs">
          <div>{request.connectionName}</div>
          <div className="text-muted-foreground">
            {request.username} · {t(`sshAgentAuth.endpoint.${request.endpoint}`, request.endpoint)}
          </div>
          {submitError && <p className="text-xs text-destructive">{submitError}</p>}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={submitting} onClick={() => void respond("cancel")}>
            {t("common.cancel", "Cancel")}
          </AlertDialogCancel>
          {!pending && (
            <AlertDialogAction disabled={submitting} onClick={() => void respond("retry")}>
              {t("common.retry", "Retry")}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
