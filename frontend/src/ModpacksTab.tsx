import { marked } from 'marked';
import { faArrowDown, faExclamationTriangle, faExternalLink, faSearch } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Loader } from '@mantine/core';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router';
import Alert from '@/elements/Alert.tsx';
import Group from '@/elements/Group.tsx';
import SegmentedControl from '@/elements/SegmentedControl.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import Badge from '@/elements/Badge.tsx';
import Button from '@/elements/Button.tsx';
import Card from '@/elements/Card.tsx';
import Checkbox from '@/elements/input/Checkbox.tsx';
import { Modal } from '@/elements/modals/Modal.tsx';
import Select from '@/elements/input/Select.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import { useServerCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import type { ServerDetection } from './detect.ts';
import { versionLabel } from './versions.ts';
import {
  CF_CLASS_MODPACKS,
  checkCurseForgeStatus,
  getCurseForgeDescription,
  getCurseForgeFiles,
  searchCurseForge,
  type CurseForgeFile,
  type CurseForgeProject,
} from './curseforge.ts';
import {
  formatDownloads,
  getProject,
  getPrimaryFile,
  getProjectVersions,
  searchProjects,
  timeAgo,
  type ModrinthProject,
  type ModrinthVersion,
  type SearchIndex,
} from './modrinth.ts';

interface ModpacksTabProps {
  detection: ServerDetection;
}

type ProviderSource = 'modrinth' | 'curseforge' | 'ftb' | 'atlauncher';
type Source = 'all' | ProviderSource;

interface ProviderVersion {
  id: string;
  label: string;
  gameVersion?: string | null;
  loader?: string | null;
  loaderVersion?: string | null;
  java?: number | null;
}

interface DisplayModpack {
  id: string;
  title: string;
  description: string;
  downloads: number;
  author: string;
  iconUrl: string | null;
  source: ProviderSource;
  websiteUrl?: string | null;
  loaders?: string[];
  gameVersion?: string | null;
  gallery?: Array<{ url: string; thumbnailUrl?: string }>;
  availableVersions?: ProviderVersion[];
  modrinthProject?: ModrinthProject;
  curseforgeProject?: CurseForgeProject;
}

const providerLabel: Record<ProviderSource, string> = {
  modrinth: 'Modrinth',
  curseforge: 'CurseForge',
  ftb: 'FTB',
  atlauncher: 'ATLauncher',
};

const providerColor: Record<ProviderSource, string> = {
  modrinth: 'green',
  curseforge: 'orange',
  ftb: 'blue',
  atlauncher: 'violet',
};

function asProviderVersions(data: unknown): ProviderVersion[] {
  if (!Array.isArray(data)) return [];
  return data.map((item) => {
    const row = item as Record<string, unknown>;
    const id = String(row.id ?? row.versionNumber ?? '');
    const gameVersion = typeof row.gameVersion === 'string'
      ? row.gameVersion
      : Array.isArray(row.gameVersions) ? String(row.gameVersions[0] ?? '') : null;
    const loader = typeof row.loader === 'string'
      ? row.loader
      : Array.isArray(row.loaders) ? String(row.loaders[0] ?? '') : null;
    return {
      id,
      label: String(row.displayName ?? row.name ?? row.versionNumber ?? id),
      gameVersion,
      loader,
      loaderVersion: typeof row.loaderVersion === 'string' ? row.loaderVersion : null,
      java: typeof row.java === 'number' ? row.java : null,
    };
  }).filter((version) => version.id !== '');
}

function mapExternalPack(raw: Record<string, unknown>, source: 'ftb' | 'atlauncher'): DisplayModpack {
  return {
    id: String(raw.id ?? raw.slug ?? ''),
    title: String(raw.name ?? 'Unknown'),
    description: String(raw.summary ?? raw.description ?? ''),
    downloads: Number(raw.downloadCount ?? 0),
    author: String(raw.author ?? providerLabel[source]),
    iconUrl: typeof raw.iconUrl === 'string' ? raw.iconUrl : null,
    source,
    websiteUrl: typeof raw.websiteUrl === 'string' ? raw.websiteUrl : null,
    loaders: Array.isArray(raw.loaders) ? raw.loaders.map(String) : [],
    gameVersion: typeof raw.gameVersions === 'string' ? raw.gameVersions : null,
    gallery: Array.isArray(raw.gallery)
      ? raw.gallery.map((item) => item as { url: string; thumbnailUrl?: string }).filter((item) => !!item.url)
      : [],
    availableVersions: asProviderVersions(raw.availableVersions),
  };
}

export default function ModpacksTab({ detection }: ModpacksTabProps) {
  const { addToast } = useToast();
  const { server, state, updateServer } = useServerStore();
  const canReinstall = useServerCan('settings.install');
  const navigate = useNavigate();

  const [source, setSource] = useState<Source>('all');
  const [cfAvailable, setCfAvailable] = useState<boolean | null>(null);
  const [query, setQuery] = useState('');
  const [sortBy, setSortBy] = useState<string>('downloads');
  const [results, setResults] = useState<DisplayModpack[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [providerWarning, setProviderWarning] = useState<string | null>(null);
  const searchTimer = useRef<ReturnType<typeof setTimeout>>(null);

  const [selectedModpack, setSelectedModpack] = useState<DisplayModpack | null>(null);
  const [modrinthVersions, setModrinthVersions] = useState<ModrinthVersion[]>([]);
  const [selectedModrinthVersion, setSelectedModrinthVersion] = useState<ModrinthVersion | null>(null);
  const [cfFiles, setCfFiles] = useState<CurseForgeFile[]>([]);
  const [selectedCfFile, setSelectedCfFile] = useState<CurseForgeFile | null>(null);
  const [providerVersions, setProviderVersions] = useState<ProviderVersion[]>([]);
  const [selectedProviderVersion, setSelectedProviderVersion] = useState<ProviderVersion | null>(null);
  const [detailBody, setDetailBody] = useState('');
  const [detailLoading, setDetailLoading] = useState(false);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [wipeFiles, setWipeFiles] = useState(true);
  const [deleteWorld, setDeleteWorld] = useState(false);
  const [acceptRisk, setAcceptRisk] = useState(false);

  const isRunning = state === 'running' || state === 'starting';

  useEffect(() => {
    checkCurseForgeStatus(server.uuid).then(setCfAvailable);
  }, [server.uuid]);

  const doModrinthSearch = useCallback(async (q: string, sort: string, page: number) => {
    const res = await searchProjects({
      query: q || undefined,
      projectType: 'modpack',
      index: sort as SearchIndex,
      offset: page * 20,
      limit: 20,
    });
    return {
      items: res.hits.map((p): DisplayModpack => ({
        id: p.project_id,
        title: p.title,
        description: p.description,
        downloads: p.downloads,
        author: p.author,
        iconUrl: p.icon_url,
        source: 'modrinth',
        modrinthProject: p,
      })),
      hasMore: (page + 1) * 20 < res.total_hits,
    };
  }, []);

  const doCurseForgeSearch = useCallback(async (q: string, sort: string, page: number) => {
    if (!cfAvailable) return { items: [] as DisplayModpack[], hasMore: false };
    const sortMap: Record<string, number> = {
      relevance: 1, downloads: 6, follows: 2, newest: 11, updated: 3,
    };
    const res = await searchCurseForge(server.uuid, {
      searchFilter: q || undefined,
      classId: CF_CLASS_MODPACKS,
      sortField: sortMap[sort] ?? 6,
      sortOrder: 'desc',
      index: page * 20,
      pageSize: 20,
    });
    return {
      items: res.data.map((p): DisplayModpack => ({
        id: String(p.id),
        title: p.name,
        description: p.summary,
        downloads: p.downloadCount,
        author: p.authors[0]?.name ?? 'Unknown',
        iconUrl: p.logo?.thumbnailUrl ?? null,
        source: 'curseforge',
        curseforgeProject: p,
      })),
      hasMore: (page + 1) * 20 < res.pagination.totalCount,
    };
  }, [cfAvailable, server.uuid]);

  const doExternalSearch = useCallback(async (provider: 'ftb' | 'atlauncher', q: string, page: number) => {
    const params = new URLSearchParams({ query: q, page: String(page), page_size: '20' });
    const endpoint = `/api/client/servers/${server.uuid}/content-installer/${provider}/search?${params}`;
    const response = await fetch(endpoint);
    if (!response.ok) throw new Error(await response.text() || `${providerLabel[provider]} search failed`);
    const body = await response.json() as { data?: Record<string, unknown>[]; hasMore?: boolean };
    return {
      items: (body.data ?? []).map((item) => mapExternalPack(item, provider)),
      hasMore: !!body.hasMore,
    };
  }, [server.uuid]);

  const searchOne = useCallback(async (provider: ProviderSource, q: string, sort: string, page: number) => {
    if (provider === 'modrinth') return doModrinthSearch(q, sort, page);
    if (provider === 'curseforge') return doCurseForgeSearch(q, sort, page);
    return doExternalSearch(provider, q, page);
  }, [doCurseForgeSearch, doExternalSearch, doModrinthSearch]);

  const doSearch = useCallback(async (q: string, sort: string, page: number) => {
    setLoading(true);
    setProviderWarning(null);
    try {
      if (source !== 'all') {
        const result = await searchOne(source, q, sort, page);
        setResults((previous) => page === 0 ? result.items : [...previous, ...result.items]);
        setHasMore(result.hasMore);
        return;
      }

      const providers: ProviderSource[] = ['modrinth', ...(cfAvailable ? ['curseforge' as const] : []), 'ftb', 'atlauncher'];
      const settled = await Promise.allSettled(providers.map((provider) => searchOne(provider, q, sort, page)));
      const good = settled.flatMap((result) => result.status === 'fulfilled' ? result.value.items : []);
      const failed = settled.filter((result) => result.status === 'rejected').length;
      const sorted = [...good].sort((a, b) => sort === 'newest' ? 0 : b.downloads - a.downloads);
      setResults((previous) => page === 0 ? sorted : [...previous, ...sorted]);
      setHasMore(settled.some((result) => result.status === 'fulfilled' && result.value.hasMore));
      if (failed > 0) setProviderWarning(`${failed} provider${failed === 1 ? '' : 's'} could not be reached. Results from the others are still shown.`);
    } catch (error) {
      addToast(`Search failed: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setLoading(false);
    }
  }, [addToast, cfAvailable, searchOne, source]);

  useEffect(() => {
    setResults([]);
    setHasMore(false);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => doSearch(query, sortBy, 0), 300);
    return () => { if (searchTimer.current) clearTimeout(searchTimer.current); };
  }, [query, sortBy, source, doSearch]);

  const loadMore = () => {
    const page = source === 'all'
      ? Math.max(1, Math.ceil(results.length / Math.max(20, cfAvailable ? 80 : 60)))
      : Math.floor(results.length / 20);
    doSearch(query, sortBy, page);
  };

  const openInstall = useCallback(async (modpack: DisplayModpack) => {
    setSelectedModpack(modpack);
    setVersionsLoading(true);
    setDetailLoading(true);
    setDetailBody(modpack.description);
    setWipeFiles(true);
    setDeleteWorld(false);
    setAcceptRisk(false);
    setModrinthVersions([]);
    setSelectedModrinthVersion(null);
    setCfFiles([]);
    setSelectedCfFile(null);
    setProviderVersions([]);
    setSelectedProviderVersion(null);

    try {
      if (modpack.source === 'modrinth' && modpack.modrinthProject) {
        const [details, versions] = await Promise.all([
          getProject(modpack.modrinthProject.project_id),
          getProjectVersions(modpack.modrinthProject.project_id),
        ]);
        setDetailBody(details.body ?? modpack.description);
        setModrinthVersions(versions);
        setSelectedModrinthVersion(versions.find((version) => version.featured) ?? versions[0] ?? null);
      } else if (modpack.source === 'curseforge' && modpack.curseforgeProject) {
        const [description, files] = await Promise.all([
          getCurseForgeDescription(server.uuid, modpack.curseforgeProject.id),
          getCurseForgeFiles(server.uuid, { modId: modpack.curseforgeProject.id, pageSize: 50 }),
        ]);
        setDetailBody(description || modpack.description);
        setCfFiles(files.data);
        setSelectedCfFile(files.data[0] ?? null);
      } else {
        let versions = modpack.availableVersions ?? [];
        if (versions.length === 0) {
          const params = new URLSearchParams(
            modpack.source === 'ftb' ? { pack_id: modpack.id } : { safe_name: modpack.id },
          );
          const response = await fetch(`/api/client/servers/${server.uuid}/content-installer/${modpack.source}/versions?${params}`);
          if (!response.ok) throw new Error(await response.text() || 'Could not load versions');
          const body = await response.json() as { data?: unknown[] };
          versions = asProviderVersions(body.data);
        }
        setProviderVersions(versions);
        setSelectedProviderVersion(versions[0] ?? null);
      }
    } catch (error) {
      addToast(`Failed to load versions: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setVersionsLoading(false);
      setDetailLoading(false);
    }
  }, [addToast, server.uuid]);

  const loaderName = useMemo(() => {
    if (selectedModpack?.source === 'modrinth' && selectedModrinthVersion) {
      return ['neoforge', 'forge', 'fabric', 'quilt'].find((loader) => selectedModrinthVersion.loaders?.includes(loader)) ?? null;
    }
    if (selectedModpack?.source === 'curseforge' && selectedCfFile) {
      const values = selectedCfFile.gameVersions?.map((value) => value.toLowerCase()) ?? [];
      return ['neoforge', 'forge', 'fabric', 'quilt'].find((loader) => values.some((value) => value.includes(loader))) ?? null;
    }
    return selectedProviderVersion?.loader ?? null;
  }, [selectedCfFile, selectedModpack?.source, selectedModrinthVersion, selectedProviderVersion]);

  const versionOptions = useMemo(() => {
    if (selectedModpack?.source === 'modrinth') {
      return modrinthVersions.map((version) => ({ value: version.id, label: versionLabel(version.version_number, version.game_versions) }));
    }
    if (selectedModpack?.source === 'curseforge') {
      return cfFiles.map((file) => ({ value: String(file.id), label: versionLabel(file.displayName, file.gameVersions) }));
    }
    return providerVersions.map((version) => ({ value: version.id, label: version.label }));
  }, [cfFiles, modrinthVersions, providerVersions, selectedModpack?.source]);

  const selectedVersionId = selectedModpack?.source === 'modrinth'
    ? selectedModrinthVersion?.id ?? null
    : selectedModpack?.source === 'curseforge'
      ? selectedCfFile ? String(selectedCfFile.id) : null
      : selectedProviderVersion?.id ?? null;

  const hasVersions = versionOptions.length > 0;
  const canInstall = selectedModpack?.source === 'modrinth'
    ? !!selectedModrinthVersion && !!getPrimaryFile(selectedModrinthVersion)
    : selectedModpack?.source === 'curseforge'
      ? !!selectedCfFile && !!selectedCfFile.downloadUrl
      : !!selectedProviderVersion;

  const doInstall = useCallback(async () => {
    if (!selectedModpack) return;
    setInstalling(true);
    try {
      let endpoint = '';
      let params: URLSearchParams;

      if (selectedModpack.source === 'modrinth') {
        if (!selectedModrinthVersion) return;
        const file = getPrimaryFile(selectedModrinthVersion);
        if (!file) throw new Error('No .mrpack file found.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/install`;
        params = new URLSearchParams({
          mrpack_url: file.url,
          wipe_files: String(wipeFiles),
          delete_world: String(deleteWorld),
          modpack_name: selectedModpack.title,
          version_name: selectedModrinthVersion.version_number,
        });
      } else if (selectedModpack.source === 'curseforge') {
        if (!selectedCfFile?.downloadUrl) throw new Error('This CurseForge version does not allow third-party downloads.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/cf-install`;
        params = new URLSearchParams({
          zip_url: selectedCfFile.downloadUrl,
          wipe_files: String(wipeFiles),
          delete_world: String(deleteWorld),
          modpack_name: selectedModpack.title,
          version_name: selectedCfFile.displayName,
        });
      } else if (selectedModpack.source === 'ftb') {
        if (!selectedProviderVersion) throw new Error('Select an FTB version.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/ftb-install`;
        params = new URLSearchParams({
          pack_id: selectedModpack.id,
          version_id: selectedProviderVersion.id,
          wipe_files: String(wipeFiles),
          delete_world: String(deleteWorld),
          modpack_name: selectedModpack.title,
          version_name: selectedProviderVersion.label,
        });
      } else {
        if (!selectedProviderVersion) throw new Error('Select an ATLauncher version.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/atlauncher-install`;
        params = new URLSearchParams({
          safe_name: selectedModpack.id,
          version: selectedProviderVersion.id,
          wipe_files: String(wipeFiles),
          delete_world: String(deleteWorld),
          modpack_name: selectedModpack.title,
        });
      }

      const response = await fetch(`${endpoint}?${params}`, { method: 'POST' });
      if (!response.ok) throw new Error(await response.text() || `Install failed: ${response.status}`);
      addToast(`Installing "${selectedModpack.title}". Opening the Console for live logs.`, 'success');
      setSelectedModpack(null);
      navigate(`/server/${server.uuidShort}`);
      updateServer({ status: 'installing' });
    } catch (error) {
      addToast(`Modpack install failed: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setInstalling(false);
    }
  }, [addToast, deleteWorld, navigate, selectedCfFile, selectedModpack, selectedModrinthVersion, selectedProviderVersion, server.uuid, server.uuidShort, updateServer, wipeFiles]);

  const sourceOptions = [
    { value: 'all', label: 'All Sources' },
    { value: 'modrinth', label: 'Modrinth' },
    ...(cfAvailable ? [{ value: 'curseforge', label: 'CurseForge' }] : []),
    { value: 'ftb', label: 'FTB' },
    { value: 'atlauncher', label: 'ATLauncher' },
  ];

  const worldDescription = detection.worldDirs.length > 0
    ? `Detected: ${detection.worldDirs.join(', ')}`
    : 'No existing Minecraft world was detected.';

  return (
    <div className='ci-browse ci-modpack-manager'>
      <div className='ci-search-bar'>
        <TextInput
          placeholder='Search modpacks across all sources...'
          leftSection={<FontAwesomeIcon icon={faSearch} />}
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          className='ci-search-input'
        />
        <Select
          data={[
            { value: 'relevance', label: 'Relevance' },
            { value: 'downloads', label: 'Downloads' },
            { value: 'newest', label: 'Newest' },
            { value: 'updated', label: 'Updated' },
          ]}
          value={sortBy}
          onChange={(value) => value && setSortBy(value)}
          w={140}
        />
      </div>

      <div className='ci-provider-row'>
        <SegmentedControl value={source} onChange={(value) => setSource(value as Source)} data={sourceOptions} />
        <Group gap='xs'>
          {detection.loader !== 'unknown' && <Badge variant='light'>Current: {detection.loader}</Badge>}
          {detection.mcVersion && <Badge variant='light'>MC {detection.mcVersion}</Badge>}
        </Group>
      </div>

      {providerWarning && <Alert color='yellow' variant='light'>{providerWarning}</Alert>}

      {loading && results.length === 0 ? (
        <div className='ci-center'><Loader color='violet' size='lg' /></div>
      ) : results.length === 0 ? (
        <Text c='dimmed' ta='center' mt='xl'>{query ? 'No modpacks found. Try a different search.' : 'No modpacks found.'}</Text>
      ) : (
        <>
          <div className='ci-results-grid ci-modpack-grid'>
            {results.map((modpack) => (
              <Card key={`${modpack.source}-${modpack.id}`} hoverable p='md' className='ci-project-card ci-modpack-card' onClick={() => openInstall(modpack)}>
                <div className='ci-card-header'>
                  {modpack.iconUrl ? <img src={modpack.iconUrl} alt='' className='ci-project-icon' /> : <div className='ci-project-icon ci-project-icon--placeholder' />}
                  <div className='ci-card-title'>
                    <Text fw={700} size='sm' lineClamp={1}>{modpack.title}</Text>
                    <Text size='xs' c='dimmed'>by {modpack.author}</Text>
                  </div>
                  <Badge variant='light' color={providerColor[modpack.source]} size='xs'>{providerLabel[modpack.source]}</Badge>
                </div>
                <div className='ci-card-body'><Text size='xs' c='dimmed' lineClamp={3}>{modpack.description}</Text></div>
                <div className='ci-card-footer'>
                  <Text size='xs' c='dimmed'>{modpack.downloads > 0 ? `${formatDownloads(modpack.downloads)} downloads` : providerLabel[modpack.source]}</Text>
                  <Group gap={4}>
                    {modpack.gameVersion && <Badge size='xs' variant='outline'>MC {modpack.gameVersion}</Badge>}
                    {modpack.loaders?.slice(0, 2).map((loader) => <Badge key={loader} size='xs' variant='outline'>{loader}</Badge>)}
                  </Group>
                </div>
              </Card>
            ))}
          </div>
          {hasMore && <Group justify='center' mt='md'><Button variant='subtle' onClick={loadMore} loading={loading}>Load More</Button></Group>}
        </>
      )}

      <Modal
        opened={!!selectedModpack}
        onClose={() => { if (!installing) setSelectedModpack(null); }}
        title={null}
        size='80%'
        padding='lg'
        classNames={{ header: 'ci-modal-header', body: 'ci-modal-body' }}
        closeOnClickOutside={!installing}
        closeOnEscape={!installing}
      >
        {selectedModpack && (
          <Stack gap='md'>
            <div className='ci-detail-top'>
              <div className='ci-detail-top-left'>
                {selectedModpack.iconUrl ? <img src={selectedModpack.iconUrl} alt='' className='ci-detail-icon' /> : <div className='ci-detail-icon ci-detail-icon--placeholder' />}
                <div className='ci-detail-meta'>
                  <Group gap='xs' align='center'>
                    <Text fw={700} size='lg'>{selectedModpack.title}</Text>
                    <Badge variant='light' color={providerColor[selectedModpack.source]} size='xs'>{providerLabel[selectedModpack.source]}</Badge>
                    {(selectedModpack.websiteUrl || selectedModpack.modrinthProject || selectedModpack.curseforgeProject) && (
                      <Button size='compact-xs' variant='subtle' leftSection={<FontAwesomeIcon icon={faExternalLink} />} onClick={(event: React.MouseEvent) => {
                        event.stopPropagation();
                        const url = selectedModpack.websiteUrl
                          ?? (selectedModpack.source === 'modrinth' && selectedModpack.modrinthProject ? `https://modrinth.com/modpack/${selectedModpack.modrinthProject.slug}` : null)
                          ?? (selectedModpack.source === 'curseforge' && selectedModpack.curseforgeProject ? `https://www.curseforge.com/minecraft/modpacks/${selectedModpack.curseforgeProject.slug}` : null);
                        if (url) window.open(url, '_blank', 'noopener');
                      }}>Open</Button>
                    )}
                  </Group>
                  <Group gap='xs'>
                    <Text size='xs' c='dimmed'>by {selectedModpack.author}</Text>
                    {selectedModpack.downloads > 0 && <Text size='xs' c='dimmed'>· {formatDownloads(selectedModpack.downloads)} downloads</Text>}
                    {selectedModpack.source === 'modrinth' && selectedModrinthVersion && <Text size='xs' c='dimmed'>· {timeAgo(selectedModrinthVersion.date_published)}</Text>}
                    {loaderName && <Badge variant='light' color='violet' size='xs'>{loaderName}</Badge>}
                  </Group>
                </div>
              </div>

              <div className='ci-detail-top-right'>
                {versionsLoading ? <Loader color='violet' size='xs' /> : !hasVersions ? <Text size='xs' c='dimmed'>No versions</Text> : (
                  <Select
                    placeholder='Version...'
                    data={versionOptions}
                    value={selectedVersionId}
                    onChange={(value) => {
                      if (selectedModpack.source === 'modrinth') setSelectedModrinthVersion(modrinthVersions.find((version) => version.id === value) ?? null);
                      else if (selectedModpack.source === 'curseforge') setSelectedCfFile(cfFiles.find((file) => String(file.id) === value) ?? null);
                      else setSelectedProviderVersion(providerVersions.find((version) => version.id === value) ?? null);
                    }}
                    searchable
                    size='sm'
                    w='min(440px, 100%)'
                    comboboxProps={{ width: 'max-content', position: 'bottom-end' }}
                    disabled={installing}
                  />
                )}
              </div>
            </div>

            {isRunning && <Alert icon={<FontAwesomeIcon icon={faExclamationTriangle} />} color='red' variant='light'>Stop your server before installing a modpack.</Alert>}
            {!canReinstall && <Alert color='yellow' variant='light'>You need the server reinstall permission to install modpacks.</Alert>}
            {selectedModpack.source === 'curseforge' && selectedCfFile && !selectedCfFile.downloadUrl && <Alert color='red' variant='light'>This modpack does not allow third-party downloads.</Alert>}

            {detailLoading ? <div className='ci-center'><Loader color='violet' size='sm' /></div> : detailBody ? (
              <div className='ci-detail-body' dangerouslySetInnerHTML={{
                __html: selectedModpack.source === 'curseforge'
                  ? detailBody
                  : (marked.parse(detailBody, { async: false, breaks: false, gfm: true }) as string),
              }} />
            ) : <Text size='sm' c='dimmed'>{selectedModpack.description}</Text>}

            {selectedModpack.gallery && selectedModpack.gallery.length > 0 && (
              <div className='ci-gallery-strip'>
                {selectedModpack.gallery.slice(0, 8).map((image, index) => (
                  <a href={image.url} target='_blank' rel='noreferrer' key={`${image.url}-${index}`} onClick={(event) => event.stopPropagation()}>
                    <img src={image.thumbnailUrl ?? image.url} alt='' className='ci-gallery-thumb' />
                  </a>
                ))}
              </div>
            )}

            {hasVersions && !versionsLoading && (
              <>
                <Card p='md' className='ci-install-plan'>
                  <Stack gap='xs'>
                    <Text fw={700}>Install plan</Text>
                    <Group gap='xs'>
                      <Badge variant='light'>{providerLabel[selectedModpack.source]}</Badge>
                      {loaderName && <Badge variant='light'>Loader: {loaderName}</Badge>}
                      {(selectedProviderVersion?.gameVersion ?? detection.mcVersion) && <Badge variant='light'>MC {selectedProviderVersion?.gameVersion ?? detection.mcVersion}</Badge>}
                      {selectedProviderVersion?.java && <Badge variant='light'>Java {selectedProviderVersion.java}</Badge>}
                    </Group>
                  </Stack>
                </Card>
                <Checkbox
                  label='Wipe old server / modpack files'
                  description='Recommended when switching packs. Keeps detected worlds and operator files such as server.properties, whitelist, bans and ops.'
                  checked={wipeFiles}
                  onChange={(event) => setWipeFiles(event.currentTarget.checked)}
                  color='red'
                  disabled={installing || isRunning}
                />
                <Checkbox
                  label='Delete existing world'
                  description={worldDescription}
                  checked={deleteWorld}
                  onChange={(event) => setDeleteWorld(event.currentTarget.checked)}
                  color='red'
                  disabled={installing || isRunning || detection.worldDirs.length === 0}
                />
                {deleteWorld && <Alert icon={<FontAwesomeIcon icon={faExclamationTriangle} />} color='red' variant='light'>World deletion is permanent unless you have a backup. Only directories containing level.dat are targeted.</Alert>}
                <Group justify='space-between' align='center' wrap='wrap'>
                  <Checkbox
                    label={deleteWorld ? 'I understand this will replace server files and delete the detected world' : 'I understand this will replace my server files'}
                    checked={acceptRisk}
                    onChange={(event) => setAcceptRisk(event.currentTarget.checked)}
                    disabled={installing || isRunning}
                  />
                  <Button
                    onClick={doInstall}
                    loading={installing}
                    disabled={!canReinstall || isRunning || !canInstall || !acceptRisk || !hasVersions}
                    color='red'
                    leftSection={<FontAwesomeIcon icon={faArrowDown} />}
                  >Install Modpack</Button>
                </Group>
              </>
            )}
          </Stack>
        )}
      </Modal>
    </div>
  );
}
