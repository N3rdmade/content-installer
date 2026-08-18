import { marked } from 'marked';
import { faArrowDown, faExclamationTriangle, faExternalLink, faSearch } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Loader } from '@mantine/core';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import type { ServerDetection } from './detect.ts';
import { versionLabel } from './versions.ts';
import {
  CF_CLASS_MODPACKS,
  checkCurseForgeStatus,
  formatDownloads as cfFormatDownloads,
  formatSize as cfFormatSize,
  getCurseForgeDescription,
  getCurseForgeFiles,
  searchCurseForge,
  type CurseForgeFile,
  type CurseForgeProject,
} from './curseforge.ts';
import {
  formatDownloads,
  formatSize,
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

type Source = 'modrinth' | 'curseforge';

interface DisplayModpack {
  id: string;
  title: string;
  description: string;
  downloads: number;
  author: string;
  iconUrl: string | null;
  source: Source;
  modrinthProject?: ModrinthProject;
  curseforgeProject?: CurseForgeProject;
}

// Content comes from Modrinth/CurseForge project descriptions (trusted API sources)

export default function ModpacksTab({ detection }: ModpacksTabProps) {
  const { addToast } = useToast();
  const { server, state } = useServerStore();

  const [source, setSource] = useState<Source>('modrinth');
  const [cfAvailable, setCfAvailable] = useState<boolean | null>(null);

  // Search state
  const [query, setQuery] = useState('');
  const [sortBy, setSortBy] = useState<string>('downloads');
  const [results, setResults] = useState<DisplayModpack[]>([]);
  const [totalHits, setTotalHits] = useState(0);
  const [loading, setLoading] = useState(false);
  const searchTimer = useRef<ReturnType<typeof setTimeout>>(null);

  // Install modal
  const [selectedModpack, setSelectedModpack] = useState<DisplayModpack | null>(null);
  // Modrinth versions
  const [modrinthVersions, setModrinthVersions] = useState<ModrinthVersion[]>([]);
  const [selectedModrinthVersion, setSelectedModrinthVersion] = useState<ModrinthVersion | null>(null);
  // CurseForge files
  const [cfFiles, setCfFiles] = useState<CurseForgeFile[]>([]);
  const [selectedCfFile, setSelectedCfFile] = useState<CurseForgeFile | null>(null);

  // Detail
  const [detailBody, setDetailBody] = useState<string>('');
  const [detailLoading, setDetailLoading] = useState(false);

  const [versionsLoading, setVersionsLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [cleanInstall, setCleanInstall] = useState(true);
  const [acceptRisk, setAcceptRisk] = useState(false);

  const isRunning = state === 'running' || state === 'starting';

  useEffect(() => {
    checkCurseForgeStatus(server.uuid).then(setCfAvailable);
  }, [server.uuid]);

  // Modrinth search
  const doModrinthSearch = useCallback(async (q: string, sort: string, offset: number) => {
    const res = await searchProjects({
      query: q || undefined,
      projectType: 'modpack',
      index: sort as SearchIndex,
      offset,
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
      total: res.total_hits,
    };
  }, []);

  // CurseForge search
  const doCurseForgeSearch = useCallback(async (q: string, sort: string, offset: number) => {
    const sortMap: Record<string, number> = {
      relevance: 1, downloads: 6, follows: 2, newest: 11, updated: 3,
    };
    const res = await searchCurseForge(server.uuid, {
      searchFilter: q || undefined,
      classId: CF_CLASS_MODPACKS,
      sortField: sortMap[sort] ?? 6,
      sortOrder: 'desc',
      index: offset,
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
      total: res.pagination.totalCount,
    };
  }, [server.uuid]);

  const doSearch = useCallback(async (q: string, sort: string, offset: number) => {
    setLoading(true);
    try {
      const result = source === 'curseforge'
        ? await doCurseForgeSearch(q, sort, offset)
        : await doModrinthSearch(q, sort, offset);
      if (offset === 0) {
        setResults(result.items);
      } else {
        setResults((prev) => [...prev, ...result.items]);
      }
      setTotalHits(result.total);
    } catch (err) {
      addToast(`Search failed: ${err instanceof Error ? err.message : 'unknown'}`, 'error');
    } finally {
      setLoading(false);
    }
  }, [source, doModrinthSearch, doCurseForgeSearch]);

  useEffect(() => {
    setResults([]);
    setTotalHits(0);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => doSearch(query, sortBy, 0), 300);
    return () => { if (searchTimer.current) clearTimeout(searchTimer.current); };
  }, [query, sortBy, doSearch, source]);

  const loadMore = () => doSearch(query, sortBy, results.length);

  // Open install modal
  const openInstall = useCallback(async (modpack: DisplayModpack) => {
    setSelectedModpack(modpack);
    setVersionsLoading(true);
    setDetailLoading(true);
    setDetailBody('');
    setCleanInstall(true);
    setAcceptRisk(false);
    setModrinthVersions([]);
    setSelectedModrinthVersion(null);
    setCfFiles([]);
    setSelectedCfFile(null);

    try {
      if (modpack.source === 'modrinth' && modpack.modrinthProject) {
        const [details, vers] = await Promise.all([
          getProject(modpack.modrinthProject.project_id),
          getProjectVersions(modpack.modrinthProject.project_id),
        ]);
        setDetailBody(details.body ?? '');
        setModrinthVersions(vers);
        const featured = vers.find((v) => v.featured) ?? vers[0];
        if (featured) setSelectedModrinthVersion(featured);
      } else if (modpack.source === 'curseforge' && modpack.curseforgeProject) {
        const [desc, res] = await Promise.all([
          getCurseForgeDescription(server.uuid, modpack.curseforgeProject.id),
          getCurseForgeFiles(server.uuid, {
            modId: modpack.curseforgeProject.id,
            pageSize: 50,
          }),
        ]);
        setDetailBody(desc);
        setCfFiles(res.data);
        if (res.data.length > 0) setSelectedCfFile(res.data[0]);
      }
    } catch (err) {
      addToast(`Failed to load versions: ${err instanceof Error ? err.message : 'unknown'}`, 'error');
    } finally {
      setVersionsLoading(false);
      setDetailLoading(false);
    }
  }, [server.uuid]);

  // Loader info from Modrinth version
  const loaderInfo = useMemo(() => {
    if (!selectedModrinthVersion) return null;
    const loaders = selectedModrinthVersion.loaders ?? [];
    if (loaders.includes('fabric')) return { name: 'Fabric' };
    if (loaders.includes('neoforge')) return { name: 'NeoForge' };
    if (loaders.includes('forge')) return { name: 'Forge' };
    if (loaders.includes('quilt')) return { name: 'Quilt' };
    return null;
  }, [selectedModrinthVersion]);

  // Version options
  const versionOptions = useMemo(() => {
    if (selectedModpack?.source === 'modrinth') {
      return modrinthVersions.map((v) => ({
        value: v.id,
        label: versionLabel(v.version_number, v.game_versions),
      }));
    }
    return cfFiles.map((f) => ({
      value: String(f.id),
      label: versionLabel(f.displayName, f.gameVersions),
    }));
  }, [selectedModpack?.source, modrinthVersions, cfFiles]);

  // Size the version select to the SELECTED label so the chosen value never
  // truncates in the closed input (#18) without reserving space for the
  // longest option — that padded the row with dead space and squeezed the
  // install button. The open dropdown sizes itself (width: max-content).
  // ~7.1px/char at size sm + 60px chrome.
  const versionSelectWidth = useMemo(() => {
    const selectedId =
      selectedModpack?.source === 'modrinth'
        ? (selectedModrinthVersion?.id ?? null)
        : selectedCfFile
          ? String(selectedCfFile.id)
          : null;
    const current = versionOptions.find((o) => o.value === selectedId)?.label ?? '';
    return Math.min(Math.max(200, Math.round(current.length * 7.1) + 60), 440);
  }, [versionOptions, selectedModpack?.source, selectedModrinthVersion, selectedCfFile]);

  const hasVersions = selectedModpack?.source === 'modrinth' ? modrinthVersions.length > 0 : cfFiles.length > 0;

  // Install
  const doInstall = useCallback(async () => {
    if (!selectedModpack) return;

    setInstalling(true);

    try {
      let endpoint: string;
      let params: URLSearchParams;

      if (selectedModpack.source === 'modrinth') {
        if (!selectedModrinthVersion) return;
        const file = getPrimaryFile(selectedModrinthVersion);
        if (!file) { addToast('No .mrpack file found.', 'error'); return; }
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/install`;
        params = new URLSearchParams({
          mrpack_url: file.url,
          clean_install: String(cleanInstall),
        });
      } else {
        if (!selectedCfFile) return;
        if (!selectedCfFile.downloadUrl) {
          addToast('This modpack does not allow third-party downloads.', 'error');
          return;
        }
        endpoint = `/api/client/servers/${server.uuid}/content-installer/modpack/cf-install`;
        params = new URLSearchParams({
          zip_url: selectedCfFile.downloadUrl,
          clean_install: String(cleanInstall),
        });
      }

      const res = await fetch(`${endpoint}?${params}`, { method: 'POST' });
      if (!res.ok) throw new Error(await res.text() || `Install failed: ${res.status}`);
      addToast(`Modpack "${selectedModpack.title}" install started`, 'success');
      setSelectedModpack(null);
    } catch (err) {
      addToast(`Modpack install failed: ${err instanceof Error ? err.message : 'unknown'}`, 'error');
    } finally {
      setInstalling(false);
    }
  }, [selectedModpack, selectedModrinthVersion, selectedCfFile, cleanInstall, server.uuid]);

  const selectedFile = selectedModpack?.source === 'modrinth' && selectedModrinthVersion
    ? getPrimaryFile(selectedModrinthVersion) : null;

  const sourceOptions = [
    { value: 'modrinth', label: 'Modrinth' },
    ...(cfAvailable ? [{ value: 'curseforge', label: 'CurseForge' }] : []),
  ];

  const canInstall = selectedModpack?.source === 'modrinth'
    ? !!selectedModrinthVersion && !!selectedFile
    : !!selectedCfFile && !!selectedCfFile?.downloadUrl;

  return (
    <div className='ci-browse'>
      {/* Search bar */}
      <div className='ci-search-bar'>
        <TextInput
          placeholder='Search modpacks...'
          leftSection={<FontAwesomeIcon icon={faSearch} />}
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          className='ci-search-input'
        />
        {sourceOptions.length > 1 && (
          <SegmentedControl
            value={source}
            onChange={(v) => setSource(v as Source)}
            data={sourceOptions}
          />
        )}
        <Select
          data={[
            { value: 'relevance', label: 'Relevance' },
            { value: 'downloads', label: 'Downloads' },
            { value: 'follows', label: 'Follows' },
            { value: 'newest', label: 'Newest' },
            { value: 'updated', label: 'Updated' },
          ]}
          value={sortBy}
          onChange={(v) => v && setSortBy(v)}
          w={140}
        />
      </div>

      {/* Results */}
      {loading && results.length === 0 ? (
        <div className='ci-center'><Loader color='violet' size='lg' /></div>
      ) : results.length === 0 ? (
        <Text c='dimmed' ta='center' mt='xl'>
          {query ? 'No modpacks found. Try a different search.' : 'No modpacks found.'}
        </Text>
      ) : (
        <>
          <div className='ci-results-grid'>
            {results.map((modpack) => (
              <Card
                key={`${modpack.source}-${modpack.id}`}
                hoverable
                p='md'
                className='ci-project-card'
                onClick={() => openInstall(modpack)}
              >
                <div className='ci-card-header'>
                  {modpack.iconUrl ? (
                    <img src={modpack.iconUrl} alt='' className='ci-project-icon' />
                  ) : (
                    <div className='ci-project-icon ci-project-icon--placeholder' />
                  )}
                  <div className='ci-card-title'>
                    <Text fw={600} size='sm' lineClamp={1}>{modpack.title}</Text>
                    <Text size='xs' c='dimmed'>by {modpack.author}</Text>
                  </div>
                </div>
                <div className='ci-card-body'>
                  <Text size='xs' c='dimmed' lineClamp={3}>{modpack.description}</Text>
                </div>
                <div className='ci-card-footer'>
                  <Text size='xs' c='dimmed'>
                    {(modpack.source === 'curseforge' ? cfFormatDownloads : formatDownloads)(modpack.downloads)} downloads
                  </Text>
                  <Badge variant='light' color={modpack.source === 'curseforge' ? 'orange' : 'green'} size='xs'>
                    {modpack.source === 'curseforge' ? 'CurseForge' : 'Modrinth'}
                  </Badge>
                </div>
              </Card>
            ))}
          </div>

          {results.length < totalHits && (
            <Group justify='center' mt='md'>
              <Button variant='subtle' onClick={loadMore} loading={loading}>
                Load More ({results.length}/{totalHits})
              </Button>
            </Group>
          )}
        </>
      )}

      {/* Install Modal */}
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
            {/* Header row: icon + meta left, version + install right */}
            <div className='ci-detail-top'>
              <div className='ci-detail-top-left'>
                {selectedModpack.iconUrl ? (
                  <img src={selectedModpack.iconUrl} alt='' className='ci-detail-icon' />
                ) : (
                  <div className='ci-detail-icon ci-detail-icon--placeholder' />
                )}
                <div className='ci-detail-meta'>
                  <Group gap='xs' align='center'>
                    <Text fw={700} size='lg'>{selectedModpack.title}</Text>
                    <Badge variant='light' color={selectedModpack.source === 'curseforge' ? 'orange' : 'green'} size='xs'>
                      {selectedModpack.source === 'curseforge' ? 'CurseForge' : 'Modrinth'}
                    </Badge>
                    {selectedModpack.source === 'modrinth' && selectedModpack.modrinthProject && (
                      <Button size='compact-xs' variant='subtle'
                        leftSection={<FontAwesomeIcon icon={faExternalLink} />}
                        onClick={(e: React.MouseEvent) => {
                          e.stopPropagation();
                          window.open(`https://modrinth.com/modpack/${selectedModpack.modrinthProject!.slug}`, '_blank', 'noopener');
                        }}>View</Button>
                    )}
                    {selectedModpack.source === 'curseforge' && selectedModpack.curseforgeProject && (
                      <Button size='compact-xs' variant='subtle'
                        leftSection={<FontAwesomeIcon icon={faExternalLink} />}
                        onClick={(e: React.MouseEvent) => {
                          e.stopPropagation();
                          window.open(`https://www.curseforge.com/minecraft/modpacks/${selectedModpack.curseforgeProject!.slug}`, '_blank', 'noopener');
                        }}>View</Button>
                    )}
                  </Group>
                  <Group gap='xs'>
                    <Text size='xs' c='dimmed'>by {selectedModpack.author}</Text>
                    <Text size='xs' c='dimmed'>&middot;</Text>
                    <Text size='xs' c='dimmed'>
                      {(selectedModpack.source === 'curseforge' ? cfFormatDownloads : formatDownloads)(selectedModpack.downloads)} downloads
                    </Text>
                    {selectedModpack.source === 'modrinth' && selectedModrinthVersion && (
                      <>
                        <Text size='xs' c='dimmed'>&middot;</Text>
                        <Text size='xs' c='dimmed'>{timeAgo(selectedModrinthVersion.date_published)}</Text>
                      </>
                    )}
                    {loaderInfo && <Badge variant='light' color='violet' size='xs'>{loaderInfo.name}</Badge>}
                  </Group>
                </div>
              </div>

              <div className='ci-detail-top-right'>
                {versionsLoading ? (
                  <Loader color='violet' size='xs' />
                ) : !hasVersions ? (
                  <Text size='xs' c='dimmed'>No versions</Text>
                ) : (
                  <Select
                    placeholder='Version...'
                    data={versionOptions}
                    value={
                      selectedModpack.source === 'modrinth'
                        ? (selectedModrinthVersion?.id ?? null)
                        : (selectedCfFile ? String(selectedCfFile.id) : null)
                    }
                    onChange={(val) => {
                      if (selectedModpack.source === 'modrinth') {
                        setSelectedModrinthVersion(modrinthVersions.find((v) => v.id === val) ?? null);
                      } else {
                        setSelectedCfFile(cfFiles.find((f) => String(f.id) === val) ?? null);
                      }
                    }}
                    searchable
                    size='sm'
                    w={`min(${versionSelectWidth}px, 100%)`}
                    comboboxProps={{ width: 'max-content', position: 'bottom-end' }}
                    disabled={installing}
                  />
                )}
              </div>
            </div>

            {isRunning && (
              <Alert icon={<FontAwesomeIcon icon={faExclamationTriangle} />} color='red' variant='light'>
                Stop your server before installing a modpack.
              </Alert>
            )}

            {selectedModpack.source === 'curseforge' && selectedCfFile && !selectedCfFile.downloadUrl && (
              <Alert color='red' variant='light'>
                This modpack does not allow third-party downloads.
              </Alert>
            )}

            {/* Description — trusted API content from Modrinth/CurseForge */}
            {detailLoading ? (
              <div className='ci-center'><Loader color='violet' size='sm' /></div>
            ) : detailBody ? (
              <div
                className='ci-detail-body'
                dangerouslySetInnerHTML={{
                  __html: selectedModpack.source === 'curseforge'
                    ? detailBody
                    : (marked.parse(detailBody, { async: false, breaks: false, gfm: true }) as string),
                }}
              />
            ) : (
              <Text size='sm' c='dimmed'>{selectedModpack.description}</Text>
            )}

            {/* Bottom bar: checkboxes + install */}
            {hasVersions && !versionsLoading && (
              <>
                <Checkbox
                  label='Clean install (recommended)'
                  description='Wipes all existing server files before installing.'
                  checked={cleanInstall}
                  onChange={(e) => setCleanInstall(e.currentTarget.checked)}
                  color='red'
                  disabled={installing || isRunning}
                />
                <Group justify='space-between' align='center' wrap='wrap'>
                  <Checkbox
                    label='I understand this will replace my server files'
                    checked={acceptRisk}
                    onChange={(e) => setAcceptRisk(e.currentTarget.checked)}
                    disabled={installing || isRunning}
                  />
                  <Group gap='sm'>
                    <Button
                      onClick={doInstall}
                      loading={installing}
                      disabled={isRunning || !canInstall || !acceptRisk || !hasVersions}
                      color='red'
                      leftSection={<FontAwesomeIcon icon={faArrowDown} />}
                    >
                      Install Modpack
                    </Button>
                  </Group>
                </Group>
              </>
            )}
          </Stack>
        )}
      </Modal>
    </div>
  );
}
