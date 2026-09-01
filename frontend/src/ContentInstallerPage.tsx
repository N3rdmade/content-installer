import { faExclamationTriangle } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Loader } from '@mantine/core';
import Alert from '@/elements/Alert.tsx';
import Group from '@/elements/Group.tsx';
import SegmentedControl from '@/elements/SegmentedControl.tsx';
import Text from '@/elements/Text.tsx';
import Title from '@/elements/Title.tsx';
import Select from '@/elements/input/Select.tsx';
import { useEffect, useState } from 'react';
import ServerContentContainer from '@/elements/containers/ServerContentContainer.tsx';
import { useServerStore } from '@/stores/server.ts';
import BrowseTab from './BrowseTab.tsx';
import ManageTab from './ManageTab.tsx';
import HybridModpacksTab from './HybridModpacksTab.tsx';
import { detectServer, getAvailableTabs, type ServerDetection } from './detect.ts';

type MainTab = 'browse' | 'manage' | 'modpacks';
type ContentTab = 'plugins' | 'mods' | 'datapacks';

const TAB_LABELS: Record<ContentTab, string> = {
  plugins: 'Plugins',
  mods: 'Mods',
  datapacks: 'Datapacks',
};

export default function ContentInstallerPage() {
  const { server } = useServerStore();

  const [detection, setDetection] = useState<ServerDetection | null>(null);
  const [detecting, setDetecting] = useState(true);
  const [mainTab, setMainTab] = useState<MainTab>('browse');
  const [contentTab, setContentTab] = useState<ContentTab>('plugins');
  const [manageRefreshKey, setManageRefreshKey] = useState(0);
  const [availableTabs, setAvailableTabs] = useState<ContentTab[]>(['plugins', 'mods', 'datapacks']);
  const [selectedWorld, setSelectedWorld] = useState<string>('world');

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

  const getInstallDir = (): string => {
    if (contentTab === 'datapacks') return `${selectedWorld}/datapacks`;
    return contentTab;
  };

  const showModpacks = detection
    ? detection.platform === 'mods' || detection.platform === 'both' || detection.platform === 'unknown' || detection.platform === 'vanilla'
    : true;

  return (
    <ServerContentContainer title='Content Installer'>
      <div className='ci-page'>
        <div className='ci-page-header'>
          <Title order={3}>{mainTab === 'modpacks' ? 'Modpacks' : TAB_LABELS[contentTab]}</Title>
        </div>

        {detecting ? (
          <div className='ci-center'>
            <Loader color='violet' size='lg' />
            <Text c='dimmed' mt='sm'>Detecting server type...</Text>
          </div>
        ) : (
          <>
            <div className='ci-tab-bar'>
              {availableTabs.length > 1 && mainTab !== 'modpacks' && (
                <SegmentedControl
                  value={contentTab}
                  onChange={(v) => setContentTab(v as ContentTab)}
                  data={availableTabs.map((t) => ({ value: t, label: TAB_LABELS[t] }))}
                  className='ci-content-tabs'
                />
              )}

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
            {detection && mainTab === 'modpacks' && <HybridModpacksTab detection={detection} />}
          </>
        )}
      </div>
    </ServerContentContainer>
  );
}
