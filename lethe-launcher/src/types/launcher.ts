export interface CheckProgressPayload {
  files_checked: number;
  total_files: number;
  bytes_processed: number;
  total_bytes: number;
  current_file: string;
  percent: number;
}

export interface DownloadProgressPayload {
  bytes_downloaded: number;
  total_bytes: number;
  percent: number;
  current_file: string;
  /** Number of files being downloaded right now (parallel downloads). */
  active_files: number;
  /** Files still queued or in flight. */
  remaining_files: number;
  /** Smoothed aggregate throughput in bytes per second. */
  bytes_per_sec: number;
  /** Estimated seconds remaining, or null when not yet determinable. */
  eta_secs: number | null;
}

export interface StatusChangePayload {
  message: string;
}

export interface ErrorPayload {
  message: string;
}

export type SyncPhase =
  | 'idle'
  | 'manifest'
  | 'checking'
  | 'downloading'
  | 'complete'
  | 'launching'
  | 'error';

export interface ModInfo {
  name: string;
  enabled: boolean;
}
