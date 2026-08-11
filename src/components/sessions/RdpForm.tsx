import { Eye, EyeOff } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MdChevronRight } from "react-icons/md";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NumberInput } from "@/components/ui/number-input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { invoke } from "@/lib/invoke";
import type {
  RdpCertificatePolicy,
  RdpClipboardMode,
  RdpDisplayMode,
  SavedPassword,
} from "@/types/global";

interface RdpFormProps {
  host: string;
  setHost: (value: string) => void;
  port: number;
  setPort: (value: number) => void;
  username: string;
  setUsername: (value: string) => void;
  domain: string;
  setDomain: (value: string) => void;
  passwordId: string;
  setPasswordId: (value: string) => void;
  password: string;
  setPassword: (value: string) => void;
  hasPassword: boolean;
  setHasPassword: (value: boolean) => void;
  useNla: boolean;
  setUseNla: (value: boolean) => void;
  certificatePolicy: RdpCertificatePolicy;
  setCertificatePolicy: (value: RdpCertificatePolicy) => void;
  displayWidth: number;
  setDisplayWidth: (value: number) => void;
  displayHeight: number;
  setDisplayHeight: (value: number) => void;
  displayMode: RdpDisplayMode;
  setDisplayMode: (value: RdpDisplayMode) => void;
  clipboardMode: RdpClipboardMode;
  setClipboardMode: (value: RdpClipboardMode) => void;
  reconnectEnabled: boolean;
  setReconnectEnabled: (value: boolean) => void;
  reconnectMaxAttempts: number;
  setReconnectMaxAttempts: (value: number) => void;
}

function RequiredMark() {
  return <span className="ml-0.5 text-destructive">*</span>;
}

export function RdpForm({
  host,
  setHost,
  port,
  setPort,
  username,
  setUsername,
  domain,
  setDomain,
  passwordId,
  setPasswordId,
  password,
  setPassword,
  hasPassword,
  setHasPassword,
  useNla,
  setUseNla,
  certificatePolicy,
  setCertificatePolicy,
  displayWidth,
  setDisplayWidth,
  displayHeight,
  setDisplayHeight,
  displayMode,
  setDisplayMode,
  clipboardMode,
  setClipboardMode,
  reconnectEnabled,
  setReconnectEnabled,
  reconnectMaxAttempts,
  setReconnectMaxAttempts,
}: RdpFormProps) {
  const { t } = useTranslation();
  const [passwords, setPasswords] = useState<SavedPassword[]>([]);
  const [showPassword, setShowPassword] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  useEffect(() => {
    invoke<SavedPassword[]>("get_saved_passwords")
      .then(setPasswords)
      .catch(() => {});
  }, []);

  return (
    <div className="space-y-3 w-full">
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_8rem]">
        <div>
          <Label className="text-xs font-medium text-foreground/80">
            {t("dialog.host")}
            <RequiredMark />
          </Label>
          <Input
            className="mt-1 h-8 text-xs"
            placeholder="192.168.1.100"
            value={host}
            onChange={(event) => setHost(event.target.value)}
          />
        </div>
        <div>
          <Label className="text-xs font-medium text-foreground/80">
            {t("dialog.port")}
            <RequiredMark />
          </Label>
          <NumberInput
            className="mt-1 [&_button]:h-8 [&_button]:w-8 [&_input]:h-8 [&_input]:text-xs"
            value={port}
            onChange={setPort}
            min={1}
            max={65535}
          />
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div>
          <Label className="text-xs font-medium text-foreground/80">
            {t("dialog.username")}
            <RequiredMark />
          </Label>
          <Input
            className="mt-1 h-8 text-xs"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
          />
        </div>
        <div>
          <Label className="text-xs font-medium text-foreground/80">{t("dialog.rdpDomain")}</Label>
          <Input
            className="mt-1 h-8 text-xs"
            value={domain}
            onChange={(event) => setDomain(event.target.value)}
          />
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <div>
          <Label className="text-xs font-medium text-foreground/80">{t("dialog.password")}</Label>
          <div className="mt-1 flex gap-2">
            <Input
              className="h-8 text-xs"
              type={showPassword ? "text" : "password"}
              value={password}
              placeholder={hasPassword ? "********" : ""}
              disabled={!!passwordId}
              onChange={(event) => {
                setPassword(event.target.value);
                setHasPassword(false);
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="icon-sm"
              className="h-8 w-8"
              onClick={() => setShowPassword((value) => !value)}
            >
              {showPassword ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
            </Button>
          </div>
        </div>
        <div>
          <Label className="text-xs font-medium text-foreground/80">
            {t("dialog.savedPassword")}
          </Label>
          <Select
            value={passwordId || "inline"}
            onValueChange={(value) => setPasswordId(value === "inline" ? "" : value)}
          >
            <SelectTrigger className="mt-1 h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="inline">{t("dialog.passwordInline")}</SelectItem>
              {passwords.map((item) => (
                <SelectItem key={item.id} value={item.id}>
                  {item.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
        <CollapsibleTrigger className="group flex w-full items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground">
          <MdChevronRight
            className={`text-sm transition-transform duration-200 ${advancedOpen ? "rotate-90" : ""}`}
          />
          <span>{t("dialog.advancedConfig")}</span>
        </CollapsibleTrigger>
        <CollapsibleContent className="mt-3">
          <Tabs defaultValue="security" className="w-full">
            <TabsList className="grid h-8 w-full grid-cols-4 pointer-events-auto">
              <TabsTrigger value="security" className="text-xs">
                {t("dialog.rdpSecurity")}
              </TabsTrigger>
              <TabsTrigger value="display" className="text-xs">
                {t("dialog.rdpDisplay")}
              </TabsTrigger>
              <TabsTrigger value="clipboard" className="text-xs">
                {t("dialog.rdpClipboard")}
              </TabsTrigger>
              <TabsTrigger value="reconnect" className="text-xs">
                {t("dialog.rdpReconnect")}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="security" className="mt-3 border-0 outline-none">
              <div className="rounded-lg border bg-accent/25 p-3">
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="rounded-md border bg-background/70 px-3 py-2">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 space-y-0.5">
                        <div className="text-xs font-medium">{t("dialog.rdpUseNla")}</div>
                        <p className="text-[0.6875rem] leading-relaxed text-muted-foreground">
                          {t("dialog.rdpUseNlaDesc")}
                        </p>
                      </div>
                      <Switch checked={useNla} onCheckedChange={setUseNla} />
                    </div>
                  </div>
                  <div>
                    <Label className="text-xs font-medium text-foreground/80">
                      {t("dialog.rdpCertificatePolicy")}
                    </Label>
                    <Select
                      value={certificatePolicy}
                      onValueChange={(value) =>
                        setCertificatePolicy(value as RdpCertificatePolicy)
                      }
                    >
                      <SelectTrigger className="mt-1 h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="prompt">{t("dialog.rdpCertificatePrompt")}</SelectItem>
                        <SelectItem value="strict">{t("dialog.rdpCertificateStrict")}</SelectItem>
                        <SelectItem value="accept-temporarily">
                          {t("dialog.rdpCertificateTemporary")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="display" className="mt-3 border-0 outline-none">
              <div className="rounded-lg border bg-accent/25 p-3">
                <div className="grid gap-3 sm:grid-cols-3">
                  <div>
                    <Label className="text-xs font-medium text-foreground/80">
                      {t("dialog.rdpDisplayMode")}
                    </Label>
                    <Select
                      value={displayMode === "native" ? "fixed" : displayMode}
                      onValueChange={(value) => setDisplayMode(value as RdpDisplayMode)}
                    >
                      <SelectTrigger className="mt-1 h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="fit-window">{t("dialog.rdpDisplayFitWindow")}</SelectItem>
                        <SelectItem value="fixed">{t("dialog.rdpDisplayFixed")}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div>
                    <Label className="text-xs font-medium text-foreground/80">
                      {t("dialog.rdpWidth")}
                    </Label>
                    <NumberInput
                      className="mt-1 [&_button]:h-8 [&_button]:w-8 [&_input]:h-8 [&_input]:text-xs"
                      value={displayWidth}
                      onChange={setDisplayWidth}
                      min={640}
                      max={7680}
                    />
                  </div>
                  <div>
                    <Label className="text-xs font-medium text-foreground/80">
                      {t("dialog.rdpHeight")}
                    </Label>
                    <NumberInput
                      className="mt-1 [&_button]:h-8 [&_button]:w-8 [&_input]:h-8 [&_input]:text-xs"
                      value={displayHeight}
                      onChange={setDisplayHeight}
                      min={480}
                      max={4320}
                    />
                  </div>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="clipboard" className="mt-3 border-0 outline-none">
              <div className="rounded-lg border bg-accent/25 p-3">
                <div className="max-w-md">
                  <Label className="text-xs font-medium text-foreground/80">
                    {t("dialog.rdpClipboard")}
                  </Label>
                  <Select
                    value={clipboardMode}
                    onValueChange={(value) => setClipboardMode(value as RdpClipboardMode)}
                  >
                    <SelectTrigger className="mt-1 h-8 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="text-only">{t("dialog.rdpClipboardTextOnly")}</SelectItem>
                      <SelectItem value="disabled">{t("dialog.disabled")}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="reconnect" className="mt-3 border-0 outline-none">
              <div className="rounded-lg border bg-accent/25 p-3">
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="rounded-md border bg-background/70 px-3 py-2">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 space-y-0.5">
                        <div className="text-xs font-medium">{t("dialog.rdpAutoReconnect")}</div>
                        <p className="text-[0.6875rem] leading-relaxed text-muted-foreground">
                          {t("dialog.rdpAutoReconnectDesc")}
                        </p>
                      </div>
                      <Switch checked={reconnectEnabled} onCheckedChange={setReconnectEnabled} />
                    </div>
                  </div>
                  <div>
                    <Label className="text-xs font-medium text-foreground/80">
                      {t("dialog.rdpReconnectAttempts")}
                    </Label>
                    <NumberInput
                      className="mt-1 [&_button]:h-8 [&_button]:w-8 [&_input]:h-8 [&_input]:text-xs"
                      value={reconnectMaxAttempts}
                      onChange={setReconnectMaxAttempts}
                      min={0}
                      max={20}
                      disabled={!reconnectEnabled}
                    />
                  </div>
                </div>
              </div>
            </TabsContent>
          </Tabs>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
