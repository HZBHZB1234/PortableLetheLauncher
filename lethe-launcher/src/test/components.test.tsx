import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatusPanel } from '../components/StatusPanel';
import { ProgressBar } from '../components/ProgressBar';
import { FileIndicator } from '../components/FileIndicator';
import { LoginButton, UserBadge } from '../components/LoginButton';
import { UpdateBanner } from '../components/UpdateBanner';

describe('StatusPanel', () => {
  it('renders the title', () => {
    render(<StatusPanel phase="idle" message="Initializing..." />);
    expect(screen.getByText('Initializing...')).toBeInTheDocument();
  });

  it('shows checking message with pulse class', () => {
    render(<StatusPanel phase="checking" message="Checking 100 files..." />);
    const msg = screen.getByText('Checking 100 files...');
    expect(msg).toBeInTheDocument();
    expect(msg.className).toContain('animate-pulse-glow');
  });

  it('shows complete in green', () => {
    render(<StatusPanel phase="complete" message="Update complete!" />);
    const msg = screen.getByText('Update complete!');
    expect(msg.className).toContain('text-amber-400');
  });

  it('shows error in red', () => {
    render(<StatusPanel phase="error" message="Something failed" />);
    const msg = screen.getByText('Something failed');
    expect(msg.className).toContain('text-red-500');
  });
});

describe('ProgressBar', () => {
  it('renders percentage correctly', () => {
    render(<ProgressBar percent={42.5} bytesProcessed={425000} totalBytes={1000000} />);
    expect(screen.getByText('42.5%')).toBeInTheDocument();
  });

  it('formats bytes correctly for small sizes', () => {
    render(<ProgressBar percent={10} bytesProcessed={500} totalBytes={5000} />);
    expect(screen.getByText('500 B / 4.9 KB')).toBeInTheDocument();
  });

  it('formats bytes correctly for MB', () => {
    render(<ProgressBar percent={50} bytesProcessed={5242880} totalBytes={10485760} />);
    expect(screen.getByText('5.0 MB / 10.0 MB')).toBeInTheDocument();
  });

  it('hides byte text when both are zero', () => {
    render(<ProgressBar percent={0} bytesProcessed={0} totalBytes={0} />);
    expect(screen.queryByText(/B/)).not.toBeInTheDocument();
  });

  it('shows 100% at completion', () => {
    render(<ProgressBar percent={100} bytesProcessed={1000000} totalBytes={1000000} />);
    expect(screen.getByText('100.0%')).toBeInTheDocument();
  });

  it('renders label when provided', () => {
    render(<ProgressBar percent={30} bytesProcessed={300} totalBytes={1000} label="Downloading..." />);
    expect(screen.getByText('Downloading...')).toBeInTheDocument();
  });

  // Speed / ETA display
  it('shows download speed while active', () => {
    render(
      <ProgressBar
        percent={50}
        bytesProcessed={5000000}
        totalBytes={10000000}
        bytesPerSec={1048576}
        etaSecs={5}
      />
    );
    expect(screen.getByText('1.0 MB/s')).toBeInTheDocument();
    // The ETA lives in a nested span, so match the combined text content.
    expect(screen.getByText(/5s left/)).toBeInTheDocument();
  });

  it('hides speed when not actively downloading', () => {
    render(<ProgressBar percent={0} bytesProcessed={0} totalBytes={1000} bytesPerSec={1048576} />);
    expect(screen.queryByText(/\/s$/)).not.toBeInTheDocument();
  });

  it('keeps the rate row but shows a placeholder when rate is zero', () => {
    render(<ProgressBar percent={50} bytesProcessed={5000} totalBytes={10000} bytesPerSec={0} />);
    // The row stays put during a stall instead of vanishing, so the user can
    // tell the download is alive but momentarily not receiving data.
    expect(screen.getByText('—')).toBeInTheDocument();
    expect(screen.queryByText(/\/s$/)).not.toBeInTheDocument();
  });

  it('formats ETA in minutes and hours', () => {
    render(
      <ProgressBar
        percent={10}
        bytesProcessed={1000}
        totalBytes={10000}
        bytesPerSec={10}
        etaSecs={90}
      />
    );
    expect(screen.getByText(/1m 30s left/)).toBeInTheDocument();
  });

  it('formats long ETA in hours', () => {
    render(
      <ProgressBar
        percent={10}
        bytesProcessed={1000}
        totalBytes={10000}
        bytesPerSec={1}
        etaSecs={3700}
      />
    );
    // etaSecs is supplied by the backend; 3700s renders as 1h 2m (1m 40s rounds up).
    expect(screen.getByText(/1h 2m left/)).toBeInTheDocument();
  });

  it('omits ETA when download is complete', () => {
    render(
      <ProgressBar
        percent={100}
        bytesProcessed={10000}
        totalBytes={10000}
        bytesPerSec={1048576}
        etaSecs={0}
      />
    );
    expect(screen.queryByText(/left/)).not.toBeInTheDocument();
  });

  it('shows no speed text for an invalid rate', () => {
    render(
      <ProgressBar
        percent={50}
        bytesProcessed={5000}
        totalBytes={10000}
        bytesPerSec={-1}
      />
    );
    expect(screen.queryByText(/\/s$/)).not.toBeInTheDocument();
  });
});

describe('FileIndicator', () => {
  it('renders nothing when idle', () => {
    const { container } = render(<FileIndicator currentFile="" phase="idle" />);
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing when complete', () => {
    const { container } = render(<FileIndicator currentFile="some/file.dll" phase="complete" />);
    expect(container.firstChild).toBeNull();
  });

  it('shows file name when checking', () => {
    render(<FileIndicator currentFile="BepInEx/core/0Harmony.dll" phase="checking" />);
    expect(screen.getByText('CHECKING')).toBeInTheDocument();
    expect(screen.getByText('0Harmony.dll')).toBeInTheDocument();
  });

  it('shows downloading prefix when downloading', () => {
    render(<FileIndicator currentFile="data/assets.bundle" phase="downloading" />);
    expect(screen.getByText('DOWNLOADING')).toBeInTheDocument();
    expect(screen.getByText('assets.bundle')).toBeInTheDocument();
  });

  it('handles backslash paths', () => {
    render(<FileIndicator currentFile="sub\\folder\\file.dll" phase="checking" />);
    expect(screen.getByText('file.dll')).toBeInTheDocument();
  });

  it('shows a parallel-count badge when several files download', () => {
    render(
      <FileIndicator
        currentFile="3 files (a.dll, b.dll, c.dll)"
        phase="downloading"
        activeFiles={3}
        remainingFiles={40}
      />
    );
    expect(screen.getByText('DOWNLOADING')).toBeInTheDocument();
    expect(screen.getByText('3/40')).toBeInTheDocument();
  });

  it('hides the parallel badge for a single active file', () => {
    render(
      <FileIndicator
        currentFile="assets.bundle"
        phase="downloading"
        activeFiles={1}
        remainingFiles={5}
      />
    );
    expect(screen.getByText('assets.bundle')).toBeInTheDocument();
    expect(screen.queryByText('1/5')).not.toBeInTheDocument();
  });

  it('does not show the parallel badge while checking', () => {
    render(
      <FileIndicator
        currentFile="BepInEx/core/0Harmony.dll"
        phase="checking"
        activeFiles={4}
        remainingFiles={40}
      />
    );
    expect(screen.queryByText('4/40')).not.toBeInTheDocument();
  });
});

describe('LoginButton', () => {
  it('renders login text', () => {
    render(<LoginButton onClick={() => {}} isLoading={false} />);
    expect(screen.getByText('Login with Discord')).toBeInTheDocument();
  });

  it('shows spinner when loading', () => {
    render(<LoginButton onClick={() => {}} isLoading={true} />);
    expect(screen.getByText('Waiting...')).toBeInTheDocument();
    expect(screen.queryByText('Login with Discord')).not.toBeInTheDocument();
  });

  it('disables button when loading', () => {
    render(<LoginButton onClick={() => {}} isLoading={true} />);
    expect(screen.getByRole('button')).toBeDisabled();
  });
});

describe('UserBadge', () => {
  it('shows logged in status', () => {
    render(<UserBadge onLogout={() => {}} />);
    expect(screen.getByText('Logged In')).toBeInTheDocument();
    expect(screen.getByText('Status')).toBeInTheDocument();
  });
});

describe('UpdateBanner', () => {
  const updateInfo = {
    version: '0.2.0',
    notes: 'Bug fixes and improvements',
    download_url: 'https://example.com/LimbusCompany.exe',
    sha256: 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
  };

  it('shows update available', () => {
    render(
      <UpdateBanner
        updateInfo={updateInfo}
        isDownloading={false}
        downloadError={null}
        onApplyUpdate={() => {}}
        onDismiss={() => {}}
      />
    );
    expect(screen.getByText('Update Available')).toBeInTheDocument();
    expect(screen.getByText('v0.2.0')).toBeInTheDocument();
    expect(screen.getByText('Bug fixes and improvements')).toBeInTheDocument();
  });

  it('shows update and later buttons', () => {
    render(
      <UpdateBanner
        updateInfo={updateInfo}
        isDownloading={false}
        downloadError={null}
        onApplyUpdate={() => {}}
        onDismiss={() => {}}
      />
    );
    expect(screen.getByText('Update')).toBeInTheDocument();
    expect(screen.getByText('Later')).toBeInTheDocument();
  });

  it('shows downloading state', () => {
    render(
      <UpdateBanner
        updateInfo={updateInfo}
        isDownloading={true}
        downloadError={null}
        onApplyUpdate={() => {}}
        onDismiss={() => {}}
      />
    );
    expect(screen.getByText('Downloading...')).toBeInTheDocument();
    expect(screen.queryByText('Update')).not.toBeInTheDocument();
  });

  it('shows error message', () => {
    render(
      <UpdateBanner
        updateInfo={updateInfo}
        isDownloading={false}
        downloadError="Network error"
        onApplyUpdate={() => {}}
        onDismiss={() => {}}
      />
    );
    expect(screen.getByText('Network error')).toBeInTheDocument();
  });
});
