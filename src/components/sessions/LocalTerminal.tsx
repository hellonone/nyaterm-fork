import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { MdChevronRight, MdFolderOpen } from "react-icons/md";
import { ConnectionRecordingSettings } from "@/components/sessions/ConnectionRecordingSettings";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { RecordingMode } from "@/types/global";

interface LocalTerminalProps {
  shellPath: string;
  setShellPath: (v: string) => void;
  shellArgs: string;
  setShellArgs: (v: string) => void;
  workingDir: string;
  setWorkingDir: (v: string) => void;
  recordingUseGlobal: boolean;
  setRecordingUseGlobal: (v: boolean) => void;
  recordingAutoStart: boolean;
  setRecordingAutoStart: (v: boolean) => void;
  recordingMode: RecordingMode;
  setRecordingMode: (v: RecordingMode) => void;
  encoding: string;
  setEncoding: (v: string) => void;
}

const BUILTIN_SHELL_PATHS = ["powershell.exe", "cmd.exe", "bash", "wsl.exe", "wt.exe"] as const;

export function LocalTerminal({
  shellPath,
  setShellPath,
  shellArgs,
  setShellArgs,
  workingDir,
  setWorkingDir,
  recordingUseGlobal,
  setRecordingUseGlobal,
  recordingAutoStart,
  setRecordingAutoStart,
  recordingMode,
  setRecordingMode,
  encoding,
  setEncoding,
}: LocalTerminalProps) {
  const { t } = useTranslation();
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const handlePickShellFile = async () => {
    const selected = await openFileDialog({
      multiple: false,
      directory: false,
      title: t("dialog.selectShellFileTitle", "Select Shell File"),
    });
    if (typeof selected === "string") {
      setShellPath(selected);
    }
  };

  const handleShellSelectChange = (val: string) => {
    if (val === "custom") {
      void handlePickShellFile();
      return;
    }

    setShellPath(val);
  };

  return (
    <div className="space-y-4 w-full">
      <div className="space-y-4">
        <div className="min-w-0">
          <Label className="text-[0.6875rem] text-muted-foreground">
            {t("dialog.shellPath", "Shell Path")}
          </Label>
          <div className="flex flex-col gap-2 sm:flex-row">
            <Select
              value={
                BUILTIN_SHELL_PATHS.includes(shellPath as (typeof BUILTIN_SHELL_PATHS)[number])
                  ? shellPath
                  : "custom"
              }
              onValueChange={handleShellSelectChange}
            >
              <SelectTrigger className="mt-1 h-8 w-full text-xs font-normal sm:w-36 sm:shrink-0">
                <SelectValue placeholder={t("dialog.selectShell", "Select Shell")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="powershell.exe">
                  {t("dialog.shellPowerShell", "PowerShell")}
                </SelectItem>
                <SelectItem value="cmd.exe">{t("dialog.shellCmd", "Command Prompt")}</SelectItem>
                <SelectItem value="bash">{t("dialog.shellBash", "Bash")}</SelectItem>
                <SelectItem value="wsl.exe">{t("dialog.shellWsl", "WSL")}</SelectItem>
                <SelectItem value="wt.exe">
                  {t("dialog.shellWindowsTerminal", "Windows Terminal")}
                </SelectItem>
                <SelectItem value="custom">{t("dialog.shellCustom", "Custom...")}</SelectItem>
              </SelectContent>
            </Select>
            <div className="mt-1 flex min-w-0 flex-1 overflow-hidden rounded-md border bg-transparent">
              <Input
                readOnly
                className="h-8 flex-1 cursor-default rounded-none border-0 text-xs focus-visible:ring-0"
                placeholder={t("dialog.selectShellFile", "Select shell file")}
                title={shellPath || t("dialog.selectShellFile", "Select shell file")}
                value={shellPath}
                onClick={() => {
                  if (
                    !BUILTIN_SHELL_PATHS.includes(shellPath as (typeof BUILTIN_SHELL_PATHS)[number])
                  ) {
                    void handlePickShellFile();
                  }
                }}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="h-8 rounded-none border-l px-2"
                onClick={() => {
                  void handlePickShellFile();
                }}
                title={t("dialog.selectShellFile", "Select shell file")}
                aria-label={t("dialog.selectShellFile", "Select shell file")}
              >
                <MdFolderOpen className="text-base" />
              </Button>
            </div>
          </div>
        </div>
      </div>
      <div>
        <Label className="text-[0.6875rem] text-muted-foreground">
          {t("dialog.shellArgs", "Shell Arguments")}
        </Label>
        <Input
          className="mt-1 h-8 text-xs"
          placeholder={t("dialog.shellArgsPlaceholder", "e.g. --login -i or -NoLogo")}
          value={shellArgs}
          onChange={(e) => setShellArgs(e.target.value)}
        />
      </div>
      <div>
        <Label className="text-[0.6875rem] text-muted-foreground">
          {t("dialog.workingDir", "Working Directory")}
        </Label>
        <Input
          className="mt-1 text-xs h-8"
          placeholder={t("dialog.workingDirPlaceholder", "e.g. C:\\Projects or ~/workspace")}
          value={workingDir}
          onChange={(e) => setWorkingDir(e.target.value)}
        />
      </div>
      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
        <CollapsibleTrigger className="group flex w-full items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground">
          <MdChevronRight
            className={`text-sm transition-transform duration-200 ${advancedOpen ? "rotate-90" : ""}`}
          />
          <span>{t("dialog.advancedConfig")}</span>
        </CollapsibleTrigger>
        <CollapsibleContent className="mt-3">
          <Tabs defaultValue="terminal" className="w-full">
            <TabsList className="grid h-8 w-full grid-cols-1 pointer-events-auto">
              <TabsTrigger value="terminal" className="text-xs">
                {t("dialog.encodingSettings")}
              </TabsTrigger>
            </TabsList>
            <TabsContent value="terminal" className="mt-3 border-0 outline-none">
              <div className="space-y-3 rounded-lg border bg-accent/25 p-3">
                <div className="max-w-md">
                  <Label className="text-xs font-medium text-foreground/80">
                    {t("connection.encoding")}
                  </Label>
                  <Select value={encoding} onValueChange={setEncoding}>
                    <SelectTrigger className="mt-1 h-8 w-full text-xs">
                      <SelectValue placeholder={t("connection.encodingFollowGlobal")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="global">{t("connection.encodingFollowGlobal")}</SelectItem>
                      <SelectItem value="UTF-8">UTF-8</SelectItem>
                      <SelectItem value="GBK">GBK</SelectItem>
                      <SelectItem value="GB2312">GB2312</SelectItem>
                      <SelectItem value="GB18030">GB18030</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <ConnectionRecordingSettings
                  useGlobal={recordingUseGlobal}
                  onUseGlobalChange={setRecordingUseGlobal}
                  autoStart={recordingAutoStart}
                  onAutoStartChange={setRecordingAutoStart}
                  mode={recordingMode}
                  onModeChange={setRecordingMode}
                />
              </div>
            </TabsContent>
          </Tabs>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
