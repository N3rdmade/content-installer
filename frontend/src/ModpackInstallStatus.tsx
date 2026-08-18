import { faCheck, faCircleXmark, faExclamationTriangle, faSpinner, faStop } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Badge from '@/elements/Badge.tsx';
import Button from '@/elements/Button.tsx';
import Card from '@/elements/Card.tsx';
import Group from '@/elements/Group.tsx';
import Progress from '@/elements/Progress.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';

export interface ModpackProgress {
  job_id?: string;
  state: string;
  total_files?: number;
  downloaded_files?: number;
  current_file?: string;
  error?: string | null;
  modpack_name?: string;
  version_name?: string;
  source?: string;
  started_at?: string;
  updated_at?: string;
}

export const IDLE_MODPACK_PROGRESS: ModpackProgress = { state: 'idle' };

export function isModpackInstallActive(progress: ModpackProgress | null | undefined): boolean {
  return (
    !!progress &&
    ['preparing', 'downloading_mods', 'applying_overrides', 'installing_loader', 'finalizing', 'cancelling'].includes(
      progress.state,
    )
  );
}

export function isModpackInstallTerminal(progress: ModpackProgress | null | undefined): boolean {
  return !!progress && ['done', 'error', 'cancelled'].includes(progress.state);
}

function stateLabel(progress: ModpackProgress): string {
  switch (progress.state) {
    case 'preparing':
      return 'Preparing modpack';
    case 'downloading_mods':
      return progress.total_files
        ? `Downloading mods (${progress.downloaded_files ?? 0}/${progress.total_files})`
        : 'Downloading mods';
    case 'applying_overrides':
      return 'Applying server configuration';
    case 'installing_loader':
      return 'Installing and checking the server loader';
    case 'finalizing':
      return 'Finalizing installation';
    case 'cancelling':
      return 'Cancelling installation';
    case 'done':
      return 'Installation complete';
    case 'error':
      return 'Installation failed';
    case 'cancelled':
      return 'Installation cancelled';
    default:
      return 'Modpack installation';
  }
}

interface ModpackInstallStatusProps {
  progress: ModpackProgress;
  onCancel?: () => void;
  onDismiss?: () => void;
  cancelling?: boolean;
  dismissing?: boolean;
}

export default function ModpackInstallStatus({
  progress,
  onCancel,
  onDismiss,
  cancelling = false,
  dismissing = false,
}: ModpackInstallStatusProps) {
  if (progress.state === 'idle') return null;

  const active = isModpackInstallActive(progress);
  const terminal = isModpackInstallTerminal(progress);
  const totalFiles = progress.total_files ?? 0;
  const determinate = progress.state === 'downloading_mods' && totalFiles > 0;
  const percentage = determinate ? Math.min(100, Math.round(((progress.downloaded_files ?? 0) / totalFiles) * 100)) : 0;
  const source = progress.source === 'curseforge' ? 'CurseForge' : progress.source === 'modrinth' ? 'Modrinth' : null;

  const icon =
    progress.state === 'done' ? (
      <FontAwesomeIcon icon={faCheck} color='#4ade80' />
    ) : progress.state === 'error' ? (
      <FontAwesomeIcon icon={faExclamationTriangle} color='#ef4444' />
    ) : progress.state === 'cancelled' ? (
      <FontAwesomeIcon icon={faCircleXmark} color='#9ca3af' />
    ) : (
      <FontAwesomeIcon icon={faSpinner} spin />
    );

  return (
    <Card p='md' className={`ci-install-status ci-install-status--${progress.state}`} role='status' aria-live='polite'>
      <Stack gap='sm'>
        <Group justify='space-between' align='flex-start' wrap='wrap'>
          <div className='ci-install-status-copy'>
            <Group gap='xs' align='center' wrap='wrap'>
              {icon}
              <Text fw={600}>{stateLabel(progress)}</Text>
              {source && (
                <Badge variant='light' color={source === 'CurseForge' ? 'orange' : 'green'} size='xs'>
                  {source}
                </Badge>
              )}
            </Group>
            {progress.modpack_name && (
              <Text size='sm' mt={4}>
                {progress.modpack_name}
                {progress.version_name ? (
                  <Text span c='dimmed'>
                    {' '}
                    · {progress.version_name}
                  </Text>
                ) : null}
              </Text>
            )}
          </div>

          <Group gap='xs'>
            {active && progress.state !== 'cancelling' && onCancel && (
              <Button
                size='compact-sm'
                variant='subtle'
                color='red'
                loading={cancelling}
                leftSection={<FontAwesomeIcon icon={faStop} />}
                onClick={onCancel}
              >
                Cancel
              </Button>
            )}
            {terminal && onDismiss && (
              <Button size='compact-sm' variant='default' loading={dismissing} onClick={onDismiss}>
                Dismiss
              </Button>
            )}
          </Group>
        </Group>

        {active && (
          <Progress
            value={percentage}
            indeterminate={!determinate}
            hourglass={false}
            color={progress.state === 'cancelling' ? 'gray' : undefined}
          />
        )}

        {progress.current_file && active && (
          <Text size='xs' c='dimmed' className='ci-install-status-detail'>
            {progress.current_file}
          </Text>
        )}
        {progress.error && (
          <Text size='xs' c='red'>
            {progress.error}
          </Text>
        )}
        {active && progress.state !== 'cancelling' && (
          <Text size='xs' c='dimmed'>
            Installation continues in the background if you leave or refresh this page.
          </Text>
        )}
        {progress.state === 'cancelled' && (
          <Text size='xs' c='dimmed'>
            Files already downloaded or replaced were not restored.
          </Text>
        )}
      </Stack>
    </Card>
  );
}
