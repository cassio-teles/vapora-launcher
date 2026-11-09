import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  Gamepad2, Compass, Library, Download, Settings, Plus, Search, Folder,
  Play, Trash2, RefreshCw, ChevronRight, ShieldCheck, Monitor,
  ArrowDownToLine, X, Database, CheckCircle2, AlertCircle, LoaderCircle, Star, LayoutGrid
} from 'lucide-react';

type Game = {
  id: number; title: string; developer: string; description: string;
  install_path: string | null; executable: string | null; cover: string | null;
  source: string; added_at: string; is_catalog: boolean;
};
type Repo = { id: number; name: string; url: string; enabled: boolean; last_sync: string | null };
type DownloadSource = {
  id: number; game_id: number; repository_id: number; repository_name: string;
  name: string; url: string; kind: string; size: string | null; sha256: string | null;
};
type DownloadEvent = {
  source_id: number; game_id: number; game_title: string; file_name: string;
  downloaded: number; total: number | null; status: string; path: string | null; error: string | null;
};
type DownloadItem = DownloadEvent;
type Page = 'store' | 'library' | 'downloads' | 'repositories' | 'settings';

const formatBytes = (bytes: number) => {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024; let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return `${value.toFixed(1)} ${units[unit]}`;
};

export default function App() {
  const [page, setPage] = useState<Page>('store');
  const [games, setGames] = useState<Game[]>([]);
  const [catalog, setCatalog] = useState<Game[]>([]);
  const [repos, setRepos] = useState<Repo[]>([]);
  const [selected, setSelected] = useState<Game | null>(null);
  const [sources, setSources] = useState<DownloadSource[]>([]);
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [query, setQuery] = useState('');
  const [modal, setModal] = useState<'repo' | 'local' | 'sources' | null>(null);
  const [repoName, setRepoName] = useState('');
  const [repoUrl, setRepoUrl] = useState('');
  const [title, setTitle] = useState('');
  const [developer, setDeveloper] = useState('');
  const [folder, setFolder] = useState('');
  const [executable, setExecutable] = useState('');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const [noticeError, setNoticeError] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ status: 'idle' | 'current' | 'available' | 'error'; version?: string; url?: string; message?: string }>({ status: 'idle' });

  const notify = (message: string, error = false) => {
    setNotice(message); setNoticeError(error);
    window.setTimeout(() => setNotice(''), 4500);
  };

  const checkUpdates = async () => {
    setUpdateBusy(true);
    setUpdateInfo({ status: 'idle', message: 'Consultando as versões publicadas...' });
    try {
      const response = await fetch('https://api.github.com/repos/cassio-teles/vapora-launcher/releases/latest', { headers: { Accept: 'application/vnd.github+json' } });
      if (response.status === 404) {
        setUpdateInfo({ status: 'error', message: 'Ainda não há uma versão publicada na página de lançamentos do Vapora.' });
        return;
      }
      if (!response.ok) throw new Error(`GitHub respondeu com HTTP ${response.status}`);
      const release = await response.json() as { tag_name?: string; name?: string; html_url?: string; draft?: boolean; prerelease?: boolean };
      const latest = (release.tag_name || release.name || '').replace(/^v/i, '').trim();
      if (!latest) throw new Error('A versão publicada não informou um número válido.');
      const currentParts = '0.4.0'.split('.').map(Number);
      const latestParts = latest.split('.').map(Number);
      const newer = latestParts.some((part, index) => part > (currentParts[index] || 0) && latestParts.slice(0, index).every((v, i) => v === (currentParts[i] || 0)));
      if (newer && release.html_url) setUpdateInfo({ status: 'available', version: latest, url: release.html_url, message: `A versão ${latest} está disponível.` });
      else setUpdateInfo({ status: 'current', version: latest, url: release.html_url, message: `Você está usando a versão 0.4.0. Última versão publicada: ${latest}.` });
    } catch (error) {
      setUpdateInfo({ status: 'error', message: `Não foi possível verificar atualizações: ${String(error)}` });
    } finally { setUpdateBusy(false); }
  };

  const refresh = async () => {
    try {
      const [localGames, catalogGames, repositories] = await Promise.all([
        invoke<Game[]>('list_games'), invoke<Game[]>('list_catalog_games'), invoke<Repo[]>('list_repositories')
      ]);
      setGames(localGames); setCatalog(catalogGames); setRepos(repositories);
      if (selected) {
        const updated = [...localGames, ...catalogGames].find(g => g.id === selected.id);
        if (updated) setSelected(updated);
      }
    } catch (error) { notify(String(error), true); }
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<DownloadEvent>('download-progress', event => {
      const item = event.payload;
      setDownloads(current => {
        const index = current.findIndex(d => d.source_id === item.source_id && d.status !== 'completed');
        if (index < 0) return [item, ...current];
        const next = [...current]; next[index] = item; return next;
      });
      if (item.status === 'completed') notify(`${item.game_title}: download concluído.`);
      if (item.status === 'failed') notify(item.error || 'Falha no download.', true);
    }).then(fn => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  const filteredCatalog = useMemo(() => catalog.filter(g =>
    `${g.title} ${g.developer}`.toLowerCase().includes(query.toLowerCase())), [catalog, query]);
  const filteredLibrary = useMemo(() => games.filter(g =>
    `${g.title} ${g.developer}`.toLowerCase().includes(query.toLowerCase())), [games, query]);
  const featured = catalog[0];

  const showGame = (game: Game) => { setSelected(game); setPage('store'); };
  const openSources = async (game: Game) => {
    setSelected(game);
    try {
      const options = await invoke<DownloadSource[]>('list_game_sources', { gameId: game.id });
      setSources(options); setModal('sources');
    } catch (error) { notify(String(error), true); }
  };

  const addRepository = async () => {
    if (!repoName.trim() || !repoUrl.trim()) return notify('Preencha o nome e o endereço do repositório.', true);
    try {
      await invoke('add_repository', { name: repoName, url: repoUrl });
      setRepoName(''); setRepoUrl(''); setModal(null); await refresh();
      notify('Repositório adicionado. Sincronize para carregar o catálogo.');
    } catch (error) { notify(String(error), true); }
  };

  const syncRepository = async (id: number) => {
    setBusy(true);
    try { const count = await invoke<number>('sync_repository', { id }); await refresh(); notify(`${count} jogos encontrados no repositório.`); }
    catch (error) { notify(String(error), true); }
    finally { setBusy(false); }
  };

  const chooseFolder = async () => {
    const path = await open({ directory: true, multiple: false });
    if (path) setFolder(path as string);
  };
  const chooseExecutable = async () => {
    const path = await open({ multiple: false, filters: [{ name: 'Executável', extensions: ['exe', 'sh', 'AppImage', 'desktop'] }] });
    if (path) setExecutable(path as string);
  };

  const addLocalGame = async () => {
    if (!title.trim()) return notify('Informe o nome do jogo.', true);
    try {
      await invoke('add_game', { title, developer, description: '', installPath: folder || null, executable: executable || null, cover: null });
      setModal(null); setTitle(''); setDeveloper(''); setFolder(''); setExecutable(''); await refresh();
      notify('Jogo adicionado à biblioteca.');
    } catch (error) { notify(String(error), true); }
  };

  const startDownload = async (source: DownloadSource) => {
    const destination = await open({ directory: true, multiple: false, title: 'Escolha onde salvar o download' });
    if (!destination) return;
    setModal(null); setPage('downloads');
    setDownloads(current => [{ source_id: source.id, game_id: source.game_id, game_title: selected?.title || 'Jogo', file_name: source.name, downloaded: 0, total: null, status: 'starting', path: null, error: null }, ...current]);
    try {
      const path = await invoke<string>('download_game_source', { sourceId: source.id, destinationDir: destination });
      notify(`Arquivo salvo em ${path}`);
    } catch (error) {
      setDownloads(current => current.map(item => item.source_id === source.id ? { ...item, status: 'failed', error: String(error) } : item));
      notify(String(error), true);
    }
  };

  const launch = async (game: Game) => {
    if (!game.executable) return notify('Este jogo ainda não tem um executável configurado.', true);
    try { await invoke('launch_game', { executable: game.executable }); notify(`Iniciando ${game.title}…`); }
    catch (error) { notify(String(error), true); }
  };

  const nav: { id: Page; label: string; icon: typeof Compass }[] = [
    { id: 'store', label: 'Explorar', icon: Compass }, { id: 'library', label: 'Biblioteca', icon: Library },
    { id: 'downloads', label: 'Downloads', icon: Download }, { id: 'repositories', label: 'Repositórios', icon: Database },
    { id: 'settings', label: 'Configurações', icon: Settings }
  ];

  return <div className="app-shell">
    <header className="topbar">
      <button className="brand" onClick={() => { setPage('store'); setSelected(null); }}><span className="brand-mark"><Gamepad2 size={23}/></span><span>vapora</span></button>
      <div className="top-search"><Search size={16}/><input value={query} onChange={e => setQuery(e.target.value)} placeholder="Pesquisar jogos e biblioteca" /></div>
      <div className="top-status"><span className="status-dot"/> Modo local <span className="top-divider"/> <span className="player-avatar">P</span><span>Jogador</span></div>
    </header>
    <div className="app-body">
      <aside className="sidebar">
        <div className="side-label">MENU PRINCIPAL</div>
        {nav.map(item => { const Icon = item.icon; return <button key={item.id} className={`nav-link ${page === item.id ? 'active' : ''}`} onClick={() => { setPage(item.id); setSelected(null); }}><Icon size={18}/><span>{item.label}</span>{item.id === 'downloads' && downloads.some(d => d.status === 'downloading') && <i className="nav-pulse"/>}</button>; })}
        <div className="side-label library-label">SUA COLEÇÃO <button title="Adicionar jogo" onClick={() => setModal('local')}><Plus size={15}/></button></div>
        <div className="sidebar-games">{games.slice(0, 9).map(game => <button key={game.id} onClick={() => showGame(game)}><span className="mini-cover" style={game.cover ? { backgroundImage: `url(${game.cover})` } : undefined}>{!game.cover && game.title[0]}</span><span>{game.title}</span></button>)}</div>
        <div className="sidebar-bottom"><ShieldCheck size={15}/><span>Seus dados ficam no dispositivo</span></div>
      </aside>

      <main className="main-content">
        {page === 'store' && !selected && <>
          <div className="page-topline"><div><span className="kicker">VAPORA / DESCOBERTA</span><h1>Descubra seu próximo jogo.</h1><p>Explore os catálogos que você escolheu adicionar.</p></div><button className="subtle-button" onClick={() => setModal('repo')}><Plus size={16}/> Adicionar repositório</button></div>
          {featured ? <section className="feature-hero" style={featured.cover ? { backgroundImage: `linear-gradient(90deg, #0a1119 0%, #0a1119ed 31%, #0a111980 65%, #0a111955 100%), url("${featured.cover}")` } : undefined}>
            <div className="feature-copy"><span className="feature-tag"><Star size={13} fill="currentColor"/> DESTAQUE DO CATÁLOGO</span><h2>{featured.title}</h2><p>{featured.description || 'Veja os detalhes deste jogo e confira as opções disponíveis nos repositórios conectados.'}</p><div className="feature-meta"><span>{featured.developer || 'Desenvolvedor não informado'}</span><span>•</span><span>{featured.source}</span></div><div className="feature-actions"><button className="primary-button" onClick={() => showGame(featured)}><ArrowDownToLine size={17}/> Ver opções</button><button className="ghost-button" onClick={() => showGame(featured)}>Mais detalhes <ChevronRight size={16}/></button></div></div>
            <div className="feature-index"><span>CATÁLOGO</span><b>01</b><i/></div>
          </section> : <section className="welcome-empty"><div className="welcome-orb"><Gamepad2 size={34}/></div><div><span className="kicker">SUA LOJA, SUAS FONTES</span><h2>Adicione seu primeiro repositório.</h2><p>Os jogos aparecem aqui depois que você conectar e sincronizar um catálogo.</p><button className="primary-button" onClick={() => setModal('repo')}><Plus size={16}/> Conectar repositório</button></div></section>}
          <div className="section-heading"><div><span className="kicker">CATÁLOGO</span><h2>Jogos para explorar</h2><p>{catalog.length} jogos importados dos seus repositórios</p></div><button className="text-button" onClick={() => setPage('repositories')}>Gerenciar fontes <ChevronRight size={15}/></button></div>
          {filteredCatalog.length ? <div className="store-grid">{filteredCatalog.map(game => <GameTile key={game.id} game={game} onClick={() => showGame(game)} onDownload={() => void openSources(game)}/>)}</div> : <div className="empty-state"><LayoutGrid size={30}/><b>Nenhum jogo no catálogo</b><span>Adicione um repositório HTTPS e sincronize para preencher esta página.</span><button className="outline-button" onClick={() => setModal('repo')}>Adicionar repositório</button></div>}
        </>}

        {page === 'store' && selected && <>
          <button className="back-link" onClick={() => setSelected(null)}>‹ Voltar para explorar</button>
          <section className="game-detail-hero" style={selected.cover ? { backgroundImage: `linear-gradient(90deg, #0a1119 0%, #0a1119ed 38%, #0a111980 72%, #0a111955 100%), url("${selected.cover}")` } : undefined}>
            <div><span className="kicker">{selected.source.toUpperCase()}</span><h1>{selected.title}</h1><p>{selected.developer || 'Desenvolvedor não informado'}</p><div className="detail-buttons"><button className="primary-button" onClick={() => selected.executable ? void launch(selected) : void openSources(selected)}>{selected.executable ? <><Play size={17} fill="currentColor"/> Jogar</> : <><ArrowDownToLine size={17}/> Escolher fonte</>}</button><button className="ghost-button" onClick={() => selected.install_path ? invoke('reveal_path', { path: selected.install_path }).catch(e => notify(String(e), true)) : notify('Este jogo ainda não tem pasta de instalação.') }><Folder size={16}/> Pasta do jogo</button></div></div>
          </section>
          <div className="detail-layout"><section className="about-panel"><span className="kicker">SOBRE O JOGO</span><h2>Visão geral</h2><p>{selected.description || 'O repositório não forneceu uma descrição para este jogo.'}</p><div className="source-callout"><div className="source-callout-icon"><Database size={20}/></div><div><b>Escolha de onde baixar</b><span>O Vapora não escolhe uma fonte por você. Compare as opções e selecione um repositório confiável.</span></div><button className="primary-button" onClick={() => void openSources(selected)}>Ver fontes</button></div></section><aside className="game-facts"><span className="kicker">INFORMAÇÕES</span><div><small>Desenvolvedor</small><b>{selected.developer || '—'}</b></div><div><small>Repositório</small><b>{selected.source}</b></div><div><small>Status</small><b>{selected.executable ? 'Instalado' : 'Disponível no catálogo'}</b></div><div><small>Fontes disponíveis</small><b>Escolha ao baixar</b></div></aside></div>
        </>}

        {page === 'library' && <><div className="page-topline"><div><span className="kicker">VAPORA / COLEÇÃO</span><h1>Sua biblioteca</h1><p>Jogos instalados e atalhos adicionados por você.</p></div><button className="primary-button" onClick={() => setModal('local')}><Plus size={16}/> Adicionar jogo</button></div>{filteredLibrary.length ? <div className="store-grid library-grid">{filteredLibrary.map(game => <GameTile game={game} key={game.id} onClick={() => showGame(game)} onDownload={() => void launch(game)} library/> )}</div> : <div className="empty-state"><Library size={30}/><b>Sua biblioteca ainda está vazia</b><span>Adicione um jogo que já esteja instalado no computador.</span><button className="outline-button" onClick={() => setModal('local')}>Adicionar jogo instalado</button></div>}</>}

        {page === 'downloads' && <><div className="page-topline"><div><span className="kicker">VAPORA / TRANSFERÊNCIAS</span><h1>Downloads</h1><p>Acompanhe os arquivos transferidos das fontes escolhidas.</p></div></div><div className="downloads-list">{downloads.length ? downloads.map((item, index) => <div className="download-row" key={`${item.source_id}-${index}`}><div className="download-icon"><Download size={19}/></div><div className="download-info"><b>{item.game_title}</b><span>{item.file_name}</span><div className="progress-track"><i style={{ width: item.total ? `${Math.min(100, item.downloaded / item.total * 100)}%` : item.status === 'completed' ? '100%' : '22%' }}/></div><small>{item.total ? `${formatBytes(item.downloaded)} de ${formatBytes(item.total)}` : formatBytes(item.downloaded)} {item.status === 'completed' ? '• Concluído' : item.status === 'failed' ? `• ${item.error}` : '• Baixando'}</small></div><div className="download-state">{item.status === 'completed' ? <CheckCircle2 className="success-icon"/> : item.status === 'failed' ? <AlertCircle className="error-icon"/> : <LoaderCircle className="spin-icon"/>}{item.path && <><button className="icon-button" title="Abrir local do arquivo" onClick={() => void invoke('reveal_path', { path: item.path })}><Folder size={17}/></button>{item.status === 'completed' && item.path.toLowerCase().endsWith('.zip') && <button className="outline-button" onClick={async () => { const target = await open({ directory: true, multiple: false, title: 'Escolha a pasta para extrair o jogo' }); if (target) { try { const out = await invoke<string>('extract_zip', { archivePath: item.path!, destinationDir: target }); notify(`Arquivos extraídos em ${out}`); } catch (e) { notify(String(e), true); } } }}>Extrair ZIP</button>}</>}</div></div>) : <div className="empty-state"><Download size={30}/><b>Nenhum download iniciado</b><span>Quando você escolher uma fonte na página de um jogo, o andamento aparecerá aqui.</span><button className="outline-button" onClick={() => setPage('store')}>Explorar jogos</button></div>}</div><p className="fine-print">O Vapora suporta downloads HTTPS e BitTorrent (magnet/.torrent), além de extração segura de arquivos ZIP. Instaladores executáveis continuam sob sua confirmação.</p></>}

        {page === 'repositories' && <><div className="page-topline"><div><span className="kicker">VAPORA / FONTES</span><h1>Repositórios</h1><p>Conecte catálogos comunitários e escolha as fontes de cada jogo.</p></div><button className="primary-button" onClick={() => setModal('repo')}><Plus size={16}/> Adicionar repositório</button></div><div className="repo-intro"><ShieldCheck size={20}/><div><b>Você controla as fontes.</b><span>Adicione apenas repositórios que conhece. O Vapora não mantém nem recomenda listas de downloads de terceiros.</span></div></div><div className="repo-list">{repos.length ? repos.map(repo => <div className="repo-row" key={repo.id}><div className="repo-icon"><Database size={20}/></div><div className="repo-info"><b>{repo.name}</b><span>{repo.url}</span><small>{repo.last_sync ? `Última sincronização: ${repo.last_sync}` : 'Ainda não sincronizado'}</small></div><button className="outline-button" disabled={busy} onClick={() => void syncRepository(repo.id)}><RefreshCw size={14} className={busy ? 'spin-icon' : ''}/> Sincronizar</button><button className="icon-button danger" title="Remover repositório" onClick={async () => { if (confirm(`Remover ${repo.name} e os jogos deste catálogo?`)) { await invoke('remove_repository', { id: repo.id }); await refresh(); } }}><Trash2 size={17}/></button></div>) : <div className="empty-state compact"><Database size={28}/><b>Nenhum repositório conectado</b><span>Adicione uma URL de catálogo JSON para começar.</span></div>}</div></>}

        {page === 'settings' && <><div className="page-topline"><div><span className="kicker">VAPORA / PREFERÊNCIAS</span><h1>Configurações</h1><p>Preferências, atualizações e informações do aplicativo.</p></div></div><div className="settings-layout"><section className="settings-card"><div className="settings-section-title"><h2>Aplicativo</h2><span>Informações gerais</span></div><div className="setting-row"><div><b>Armazenamento</b><span>Biblioteca e repositórios salvos localmente em SQLite.</span></div><em>Local</em></div><div className="setting-row"><div><b>Privacidade</b><span>Sem conta, perfil remoto, analytics ou servidor próprio.</span></div><em><ShieldCheck size={14}/> Ativo</em></div><div className="setting-row"><div><b>Plataformas</b><span>Windows e Linux desktop.</span></div><em><Monitor size={14}/> Multiplataforma</em></div><div className="setting-row"><div><b>Versão instalada</b><span>Vapora Launcher</span></div><em>0.4.0</em></div></section><section className="update-card"><div className="update-heading"><div className="update-symbol"><RefreshCw size={22}/></div><div><h2>Atualizações</h2><p>Mantenha o Vapora na versão mais recente.</p></div></div><p className="update-description">Verifique se existe uma versão mais nova publicada no GitHub. Quando houver, você poderá abrir a página oficial para baixar o instalador.</p><button className="primary-button update-button" disabled={updateBusy} onClick={() => void checkUpdates()}><RefreshCw size={15} className={updateBusy ? 'spin-icon' : ''}/>{updateBusy ? 'Verificando...' : 'Verificar atualizações'}</button>{updateInfo.message && <div className={`update-result ${updateInfo.status}`}><span>{updateInfo.message}</span>{updateInfo.status === 'available' && updateInfo.url && <a href={updateInfo.url} target="_blank" rel="noreferrer">Abrir página da atualização <ChevronRight size={14}/></a>}</div>}<small>As atualizações são verificadas pela página oficial de lançamentos do projeto.</small></section></div></>}
      </main>
    </div>
    <footer className="app-footer"><span>VAPORA <i>•</i> LAUNCHER</span><span><span className="status-dot"/> {games.length} jogos na biblioteca <i>•</i> {repos.length} repositórios</span></footer>

    {modal && <div className="modal-backdrop" onMouseDown={() => setModal(null)}><div className="modal-card" onMouseDown={e => e.stopPropagation()}>
      <div className="modal-head"><div><span className="kicker">VAPORA</span><h2>{modal === 'repo' ? 'Adicionar repositório' : modal === 'local' ? 'Adicionar à biblioteca' : 'Escolher uma fonte'}</h2></div><button className="icon-button" onClick={() => setModal(null)}><X size={19}/></button></div>
      {modal === 'repo' && <><p className="modal-description">Informe a URL HTTPS de um catálogo JSON compatível. Depois, sincronize para importar os jogos e suas fontes.</p><label className="field-label">Nome do repositório<input value={repoName} onChange={e => setRepoName(e.target.value)} placeholder="Ex.: Minha comunidade"/></label><label className="field-label">URL do catálogo<input value={repoUrl} onChange={e => setRepoUrl(e.target.value)} placeholder="https://exemplo.org/catalog.json"/></label><div className="modal-actions"><button className="outline-button" onClick={() => setModal(null)}>Cancelar</button><button className="primary-button" onClick={() => void addRepository()}>Conectar</button></div><p className="fine-print">Use somente catálogos de fontes em que você confia e conteúdo que tenha direito de baixar.</p></>}
      {modal === 'local' && <><p className="modal-description">Cadastre um jogo já instalado para iniciá-lo pela biblioteca.</p><label className="field-label">Nome do jogo<input value={title} onChange={e => setTitle(e.target.value)} placeholder="Nome do jogo"/></label><label className="field-label">Desenvolvedor<input value={developer} onChange={e => setDeveloper(e.target.value)} placeholder="Estúdio (opcional)"/></label><label className="field-label">Pasta de instalação<div className="input-pick"><input value={folder} readOnly placeholder="Escolha a pasta"/><button className="outline-button" onClick={() => void chooseFolder()}><Folder size={14}/> Procurar</button></div></label><label className="field-label">Executável<div className="input-pick"><input value={executable} readOnly placeholder="Escolha o executável"/><button className="outline-button" onClick={() => void chooseExecutable()}>Procurar</button></div></label><div className="modal-actions"><button className="outline-button" onClick={() => setModal(null)}>Cancelar</button><button className="primary-button" onClick={() => void addLocalGame()}>Adicionar jogo</button></div></>}
      {modal === 'sources' && <><p className="modal-description">Selecione o repositório e a opção de download. Confira a origem antes de iniciar.</p>{sources.length ? <div className="source-options">{sources.map(source => <div className="source-option" key={source.id}><div className="source-option-icon"><Download size={18}/></div><div className="source-option-info"><b>{source.name}</b><span>{source.repository_name} · {source.size || 'Tamanho não informado'}</span><small>{source.kind === 'magnet' ? 'BitTorrent · Magnet' : source.kind === 'torrent' ? 'BitTorrent · Arquivo .torrent' : 'Download direto HTTPS'} {source.sha256 ? '· SHA-256 informado' : source.kind === 'direct' ? '· Sem hash de verificação' : '· Verificação por peças BitTorrent'}</small></div><button className="primary-button" onClick={() => void startDownload(source)}>Escolher <ChevronRight size={15}/></button></div>)}</div> : <div className="empty-state compact"><AlertCircle size={25}/><b>Nenhuma fonte cadastrada para este jogo</b><span>Verifique se o repositório foi sincronizado e se o catálogo oferece fontes HTTPS ou BitTorrent compatíveis.</span><button className="outline-button" onClick={() => { setModal(null); setPage('repositories'); }}>Ver repositórios</button></div>}<div className="modal-actions"><button className="outline-button" onClick={() => setModal(null)}>Fechar</button></div></>}
    </div></div>}
    {notice && <div className={`toast ${noticeError ? 'toast-error' : ''}`}>{noticeError ? <AlertCircle size={17}/> : <CheckCircle2 size={17}/>}<span>{notice}</span></div>}
  </div>;
}

function GameTile({ game, onClick, onDownload, library = false }: { game: Game; onClick: () => void; onDownload: () => void; library?: boolean }) {
  return <article className="game-tile"><button className="tile-art" onClick={onClick} style={game.cover ? { backgroundImage: `linear-gradient(0deg, #080d13dd 0%, transparent 62%), url("${game.cover}")` } : undefined}>{!game.cover && <span className="tile-placeholder">{game.title.slice(0, 1).toUpperCase()}</span>}<span className="tile-hover">{library ? <Play size={21} fill="currentColor"/> : <ArrowDownToLine size={21}/>}</span></button><div className="tile-meta"><button className="tile-title" onClick={onClick}>{game.title}</button><span>{game.developer || 'Desenvolvedor não informado'}</span><div className="tile-bottom"><small>{library ? (game.executable ? 'INSTALADO' : 'ADICIONADO') : game.source}</small><button onClick={onDownload} title={library ? 'Iniciar jogo' : 'Ver opções de download'}>{library ? <Play size={15}/> : <ArrowDownToLine size={15}/>}</button></div></div></article>;
}
