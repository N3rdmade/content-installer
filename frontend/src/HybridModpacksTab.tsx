import { marked } from 'marked';
import { faArrowDown, faExclamationTriangle, faExternalLink, faSearch } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Loader } from '@mantine/core';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router';
import createBackup from '@/api/server/backups/createBackup.ts';
import getBackups from '@/api/server/backups/getBackups.ts';
import Alert from '@/elements/Alert.tsx';
import Badge from '@/elements/Badge.tsx';
import Button from '@/elements/Button.tsx';
import Card from '@/elements/Card.tsx';
import Group from '@/elements/Group.tsx';
import SegmentedControl from '@/elements/SegmentedControl.tsx';
import Stack from '@/elements/Stack.tsx';
import Text from '@/elements/Text.tsx';
import Checkbox from '@/elements/input/Checkbox.tsx';
import Select from '@/elements/input/Select.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import { Modal } from '@/elements/modals/Modal.tsx';
import { useServerCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import type { ServerDetection } from './detect.ts';
import {
  CF_CLASS_MODPACKS,
  CF_LOADER_FABRIC,
  CF_LOADER_FORGE,
  CF_LOADER_NEOFORGE,
  CF_LOADER_QUILT,
  checkCurseForgeStatus,
  getCurseForgeDescription,
  getCurseForgeFiles,
  searchCurseForge,
  type CurseForgeFile,
  type CurseForgeProject,
} from './curseforge.ts';
import {
  formatDownloads,
  getPrimaryFile,
  getProject,
  getProjectVersions,
  searchProjects,
  timeAgo,
  type ModrinthProject,
  type ModrinthVersion,
  type SearchIndex,
} from './modrinth.ts';
import { versionLabel } from './versions.ts';

interface Props { detection: ServerDetection }
type Provider = 'modrinth' | 'curseforge' | 'ftb' | 'atlauncher';
type Source = 'all' | Provider;
type LoaderName = 'forge' | 'neoforge' | 'fabric' | 'quilt';

interface GenericVersion {
  id: string;
  label: string;
  gameVersion?: string | null;
  loader?: string | null;
  loaderVersion?: string | null;
  java?: number | null;
}

interface Pack {
  id: string;
  title: string;
  description: string;
  downloads: number;
  author: string;
  iconUrl: string | null;
  source: Provider;
  websiteUrl?: string | null;
  loaders?: string[];
  gameVersion?: string | null;
  gallery?: Array<{ url: string; thumbnailUrl?: string }>;
  availableVersions?: GenericVersion[];
  modrinth?: ModrinthProject;
  curseforge?: CurseForgeProject;
}

const LABEL: Record<Provider, string> = {
  modrinth: 'Modrinth', curseforge: 'CurseForge', ftb: 'FTB', atlauncher: 'ATLauncher',
};
const COLOR: Record<Provider, string> = {
  modrinth: 'green', curseforge: 'orange', ftb: 'blue', atlauncher: 'violet',
};
const CF_LOADER: Record<LoaderName, number> = {
  forge: CF_LOADER_FORGE, neoforge: CF_LOADER_NEOFORGE, fabric: CF_LOADER_FABRIC, quilt: CF_LOADER_QUILT,
};
const MC_VERSIONS = [
  '1.21.8','1.21.7','1.21.6','1.21.5','1.21.4','1.21.3','1.21.2','1.21.1','1.21',
  '1.20.6','1.20.5','1.20.4','1.20.2','1.20.1','1.20','1.19.4','1.19.3','1.19.2','1.19.1','1.19',
  '1.18.2','1.18.1','1.18','1.17.1','1.17','1.16.5','1.16.4','1.16.3','1.16.2','1.16.1','1.16',
].map((value) => ({ value, label: `Minecraft ${value}` }));

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function genericVersions(value: unknown): GenericVersion[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => {
    const row = item as Record<string, unknown>;
    return {
      id: String(row.id ?? row.versionNumber ?? ''),
      label: String(row.displayName ?? row.name ?? row.versionNumber ?? row.id ?? ''),
      gameVersion: typeof row.gameVersion === 'string' ? row.gameVersion : null,
      loader: typeof row.loader === 'string' ? row.loader : null,
      loaderVersion: typeof row.loaderVersion === 'string' ? row.loaderVersion : null,
      java: typeof row.java === 'number' ? row.java : null,
    };
  }).filter((version) => version.id);
}

function externalPack(row: Record<string, unknown>, source: 'ftb' | 'atlauncher'): Pack {
  return {
    id: String(row.id ?? row.slug ?? ''),
    title: String(row.name ?? 'Unknown'),
    description: String(row.summary ?? row.description ?? ''),
    downloads: Number(row.downloadCount ?? 0),
    author: String(row.author ?? LABEL[source]),
    iconUrl: typeof row.iconUrl === 'string' ? row.iconUrl : null,
    source,
    websiteUrl: typeof row.websiteUrl === 'string' ? row.websiteUrl : null,
    loaders: Array.isArray(row.loaders) ? row.loaders.map(String) : [],
    gameVersion: typeof row.gameVersions === 'string' ? row.gameVersions : null,
    gallery: Array.isArray(row.gallery)
      ? row.gallery.map((item) => item as { url: string; thumbnailUrl?: string }).filter((item) => !!item.url)
      : [],
    availableVersions: genericVersions(row.availableVersions),
  };
}

function mcFromCf(file: CurseForgeFile | null): string | null {
  return file?.gameVersions?.find((value) => /^\d+\.\d+(?:\.\d+)?$/.test(value)) ?? null;
}

function loaderFromCf(file: CurseForgeFile | null): LoaderName | null {
  const values = file?.gameVersions?.map((value) => value.toLowerCase()) ?? [];
  return (['neoforge', 'forge', 'fabric', 'quilt'] as LoaderName[])
    .find((loader) => values.some((value) => value.includes(loader))) ?? null;
}

function loaderFromModrinth(version: ModrinthVersion | null): LoaderName | null {
  return (['neoforge', 'forge', 'fabric', 'quilt'] as LoaderName[])
    .find((loader) => version?.loaders?.includes(loader)) ?? null;
}

export default function HybridModpacksTab({ detection }: Props) {
  const { addToast } = useToast();
  const { server, state, updateServer } = useServerStore();
  const canInstall = useServerCan('settings.install');
  const canBackup = useServerCan('backups.create');
  const navigate = useNavigate();

  const [source, setSource] = useState<Source>('all');
  const [cfAvailable, setCfAvailable] = useState<boolean | null>(null);
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState('downloads');
  const [gameVersion, setGameVersion] = useState<string | null>(null);
  const [loaders, setLoaders] = useState<LoaderName[]>([]);
  const [packs, setPacks] = useState<Pack[]>([]);
  const [page, setPage] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [warning, setWarning] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(null);

  const [selected, setSelected] = useState<Pack | null>(null);
  const [mrVersions, setMrVersions] = useState<ModrinthVersion[]>([]);
  const [mrVersion, setMrVersion] = useState<ModrinthVersion | null>(null);
  const [cfFiles, setCfFiles] = useState<CurseForgeFile[]>([]);
  const [cfFile, setCfFile] = useState<CurseForgeFile | null>(null);
  const [otherVersions, setOtherVersions] = useState<GenericVersion[]>([]);
  const [otherVersion, setOtherVersion] = useState<GenericVersion | null>(null);
  const [details, setDetails] = useState('');
  const [gallery, setGallery] = useState<Array<{ url: string; thumbnailUrl?: string }>>([]);
  const [detailLoading, setDetailLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [wipeFiles, setWipeFiles] = useState(true);
  const [deleteWorld, setDeleteWorld] = useState(false);
  const [backupFirst, setBackupFirst] = useState(true);
  const [accepted, setAccepted] = useState(false);

  const isRunning = state === 'running' || state === 'starting';

  useEffect(() => { checkCurseForgeStatus(server.uuid).then(setCfAvailable); }, [server.uuid]);

  const searchModrinth = useCallback(async (q: string, p: number) => {
    const result = await searchProjects({
      query: q || undefined,
      projectType: 'modpack',
      loaders: loaders.length ? loaders : undefined,
      gameVersions: gameVersion ? [gameVersion] : undefined,
      index: sort as SearchIndex,
      offset: p * 20,
      limit: 20,
    });
    return {
      data: result.hits.map((pack): Pack => ({
        id: pack.project_id, title: pack.title, description: pack.description, downloads: pack.downloads,
        author: pack.author, iconUrl: pack.icon_url, source: 'modrinth', loaders: pack.categories,
        gameVersion, modrinth: pack,
      })),
      hasMore: (p + 1) * 20 < result.total_hits,
    };
  }, [gameVersion, loaders, sort]);

  const searchCfOnce = useCallback(async (q: string, p: number, loader?: LoaderName) => {
    const result = await searchCurseForge(server.uuid, {
      searchFilter: q || undefined,
      classId: CF_CLASS_MODPACKS,
      gameVersion: gameVersion ?? undefined,
      modLoaderType: loader ? CF_LOADER[loader] : undefined,
      sortField: sort === 'newest' ? 3 : sort === 'relevance' ? 1 : 6,
      sortOrder: 'desc', index: p * 20, pageSize: 20,
    });
    return {
      data: result.data.map((pack): Pack => ({
        id: String(pack.id), title: pack.name, description: pack.summary, downloads: pack.downloadCount,
        author: pack.authors[0]?.name ?? 'Unknown', iconUrl: pack.logo?.thumbnailUrl ?? null,
        source: 'curseforge', gameVersion, curseforge: pack,
      })),
      hasMore: (p + 1) * 20 < result.pagination.totalCount,
    };
  }, [gameVersion, server.uuid, sort]);

  const searchCurseForgePacks = useCallback(async (q: string, p: number) => {
    if (!cfAvailable) return { data: [] as Pack[], hasMore: false };
    if (loaders.length <= 1) return searchCfOnce(q, p, loaders[0]);
    const settled = await Promise.all(loaders.map((loader) => searchCfOnce(q, p, loader)));
    const unique = new Map<string, Pack>();
    settled.flatMap((result) => result.data).forEach((pack) => unique.set(pack.id, pack));
    return { data: [...unique.values()], hasMore: settled.some((result) => result.hasMore) };
  }, [cfAvailable, loaders, searchCfOnce]);

  const searchExternal = useCallback(async (provider: 'ftb' | 'atlauncher', q: string, p: number) => {
    const params = new URLSearchParams({ query: q, page: String(p), page_size: '20' });
    if (gameVersion) params.set('game_version', gameVersion);
    if (provider === 'ftb' && loaders.length) params.set('loaders', loaders.join(','));
    const response = await fetch(`/api/client/servers/${server.uuid}/content-installer/${provider}/search?${params}`);
    if (!response.ok) throw new Error(await response.text() || `${LABEL[provider]} search failed`);
    const result = await response.json() as { data?: Record<string, unknown>[]; hasMore?: boolean };
    return { data: (result.data ?? []).map((row) => externalPack(row, provider)), hasMore: !!result.hasMore };
  }, [gameVersion, loaders, server.uuid]);

  const searchProvider = useCallback(async (provider: Provider, q: string, p: number) => {
    if (provider === 'modrinth') return searchModrinth(q, p);
    if (provider === 'curseforge') return searchCurseForgePacks(q, p);
    return searchExternal(provider, q, p);
  }, [searchCurseForgePacks, searchExternal, searchModrinth]);

  const runSearch = useCallback(async (q: string, p: number) => {
    setLoading(true);
    setWarning(null);
    try {
      if (source !== 'all') {
        const result = await searchProvider(source, q, p);
        setPacks((old) => p === 0 ? result.data : [...old, ...result.data]);
        setHasMore(result.hasMore);
        setPage(p);
        return;
      }
      const providers: Provider[] = ['modrinth', ...(cfAvailable ? ['curseforge' as const] : []), 'ftb', 'atlauncher'];
      const settled = await Promise.allSettled(providers.map((provider) => searchProvider(provider, q, p)));
      const data = settled.flatMap((result) => result.status === 'fulfilled' ? result.value.data : []);
      const failed = settled.filter((result) => result.status === 'rejected').length;
      if (sort === 'downloads') data.sort((a, b) => b.downloads - a.downloads);
      setPacks((old) => p === 0 ? data : [...old, ...data]);
      setHasMore(settled.some((result) => result.status === 'fulfilled' && result.value.hasMore));
      setPage(p);
      if (failed) setWarning(`${failed} source${failed === 1 ? '' : 's'} failed; results from the remaining providers are still shown.`);
    } catch (error) {
      addToast(`Search failed: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setLoading(false);
    }
  }, [addToast, cfAvailable, searchProvider, sort, source]);

  useEffect(() => {
    setPacks([]);
    setPage(0);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => runSearch(query, 0), 300);
    return () => { if (timer.current) clearTimeout(timer.current); };
  }, [gameVersion, loaders, query, runSearch, sort, source]);

  const openPack = useCallback(async (pack: Pack) => {
    setSelected(pack);
    setDetails(pack.description);
    setGallery(pack.gallery ?? []);
    setDetailLoading(true);
    setWipeFiles(true);
    setDeleteWorld(false);
    setBackupFirst(canBackup);
    setAccepted(false);
    setMrVersions([]); setMrVersion(null); setCfFiles([]); setCfFile(null); setOtherVersions([]); setOtherVersion(null);
    try {
      if (pack.source === 'modrinth' && pack.modrinth) {
        const [project, versions] = await Promise.all([
          getProject(pack.modrinth.project_id),
          getProjectVersions(pack.modrinth.project_id, {
            loaders: loaders.length ? loaders : undefined,
            gameVersions: gameVersion ? [gameVersion] : undefined,
          }),
        ]);
        setDetails(project.body || pack.description);
        setGallery(project.gallery.map((image) => ({ url: image.url, thumbnailUrl: image.url })));
        setMrVersions(versions);
        setMrVersion(versions.find((version) => version.featured) ?? versions[0] ?? null);
      } else if (pack.source === 'curseforge' && pack.curseforge) {
        const [description, files] = await Promise.all([
          getCurseForgeDescription(server.uuid, pack.curseforge.id),
          getCurseForgeFiles(server.uuid, {
            modId: pack.curseforge.id,
            gameVersion: gameVersion ?? undefined,
            modLoaderType: loaders.length === 1 ? CF_LOADER[loaders[0]] : undefined,
            pageSize: 50,
          }),
        ]);
        setDetails(description || pack.description);
        setCfFiles(files.data);
        setCfFile(files.data.find((file) => file.isServerPack) ?? files.data[0] ?? null);
      } else {
        let versions = pack.availableVersions ?? [];
        if (!versions.length) {
          const params = new URLSearchParams(pack.source === 'ftb' ? { pack_id: pack.id } : { safe_name: pack.id });
          const response = await fetch(`/api/client/servers/${server.uuid}/content-installer/${pack.source}/versions?${params}`);
          if (!response.ok) throw new Error(await response.text() || 'Could not load versions');
          const result = await response.json() as { data?: unknown[] };
          versions = genericVersions(result.data);
        }
        setOtherVersions(versions);
        setOtherVersion(versions.find((version) => !gameVersion || version.gameVersion === gameVersion) ?? versions[0] ?? null);
      }
    } catch (error) {
      addToast(`Could not load pack details: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setDetailLoading(false);
    }
  }, [addToast, canBackup, gameVersion, loaders, server.uuid]);

  const selectedLoader = useMemo<LoaderName | null>(() => {
    if (selected?.source === 'modrinth') return loaderFromModrinth(mrVersion);
    if (selected?.source === 'curseforge') return loaderFromCf(cfFile);
    const value = otherVersion?.loader?.toLowerCase();
    return (['forge','neoforge','fabric','quilt'] as LoaderName[]).find((loader) => loader === value) ?? null;
  }, [cfFile, mrVersion, otherVersion?.loader, selected?.source]);

  const selectedMc = useMemo(() => {
    if (selected?.source === 'modrinth') return mrVersion?.game_versions?.find((value) => /^\d+\.\d+/.test(value)) ?? null;
    if (selected?.source === 'curseforge') return mcFromCf(cfFile);
    return otherVersion?.gameVersion ?? null;
  }, [cfFile, mrVersion, otherVersion?.gameVersion, selected?.source]);

  const versionOptions = useMemo(() => {
    if (selected?.source === 'modrinth') return mrVersions.map((version) => ({ value: version.id, label: versionLabel(version.version_number, version.game_versions) }));
    if (selected?.source === 'curseforge') return cfFiles.map((file) => ({ value: String(file.id), label: versionLabel(file.displayName, file.gameVersions) }));
    return otherVersions.map((version) => ({ value: version.id, label: version.label }));
  }, [cfFiles, mrVersions, otherVersions, selected?.source]);

  const selectedVersionId = selected?.source === 'modrinth' ? mrVersion?.id ?? null
    : selected?.source === 'curseforge' ? (cfFile ? String(cfFile.id) : null)
      : otherVersion?.id ?? null;

  const createSafetyBackup = useCallback(async () => {
    const name = `Before modpack install — ${selected?.title ?? 'modpack'} (${new Date().toLocaleString()})`;
    addToast('Creating safety backup before the install...', 'info');
    const backup = await createBackup(server.uuid, { name, ignoredFiles: [] });
    for (let attempt = 0; attempt < 150; attempt += 1) {
      const result = await getBackups(server.uuid, 1);
      const current = result.data.find((item) => item.uuid === backup.uuid);
      if (current?.completed) {
        if (!current.isSuccessful) throw new Error('Safety backup failed; install was cancelled.');
        addToast('Safety backup completed.', 'success');
        return;
      }
      await sleep(2000);
    }
    throw new Error('Safety backup did not finish within five minutes; install was cancelled.');
  }, [addToast, selected?.title, server.uuid]);

  const install = useCallback(async () => {
    if (!selected) return;
    setInstalling(true);
    try {
      if (backupFirst) await createSafetyBackup();

      let endpoint = '';
      let params = new URLSearchParams({
        wipe_files: String(wipeFiles), delete_world: String(deleteWorld), modpack_name: selected.title,
      });
      if (selected.source === 'modrinth') {
        const file = mrVersion ? getPrimaryFile(mrVersion) : null;
        if (!mrVersion || !file) throw new Error('No installable Modrinth version selected.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/install`;
        params.set('mrpack_url', file.url);
        params.set('version_name', mrVersion.version_number);
        if (selectedLoader) params.set('loader', selectedLoader);
        if (selectedMc) params.set('minecraft', selectedMc);
      } else if (selected.source === 'curseforge') {
        if (!cfFile?.downloadUrl) throw new Error('This CurseForge version cannot be downloaded by third-party installers.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/cf-install`;
        params.set('zip_url', cfFile.downloadUrl);
        params.set('version_name', cfFile.displayName);
        if (selectedLoader) params.set('loader', selectedLoader);
        if (selectedMc) params.set('minecraft', selectedMc);
      } else if (selected.source === 'ftb') {
        if (!otherVersion) throw new Error('Select an FTB version.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/ftb-install`;
        params.set('pack_id', selected.id);
        params.set('version_id', otherVersion.id);
        params.set('version_name', otherVersion.label);
      } else {
        if (!otherVersion) throw new Error('Select an ATLauncher version.');
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/atlauncher-install`;
        params.set('safe_name', selected.id);
        params.set('version', otherVersion.id);
      }

      const response = await fetch(`${endpoint}?${params}`, { method: 'POST' });
      if (!response.ok) throw new Error(await response.text() || `Install failed (${response.status})`);
      addToast(`Installing “${selected.title}”. Opening the console for live logs.`, 'success');
      setSelected(null);
      updateServer({ status: 'installing' });
      navigate(`/server/${server.uuidShort}`);
    } catch (error) {
      addToast(`Modpack install failed: ${error instanceof Error ? error.message : 'unknown'}`, 'error');
    } finally {
      setInstalling(false);
    }
  }, [addToast, backupFirst, cfFile, createSafetyBackup, deleteWorld, mrVersion, navigate, otherVersion, selected, selectedLoader, selectedMc, server.uuid, server.uuidShort, updateServer, wipeFiles]);

  const toggleLoader = (loader: LoaderName, checked: boolean) => {
    setLoaders((old) => checked ? [...new Set([...old, loader])] : old.filter((value) => value !== loader));
  };

  return (
    <div className='ci-browse ci-modpack-manager'>
      <div className='ci-search-bar'>
        <TextInput placeholder='Search modpacks...' leftSection={<FontAwesomeIcon icon={faSearch} />} value={query} onChange={(event) => setQuery(event.currentTarget.value)} className='ci-search-input' />
        <Select searchable clearable placeholder='Minecraft version' data={MC_VERSIONS} value={gameVersion} onChange={setGameVersion} w={210} />
        <Select data={[{value:'relevance',label:'Relevance'},{value:'downloads',label:'Downloads'},{value:'newest',label:'Newest'},{value:'updated',label:'Updated'}]} value={sort} onChange={(value) => value && setSort(value)} w={140} />
      </div>

      <div className='ci-provider-row'>
        <SegmentedControl value={source} onChange={(value) => setSource(value as Source)} data={[
          { value: 'all', label: 'All Sources' }, { value: 'modrinth', label: 'Modrinth' },
          ...(cfAvailable ? [{ value: 'curseforge', label: 'CurseForge' }] : []),
          { value: 'ftb', label: 'FTB' }, { value: 'atlauncher', label: 'ATLauncher' },
        ]} />
        <Group gap='sm' wrap='wrap'>
          {(['forge','neoforge','fabric','quilt'] as LoaderName[]).map((loader) => (
            <Checkbox key={loader} label={loader === 'neoforge' ? 'NeoForge' : loader[0].toUpperCase() + loader.slice(1)} checked={loaders.includes(loader)} onChange={(event) => toggleLoader(loader, event.currentTarget.checked)} />
          ))}
        </Group>
      </div>

      {(gameVersion || loaders.length > 0) && <Group gap='xs' mt='sm'>
        {gameVersion && <Badge variant='light'>MC {gameVersion}</Badge>}
        {loaders.map((loader) => <Badge key={loader} variant='light'>{loader}</Badge>)}
        <Button size='compact-xs' variant='subtle' onClick={() => { setGameVersion(null); setLoaders([]); }}>Clear filters</Button>
      </Group>}
      {warning && <Alert color='yellow' variant='light' mt='sm'>{warning}</Alert>}

      {loading && packs.length === 0 ? <div className='ci-center'><Loader color='violet' size='lg' /></div> : packs.length === 0 ? (
        <Text c='dimmed' ta='center' mt='xl'>No modpacks matched your filters.</Text>
      ) : <>
        <div className='ci-results-grid ci-modpack-grid'>
          {packs.map((pack) => <Card key={`${pack.source}-${pack.id}`} hoverable p='md' className='ci-project-card ci-modpack-card' onClick={() => openPack(pack)}>
            <div className='ci-card-header'>
              {pack.iconUrl ? <img src={pack.iconUrl} alt='' className='ci-project-icon' /> : <div className='ci-project-icon ci-project-icon--placeholder' />}
              <div className='ci-card-title'><Text fw={700} size='sm' lineClamp={1}>{pack.title}</Text><Text size='xs' c='dimmed'>by {pack.author}</Text></div>
              <Badge variant='light' color={COLOR[pack.source]} size='xs'>{LABEL[pack.source]}</Badge>
            </div>
            <div className='ci-card-body'><Text size='xs' c='dimmed' lineClamp={3}>{pack.description}</Text></div>
            <div className='ci-card-footer'><Text size='xs' c='dimmed'>{pack.downloads ? `${formatDownloads(pack.downloads)} downloads` : LABEL[pack.source]}</Text></div>
          </Card>)}
        </div>
        {hasMore && <Group justify='center' mt='md'><Button variant='subtle' loading={loading} onClick={() => runSearch(query, page + 1)}>Load More</Button></Group>}
      </>}

      <Modal opened={!!selected} onClose={() => { if (!installing) setSelected(null); }} title={null} size='80%' padding='lg' classNames={{ header: 'ci-modal-header', body: 'ci-modal-body' }} closeOnClickOutside={!installing} closeOnEscape={!installing}>
        {selected && <Stack gap='md'>
          <div className='ci-detail-top'>
            <div className='ci-detail-top-left'>
              {selected.iconUrl ? <img src={selected.iconUrl} alt='' className='ci-detail-icon' /> : <div className='ci-detail-icon ci-detail-icon--placeholder' />}
              <div className='ci-detail-meta'>
                <Group gap='xs'><Text fw={700} size='lg'>{selected.title}</Text><Badge color={COLOR[selected.source]} variant='light'>{LABEL[selected.source]}</Badge></Group>
                <Group gap='xs'><Text size='xs' c='dimmed'>by {selected.author}</Text>{selected.source === 'modrinth' && mrVersion && <Text size='xs' c='dimmed'>· {timeAgo(mrVersion.date_published)}</Text>}</Group>
              </div>
              {(selected.websiteUrl || selected.modrinth || selected.curseforge) && <Button size='compact-xs' variant='subtle' leftSection={<FontAwesomeIcon icon={faExternalLink} />} onClick={() => {
                const url = selected.websiteUrl ?? (selected.modrinth ? `https://modrinth.com/modpack/${selected.modrinth.slug}` : selected.curseforge ? `https://www.curseforge.com/minecraft/modpacks/${selected.curseforge.slug}` : null);
                if (url) window.open(url, '_blank', 'noopener');
              }}>Open</Button>}
            </div>
            <div className='ci-detail-top-right'>
              {detailLoading ? <Loader size='xs' /> : <Select searchable placeholder='Version...' data={versionOptions} value={selectedVersionId} onChange={(value) => {
                if (selected.source === 'modrinth') setMrVersion(mrVersions.find((version) => version.id === value) ?? null);
                else if (selected.source === 'curseforge') setCfFile(cfFiles.find((file) => String(file.id) === value) ?? null);
                else setOtherVersion(otherVersions.find((version) => version.id === value) ?? null);
              }} w='min(440px, 100%)' />}
            </div>
          </div>

          {isRunning && <Alert icon={<FontAwesomeIcon icon={faExclamationTriangle} />} color='red'>Stop the server before installing a modpack.</Alert>}
          {!canInstall && <Alert color='yellow'>You need the server reinstall permission.</Alert>}

          {detailLoading ? <div className='ci-center'><Loader size='sm' /></div> : details ? <div className='ci-detail-body' dangerouslySetInnerHTML={{ __html: selected.source === 'curseforge' ? details : marked.parse(details, { async: false, gfm: true }) as string }} /> : null}
          {gallery.length > 0 && <div className='ci-gallery-strip'>{gallery.slice(0, 10).map((image, index) => <a key={`${image.url}-${index}`} href={image.url} target='_blank' rel='noreferrer'><img className='ci-gallery-thumb' src={image.thumbnailUrl ?? image.url} alt='' /></a>)}</div>}

          <Card p='md' className='ci-install-plan'><Stack gap='xs'><Text fw={700}>Install plan</Text><Group gap='xs'>
            <Badge variant='light'>{LABEL[selected.source]}</Badge>
            {selectedMc && <Badge variant='light'>Minecraft {selectedMc}</Badge>}
            {selectedLoader && <Badge variant='light'>Loader: {selectedLoader}</Badge>}
            {otherVersion?.loaderVersion && <Badge variant='light'>Loader {otherVersion.loaderVersion}</Badge>}
            {otherVersion?.java && <Badge variant='light'>Java {otherVersion.java}</Badge>}
          </Group></Stack></Card>

          <Checkbox label='Create a safety backup first' description={canBackup ? 'Waits for a successful Calagopus backup before changing files.' : 'You do not have backups.create permission.'} checked={backupFirst} onChange={(event) => setBackupFirst(event.currentTarget.checked)} disabled={!canBackup || installing || isRunning} />
          <Checkbox label='Wipe old server / modpack files' description='Recommended when switching packs. Detected worlds, server.properties, whitelist, bans and ops are preserved.' checked={wipeFiles} onChange={(event) => setWipeFiles(event.currentTarget.checked)} disabled={installing || isRunning} />
          <Checkbox label='Delete existing world' description={detection.worldDirs.length ? `Detected: ${detection.worldDirs.join(', ')}` : 'No world directories containing level.dat were detected.'} checked={deleteWorld} onChange={(event) => setDeleteWorld(event.currentTarget.checked)} disabled={installing || isRunning || detection.worldDirs.length === 0} />
          {deleteWorld && <Alert icon={<FontAwesomeIcon icon={faExclamationTriangle} />} color='red'>The detected world will be deleted after the optional backup succeeds.</Alert>}
          <Group justify='space-between' align='center' wrap='wrap'>
            <Checkbox label={deleteWorld ? 'I understand this will replace server files and delete the world' : 'I understand this will replace server files'} checked={accepted} onChange={(event) => setAccepted(event.currentTarget.checked)} disabled={installing || isRunning} />
            <Button color='red' leftSection={<FontAwesomeIcon icon={faArrowDown} />} loading={installing} disabled={!canInstall || isRunning || !selectedVersionId || !accepted} onClick={install}>Install Modpack</Button>
          </Group>
        </Stack>}
      </Modal>
    </div>
  );
}
