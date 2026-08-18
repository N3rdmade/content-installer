import { faExclamationTriangle } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Loader } from '@mantine/core';
import Alert from '@/elements/Alert.tsx';
import Group from '@/elements/Group.tsx';
import SegmentedControl from '@/elements/SegmentedControl.tsx';
import Text from '@/elements/Text.tsx';
import Title from '@/elements/Title.tsx';
import Select from '@/elements/input/Select.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import { useCallback, useEffect, useRef, useState } from 'react';
import ServerContentContainer from '@/elements/containers/ServerContentContainer.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import BrowseTab from './BrowseTab.tsx';
import ManageTab from './ManageTab.tsx';
import ModpacksTab from './ModpacksTab.tsx';
import ModpackInstallStatus, {
  IDLE_MODPACK_PROGRESS,
  isModpackInstallActive,
  type ModpackProgress,
} from './ModpackInstallStatus.tsx';
import { detectServer, getAvailableTabs, type ServerDetection } from './detect.ts';

type MainTab = 'browse' | 'manage' | 'modpacks';
type ContentTab = 'plugins' | 'mods' | 'datapacks';

const TAB_LABELS: Record<ContentTab, string> = {
  plugins: 'Plugins',
  mods: 'Mods',
  datapacks: 'Datapacks',
};

export default function ContentInstallerPage() {
  const { addToast } = useToast();
  const { server } = useServerStore();

  const [detection, setDetection] = useState<ServerDetection | null>(null);
  const [detecting, setDetecting] = useState(true);
  const [mainTab, setMainTab] = useState<MainTab>('browse');
  const [contentTab, setContentTab] = useState<ContentTab>('plugins');
  const [manageRefreshKey, setManageRefreshKey] = useState(0);
  const [availableTabs, setAvailableTabs] = useState<ContentTab[]>(['plugins', 'mods', 'datapacks']);
  const [selectedWorld, setSelectedWorld] = useState<string>('world');
  const [modpackProgress, setModpackProgress] = useState<ModpackProgress>(IDLE_MODPACK_PROGRESS);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [dismissing, setDismissing] = useState(false);
  const previousModpackState = useRef<string | null>(null);
  const statusRequestSequence = useRef(0);
  const modpackInstallActive = isModpackInstallActive(modpackProgress);

  const modpackStatusUrl = `/api/client/servers/${server.uuid}/content-installer/modpack/status`;

  const refreshModpackProgress = useCallback(async (): Promise<ModpackProgress> => {
    const requestSequence = ++statusRequestSequence.current;
    const response = await fetch(modpackStatusUrl);
    if (!response.ok) throw new Error(await response.text() || `Status request failed: ${response.status}`);
    const next = await response.json() as ModpackProgress;
    if (requestSequence === statusRequestSequence.current) setModpackProgress(next);
    return next;
  }, [modpackStatusUrl]);

  useEffect(() => {
    statusRequestSequence.current += 1;
    previousModpackState.current = null;
    setModpackProgress(IDLE_MODPACK_PROGRESS);
    setConfirmCancel(false);
  }, [server.uuid]);

  // Reattach to an existing backend job immediately on mount. Keep a slow idle
  // poll so another browser tab can start a job, then poll actively while work
  // is running. Switching into the active cadence restarts this loop at once.
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const poll = async () => {
      let delay = 30_000;
      try {
        const next = await refreshModpackProgress();
        delay = isModpackInstallActive(next) ? 5_000 : 30_000;
      } catch {
        delay = 15_000;
      }

      if (!stopped) timer = setTimeout(poll, delay);
    };

    poll();
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [refreshModpackProgress, modpackInstallActive]);

  useEffect(() => {
    const previous = previousModpackState.current;
    if (previous && isModpackInstallActive({ state: previous })) {
      if (modpackProgress.state === 'done') {
        addToast(`${modpackProgress.modpack_name ?? 'Modpack'} installed successfully!`, 'success');
      } else if (modpackProgress.state === 'error') {
        addToast(`Modpack installation failed: ${modpackProgress.error ?? 'unknown error'}`, 'error');
      } else if (modpackProgress.state === 'cancelled') {
        addToast('Modpack installation cancelled.', 'success');
      }
    }
    previousModpackState.current = modpackProgress.state;
  }, [modpackProgress.state, modpackProgress.error, modpackProgress.modpack_name]);

  const cancelModpackInstall = async () => {
    setCancelling(true);
    try {
      const response = await fetch(
        `/api/client/servers/${server.uuid}/content-installer/modpack/cancel`,
        { method: 'POST' },
      );
      if (!response.ok) throw new Error(await response.text() || `Cancel failed: ${response.status}`);
      await refreshModpackProgress();
      setConfirmCancel(false);
    } catch (error) {
      addToast(`Could not cancel installation: ${error instanceof Error ? error.message : 'unknown error'}`, 'error');
    } finally {
      setCancelling(false);
    }
  };

  const dismissModpackStatus = async () => {
    setDismissing(true);
    try {
      const response = await fetch(modpackStatusUrl, { method: 'DELETE' });
      if (!response.ok) throw new Error(await response.text() || `Dismiss failed: ${response.status}`);
      statusRequestSequence.current += 1;
      setModpackProgress(IDLE_MODPACK_PROGRESS);
    } catch (error) {
      addToast(`Could not dismiss status: ${error instanceof Error ? error.message : 'unknown error'}`, 'error');
    } finally {
      setDismissing(false);
    }
  };

  // Detect server type on mount
  useEffect(() => {
    setDetecting(true);
    detectServer(
      server.uuid,
      server.egg.name,
      server.startup,
      server.image ?? '',
    ).then((result) => {
      setDetection(result);
      const tabs = getAvailableTabs(result.platform);
      setAvailableTabs(tabs);
      setContentTab(tabs[0]);
      setSelectedWorld(result.worldDir);
      const willShowModpacks = result.platform === 'mods' || result.platform === 'both'
        || result.platform === 'unknown' || result.platform === 'vanilla';
      if (!willShowModpacks) setMainTab('browse');
    }).finally(() => setDetecting(false));
  }, [server.uuid]);

  const onInstalled = () => setManageRefreshKey((k) => k + 1);

  /** Get the install directory for the current content type */
  const getInstallDir = (): string => {
    if (contentTab === 'datapacks') {
      return `${selectedWorld}/datapacks`;
    }
    return contentTab;
  };

  // Show modpacks tab for mod-capable servers or unknown
  const showModpacks = detection
    ? detection.platform === 'mods' || detection.platform === 'both' || detection.platform === 'unknown' || detection.platform === 'vanilla'
    : true;

  return (
    <ServerContentContainer title='Content Installer'>
      <div className='ci-page'>
        <div className='ci-page-header'>
          <Title order={3}>
            {mainTab === 'modpacks' ? 'Modpacks' : TAB_LABELS[contentTab]}
          </Title>
        </div>

        <ModpackInstallStatus
          progress={modpackProgress}
          onCancel={() => setConfirmCancel(true)}
          onDismiss={dismissModpackStatus}
          cancelling={cancelling}
          dismissing={dismissing}
        />

        {detecting ? (
          <div className='ci-center'>
            <Loader color='violet' size='lg' />
            <Text c='dimmed' mt='sm'>Detecting server type...</Text>
          </div>
        ) : (
          <>
            {/* Tab selectors */}
            <div className='ci-tab-bar'>
              {/* Content type selector */}
              {availableTabs.length > 1 && mainTab !== 'modpacks' && (
                <SegmentedControl
                  value={contentTab}
                  onChange={(v) => setContentTab(v as ContentTab)}
                  data={availableTabs.map((t) => ({ value: t, label: TAB_LABELS[t] }))}
                  className='ci-content-tabs'
                />
              )}

              {/* Main tab selector */}
              <SegmentedControl
                value={mainTab}
                onChange={(v) => setMainTab(v as MainTab)}
                data={[
                  { value: 'browse', label: 'Browse' },
                  { value: 'manage', label: 'Installed' },
                  ...(showModpacks ? [{ value: 'modpacks', label: 'Modpacks' }] : []),
                ]}
                className='ci-main-tabs'
              />
            </div>

            {/* World selector for datapacks */}
            {contentTab === 'datapacks' && mainTab !== 'modpacks' && detection && detection.worldDirs.length > 1 && (
              <Group gap='sm' mb='sm'>
                <Text size='sm' fw={500}>World:</Text>
                <Select
                  data={detection.worldDirs.map((w) => ({ value: w, label: w }))}
                  value={selectedWorld}
                  onChange={(v) => v && setSelectedWorld(v)}
                  w={200}
                  size='sm'
                />
              </Group>
            )}

            {/* No detection warning */}
            {detection?.platform === 'unknown' && (
              <Alert
                icon={<FontAwesomeIcon icon={faExclamationTriangle} />}
                color='yellow'
                variant='light'
                mt='sm'
                mb='sm'
              >
                Could not detect your server type. Start the server at least once so we can identify it.
                You can still browse and install content manually.
              </Alert>
            )}

            {/* Tab content */}
            {detection && mainTab === 'browse' && (
              <BrowseTab
                detection={detection}
                contentType={contentTab}
                installDir={getInstallDir()}
                onInstalled={onInstalled}
              />
            )}
            {detection && mainTab === 'manage' && (
              <ManageTab
                detection={detection}
                contentType={contentTab}
                installDir={getInstallDir()}
                refreshKey={manageRefreshKey}
              />
            )}
            {detection && mainTab === 'modpacks' && (
              <ModpacksTab
                progress={modpackProgress}
                refreshProgress={refreshModpackProgress}
              />
            )}
          </>
        )}

        <ConfirmationModal
          opened={confirmCancel}
          onClose={() => setConfirmCancel(false)}
          title={<Text fw={600}>Cancel modpack installation?</Text>}
          confirm='Cancel installation'
          confirmColor='red'
          onConfirmed={cancelModpackInstall}
        >
          <Text size='sm'>
            This stops any remaining installation work and removes temporary files. Files already downloaded,
            replaced, or deleted by a clean install will not be restored.
          </Text>
        </ConfirmationModal>
      </div>
    </ServerContentContainer>
  );
}
