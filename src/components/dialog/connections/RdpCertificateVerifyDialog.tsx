import { ShieldAlert, ShieldQuestion } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { invoke } from "@/lib/invoke";
import { logger } from "@/lib/logger";

export interface RdpCertificateVerifyRequest {
  requestId: string;
  sessionId: string;
  host: string;
  port: number;
  fingerprint: string;
  subject?: string | null;
  issuer?: string | null;
  validFrom?: string | null;
  validTo?: string | null;
  knownHostStatus: "match" | "changed" | "unknown" | string;
  targetWindowLabel?: string | null;
}

interface RdpCertificateVerifyDialogProps {
  request: RdpCertificateVerifyRequest | null;
  onDone: (requestId: string) => void;
}

export function RdpCertificateVerifyDialog({ request, onDone }: RdpCertificateVerifyDialogProps) {
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);
  const changed = request?.knownHostStatus === "changed";

  const respond = async (accepted: boolean, remember: boolean) => {
    if (!request || submitting) return;
    setSubmitting(true);
    try {
      await invoke("respond_rdp_certificate", {
        requestId: request.requestId,
        accepted,
        remember,
      });
      logger.info({
        domain: "security.flow",
        event: accepted ? "rdp_certificate.user_accepted" : "rdp_certificate.user_rejected",
        message: accepted ? "User accepted RDP certificate" : "User rejected RDP certificate",
        ids: { request_id: request.requestId, session_id: request.sessionId },
      });
    } catch (error) {
      logger.error({
        domain: "security.flow",
        event: "rdp_certificate.response_failed",
        message: "Failed to send RDP certificate response",
        ids: { request_id: request.requestId, session_id: request.sessionId },
        error,
      });
    }
    setSubmitting(false);
    onDone(request.requestId);
  };

  return (
    <Dialog
      disablePointerDismissal
      open={!!request}
      onOpenChange={(open) => {
        if (!open) void respond(false, false);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-sm">
            {changed ? (
              <ShieldAlert className="h-4 w-4 text-destructive" />
            ) : (
              <ShieldQuestion className="h-4 w-4 text-yellow-500" />
            )}
            {t("settings.rdpCertificateVerifyTitle")}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {changed
              ? t("settings.rdpCertificateVerifyChanged")
              : t("settings.rdpCertificateVerifyNew")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 overflow-hidden py-2 text-xs">
          <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
            <span className="text-muted-foreground">{t("settings.hostKeyVerifyHost")}</span>
            <span className="truncate font-mono" title={`${request?.host}:${request?.port}`}>
              {request?.host}:{request?.port}
            </span>
            <span className="text-muted-foreground">{t("settings.hostKeyVerifyFingerprint")}</span>
            <span className="break-all font-mono select-all">{request?.fingerprint}</span>
            <span className="text-muted-foreground">{t("settings.rdpCertificateSubject")}</span>
            <span className="truncate font-mono" title={request?.subject ?? ""}>
              {request?.subject || t("common.unknown")}
            </span>
            <span className="text-muted-foreground">{t("settings.rdpCertificateIssuer")}</span>
            <span className="truncate font-mono" title={request?.issuer ?? ""}>
              {request?.issuer || t("common.unknown")}
            </span>
            <span className="text-muted-foreground">{t("settings.rdpCertificateValidity")}</span>
            <span className="truncate font-mono">
              {request?.validFrom || t("common.unknown")} -{" "}
              {request?.validTo || t("common.unknown")}
            </span>
          </div>

          {changed && (
            <div className="rounded-md border border-destructive/50 bg-destructive/10 p-2 text-[0.6875rem] text-destructive">
              {t("settings.rdpCertificateVerifyWarning")}
            </div>
          )}
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            variant="ghost"
            size="sm"
            className="text-xs"
            onClick={() => void respond(false, false)}
            disabled={submitting}
          >
            {t("settings.hostKeyVerifyReject")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="text-xs"
            onClick={() => void respond(true, false)}
            disabled={submitting}
          >
            {t("settings.rdpCertificateAcceptOnce")}
          </Button>
          <Button
            size="sm"
            className="text-xs"
            variant={changed ? "destructive" : "default"}
            onClick={() => void respond(true, true)}
            disabled={submitting}
          >
            {t("settings.rdpCertificateAcceptRemember")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
