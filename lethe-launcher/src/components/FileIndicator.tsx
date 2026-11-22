import type { SyncPhase } from "../types/launcher";

interface FileIndicatorProps {
  currentFile: string;
  phase: SyncPhase;
  /** Number of files downloading in parallel. */
  activeFiles?: number;
  /** Files still queued or in flight. */
  remainingFiles?: number;
}

export function FileIndicator({
  currentFile,
  phase,
  activeFiles = 0,
  remainingFiles = 0,
}: FileIndicatorProps) {
  if (!currentFile || phase === "idle" || phase === "complete") {
    return null;
  }

  const isChecking = phase === "checking";
  const prefix = isChecking ? "CHECKING" : "DOWNLOADING";

  // With parallel downloads the backend sends a summary such as
  // "3 files (a.dll, b.dll)" instead of a single path. The checking phase is
  // still single-file, so only the download phase needs the plural treatment.
  const displayName = currentFile.includes("/")
    ? currentFile.split("/").pop() || currentFile
    : currentFile.includes("\\")
      ? currentFile.split("\\").pop() || currentFile
      : currentFile;

  return (
    <div
      key={currentFile}
      className="animate-slide-left text-xs flex items-center gap-2 font-mono"
    >
      <span className="inline-block w-1.5 h-1.5 bg-red-700 animate-pulse" />
      <span className="text-stone-600 tracking-wider">{prefix}</span>
      <span className="text-stone-500 truncate max-w-xs">{displayName}</span>
      {!isChecking && activeFiles > 1 && (
        <span className="text-stone-700 tracking-wider">
          {activeFiles}/{remainingFiles}
        </span>
      )}
    </div>
  );
}
