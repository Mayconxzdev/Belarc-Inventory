const API = '';
let machines = [];
let alerts = [];
let fleetHealth = null;
let companyProfile = null;
let selectedId = null;
let currentDetail = null;
let currentIncidents = [];
let maintenancePlaybook = null;
let activeTab = 'admin';
let currentView = 'inventory';
let adminFormDirty = false;
let tiToken = sessionStorage.getItem('belarc-ti-token') || '';
let ticketList = [];
let selectedTicketId = null;

function authHeaders() {
  return tiToken ? { Authorization: `Bearer ${tiToken}` } : {};
}

function setTiUi(authenticated, username = '') {
  const user = document.getElementById('auth-user');
  const login = document.getElementById('btn-login');
  const logout = document.getElementById('btn-logout');
  const privileged = ['view-directory-btn', 'view-tickets-btn', 'view-bi-btn'];
  if (user) {
    user.textContent = username || '';
    user.classList.toggle('hidden', !authenticated);
  }
  if (login) login.classList.toggle('hidden', authenticated);
  if (logout) logout.classList.toggle('hidden', !authenticated);
  privileged.forEach(id => document.getElementById(id)?.classList.toggle('hidden', !authenticated));
}

function closeLoginModal() {
  const modal = document.getElementById('login-modal');
  modal?.classList.add('hidden');
  modal?.setAttribute('aria-hidden', 'true');
  document.getElementById('login-error')?.classList.add('hidden');
}

function openLoginModal() {
  const modal = document.getElementById('login-modal');
  modal?.classList.remove('hidden');
  modal?.setAttribute('aria-hidden', 'false');
  document.querySelector('#login-form input[name="username"]')?.focus();
}

async function restoreTiSession() {
  if (!tiToken) {
    setTiUi(false);
    return false;
  }
  try {
    const response = await fetch(API + '/api/auth/me', { headers: authHeaders() });
    if (!response.ok) throw new Error('Sessão indisponível');
    const me = await response.json();
    if (!me.authenticated) throw new Error('Sessão expirada');
    setTiUi(true, me.username || 'TI');
    return true;
  } catch {
    tiToken = '';
    sessionStorage.removeItem('belarc-ti-token');
    setTiUi(false);
    return false;
  }
}

function adminDraftKey(id) {
  return `belarc-admin-draft-${id}`;
}

function saveAdminDraft() {
  const form = document.getElementById('admin-form');
  if (!form || !selectedId) return;
  sessionStorage.setItem(adminDraftKey(selectedId), JSON.stringify(Object.fromEntries(new FormData(form).entries())));
}

function loadAdminDraft() {
  if (!selectedId) return null;
  try {
    const raw = sessionStorage.getItem(adminDraftKey(selectedId));
    return raw ? JSON.parse(raw) : null;
  } catch { return null; }
}

function clearAdminDraft() {
  if (selectedId) sessionStorage.removeItem(adminDraftKey(selectedId));
  adminFormDirty = false;
}

function formatUptime(s) {
  if (s == null || s === '') return '—';
  const n = Number(s);
  if (!n) return '—';
  const d = Math.floor(n / 86400);
  const h = Math.floor((n % 86400) / 3600);
  const m = Math.floor((n % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  return `${h}h ${m}m`;
}

function formatDate(iso) {
  if (!iso) return '—';
  return new Date(iso).toLocaleString('pt-BR');
}

function esc(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function scoreBand(score) {
  if (score == null || score === '') return '—';
  if (score >= 90) return 'Excelente';
  if (score >= 75) return 'Bom';
  if (score >= 50) return 'Atenção';
  if (score >= 25) return 'Problema';
  return 'Crítico';
}

function scoreClass(s) {
  if (s == null) return '';
  if (s >= 90) return 'good';
  if (s >= 75) return 'good';
  if (s >= 50) return 'warn';
  if (s >= 25) return 'warn';
  return 'bad';
}

function alertCountForMachine(id) {
  return dedupeAlerts(alerts.filter(a => a.machine_id === id)).length;
}

async function fetchJson(path, options = {}) {
  const headers = { ...authHeaders(), ...(options.headers || {}) };
  const res = await fetch(API + path, { ...options, headers });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

async function ensureCompanyProfile() {
  if (companyProfile) return companyProfile;
  try {
    companyProfile = await fetchJson('/api/company-profile');
  } catch {
    companyProfile = {
      erp_name: 'AcmeERP',
      erp_server_ip: '10.0.0.50',
      antivirus_name: 'ESET',
      banking_app: { id: 'banking', label: 'App Bancário Corporativo' },
      erp_branches: [
        { id: 'erp_branch_a', label: 'AcmeERP (Filial Alpha)' },
        { id: 'erp_branch_b', label: 'AcmeERP (Filial Beta)' },
      ],
      ui: {
        erp_user: 'ERP — usuário',
        erp_access_reference: 'Referência de acesso (senha não armazenada)',
        erp_offline_warning: 'offline — ERP pode travar',
        freeze_prone_description: 'ERP (LAN), Fusion 360, AutoCAD e LibreOffice Draw — monitorados no registro de incidentes.',
        critical_apps_summary: 'ERP, Fusion 360, AutoCAD, LibreOffice Draw',
      },
    };
  }
  return companyProfile;
}

function erpBranchLabel(id) {
  const branch = (companyProfile?.erp_branches || []).find(b => b.id === id);
  return branch?.label || id;
}

function bankingAppLabel() {
  return companyProfile?.banking_app?.label || 'App Bancário';
}

function bankingAppId() {
  return companyProfile?.banking_app?.id || 'banking';
}

function getCollector(name) {
  return currentDetail?.collectors?.find(c => c.name === name)?.data || {};
}

function buildViewModel() {
  const m = currentDetail?.machine || {};
  const h = currentDetail?.highlight || {};
  const id = getCollector('identity');
  const os = getCollector('os');
  const net = getCollector('network');
  const ra = getCollector('remote_access');
  const em = getCollector('email');
  const lic = getCollector('licensing');
  const ev = getCollector('event_logs');
  const hw = getCollector('hardware');
  const perf = getCollector('performance');
  const ts = ra.tailscale || {};
  const ad = ra.anydesk || {};
  const tv = ra.teamviewer || {};
  const tbSum = em.thunderbird_summary || {};

  let thunderbirdEmails = h.thunderbird_emails?.length ? [...h.thunderbird_emails] : [...(tbSum.all_emails || [])];
  if (!thunderbirdEmails.length && em.thunderbird) {
    thunderbirdEmails = [...new Set((em.thunderbird || []).flatMap(p => p.emails || []))];
  }

  const evSum = ev.summary || {};
  const admin = currentDetail?.admin || h.admin || {};
  const licenseKeys = h.license_keys || {
    windows_key_partial: lic.windows?.product_key_partial,
    windows_product_key: lic.windows?.product_key,
    office_keys: lic.office || [],
  };
  const standardApps = h.standard_apps || [];

  return {
    ...h,
    admin,
    machine: m,
    hostname: m.hostname || h.hostname || id.hostname,
    status: m.status || h.status,
    logged_user: m.logged_user || h.logged_user || id.logged_user,
    last_seen: m.last_seen || h.last_seen,
    health_score: m.health_score ?? h.health_score,
    lan_ip: h.lan_ip || net.primary_lan_ip || id.ip_primary || m.ip_address,
    lan_ips: (h.lan_ips?.length ? h.lan_ips : null) || net.lan_ips || [],
    tailscale_installed: h.tailscale_installed || ts.installed || false,
    tailscale_connected: h.tailscale_connected || ts.connected || false,
    tailscale_ip: h.tailscale_ip || (ts.tailscale_ips || []).find(ip => !String(ip).includes(':')) || (ts.tailscale_ips || [])[0] || null,
    tailscale_dns: h.tailscale_dns || ts.dns_name,
    tailscale_startup: h.tailscale_startup || ts.startup_automatic || false,
    anydesk_id: h.anydesk_id || ad.client_id || null,
    anydesk_running: h.anydesk_running || ad.service_running || false,
    teamviewer_id: tv.client_id || null,
    os_version: h.os_version || os.caption || os.version,
    model: h.model || id.model || hw.system?.model,
    uptime_seconds: m.uptime_seconds ?? h.uptime_seconds ?? id.uptime_seconds,
    thunderbird_emails: thunderbirdEmails,
    thunderbird_profile: h.thunderbird_profile || tbSum.default_profile,
    windows_product_key: h.windows_product_key || lic.windows?.product_key,
    windows_key_partial: h.windows_key_partial || lic.windows?.product_key_partial,
    office_licenses: h.office_licenses?.length ? h.office_licenses : (lic.office || []).map(o => {
      const name = o.license_name || o.product_id;
      const key = o.key_partial;
      if (name && key) return `${name} (***-${key})`;
      return name || key || null;
    }).filter(Boolean),
    has_recent_bsod: h.has_recent_bsod || evSum.has_recent_bsod || false,
    last_bsod_summary: h.last_bsod_summary || evSum.last_bsod_summary,
    last_bugcheck_code: h.last_bugcheck_code || evSum.last_bugcheck_code,
    system_errors_count: h.system_errors_count || evSum.system_errors_count || 0,
    display_email: h.display_email || admin.primary_email || thunderbirdEmails[0] || null,
    owner_name: admin.owner_name,
    ramal: admin.ramal,
    network_cable: admin.network_cable,
    license_keys: licenseKeys,
    standard_apps: standardApps,
    standard_apps_installed: h.standard_apps_installed ?? standardApps.filter(a => a.installed).length,
    eset_installed: h.eset_installed ?? h.eset_info?.installed ?? standardApps.find(a => a.id === 'eset')?.installed,
    eset_product: h.eset_product,
    eset_info: h.eset_info || {},
    folder_access: h.folder_access?.length ? h.folder_access : (net.smb_mappings || []).map(m => {
      const l = m.local || '';
      const r = m.remote || '';
      return l && r ? `${l} → ${r}` : (r || l);
    }).filter(Boolean),
    application_errors_count: evSum.application_errors_count || 0,
    service_failures_count: evSum.service_failures_count || 0,
    minidump_count: evSum.minidump_count || 0,
    unexpected_shutdowns: evSum.unexpected_shutdowns || 0,
    critical_app_events: ev.critical_app_events || [],
    critical_app_events_count: evSum.critical_app_events_count || 0,
    critical_app_events_today: evSum.critical_app_events_today || 0,
    app_crash_hang: ev.app_crash_hang || [],
    disk_events: ev.disk_events || [],
    whea_events: ev.whea_events || [],
    event_daily_timeline: ev.daily_timeline || null,
    reliability_records: perf.reliability_records || [],
    startup_programs: getCollector('software').startup || [],
    program_count: getCollector('software').program_count || 0,
    logical_disks: hw.logical_disks || hw.disks || [],
    physical_disks: hw.physical_disks || [],
    ram_total_gb: hw.ram?.total_gb || hw.system?.total_physical_memory_gb,
    ram_modules: hw.ram?.modules || [],
    cpu_info: (hw.cpu || [])[0] || null,
    gpu_info: (hw.gpu || [])[0] || null,
    os_build: os.build,
    os_last_boot: os.last_boot || id.last_boot,
    hotfix_count: os.hotfix_count,
    serial: id.serial || m.serial,
    mac_primary: id.mac_primary,
    agent_status: null,
    perf_cpu_load: perf.cpu?.load_percent,
    perf_mem_used_pct: perf.memory?.used_percent,
    perf_mem_total_gb: perf.memory?.total_gb,
    perf_disk_time: perf.physical_disk_io?.peak_disk_time_percent ?? perf.physical_disk_io?.total_disk_time_percent,
    perf_disk_queue: perf.physical_disk_io?.avg_queue_length,
    perf_disk_samples: perf.physical_disk_io?.disk_time_samples || [],
    perf_disk_queue_samples: perf.physical_disk_io?.disk_queue_samples || [],
    perf_temperatures: perf.temperatures || [],
    perf_max_temp: perf.summary?.max_temperature_celsius,
    perf_peak_disk: perf.summary?.peak_disk_time_percent ?? perf.physical_disk_io?.peak_disk_time_percent,
    perf_daily_timeline: perf.daily_timeline || null,
    perf_top_cpu: perf.top_processes?.by_cpu || [],
    perf_top_mem: perf.top_processes?.by_memory || [],
    perf_reliability_failures: perf.summary?.reliability_failures || 0,
    perf_signals: perf.signals || [],
    perf_collected_at: perf.collected_at,
    erp_server: perf.erp_server || null,
    known_heavy_apps: perf.known_heavy_apps || [],
    disk_errors_count: evSum.disk_errors_count || 0,
    whea_events_count: evSum.whea_events_count || 0,
    timeline_today_count: (evSum.timeline_today_count || 0) + (perf.summary?.timeline_today_count || 0),
  };
}

function formatDiskFree(disk) {
  const pct = disk.free_percent;
  const cls = pct < 10 ? 'bad' : pct < 20 ? 'warn' : 'ok';
  return { text: `${disk.letter || '?'}: ${disk.free_gb} GB livres (${pct}%)`, cls };
}

function formatUptimeDays(seconds) {
  if (!seconds) return null;
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  return d > 0 ? `${d} dia(s) e ${h}h` : formatUptime(seconds);
}

function renderPerfMetrics(vm) {
  const disks = vm.logical_disks || [];
  const worstDisk = disks.length
    ? disks.reduce((a, b) => (a.free_percent < b.free_percent ? a : b))
    : null;
  const startup = vm.startup_programs || [];
  const ram = vm.ram_total_gb;
  const cpu = vm.cpu_info;
  const uptime = formatUptimeDays(vm.uptime_seconds);

  const cpuLoad = vm.perf_cpu_load;
  const memPct = vm.perf_mem_used_pct;
  const diskTime = vm.perf_disk_time;
  const maxTemp = vm.perf_max_temp;

  const metrics = [
    { label: 'CPU agora', value: cpuLoad != null ? `${cpuLoad}%` : '—', hint: cpuLoad >= 90 ? 'Pico — possível lag' : cpuLoad >= 75 ? 'Elevada' : cpu?.cores ? `${cpu.cores} núcleos` : '', cls: cpuLoad >= 90 ? 'bad' : cpuLoad >= 75 ? 'warn' : 'ok' },
    { label: 'RAM em uso', value: memPct != null ? `${memPct}%` : '—', hint: memPct >= 90 ? 'Memória crítica' : ram ? `${ram} GB total` : '', cls: memPct >= 90 ? 'bad' : memPct >= 75 ? 'warn' : 'ok' },
    { label: 'RAM total', value: ram ? `${ram} GB` : '—', hint: ram && ram < 8 ? 'Upgrade recomendado' : ram && ram >= 16 ? 'Adequado' : 'Aceitável', cls: ram && ram < 8 ? 'bad' : ram && ram < 16 ? 'warn' : 'ok' },
    { label: 'CPU (modelo)', value: cpu?.name ? cpu.name.replace(/\s+/g, ' ').slice(0, 36) : '—', hint: '', cls: 'ok' },
    { label: 'Disco I/O (pico)', value: diskTime != null ? `${diskTime}%` : '—', hint: diskTime >= 90 ? 'SSD/HDD saturado — causa delay' : vm.perf_disk_queue != null ? `fila ${vm.perf_disk_queue}` : '', cls: diskTime >= 90 ? 'bad' : diskTime >= 70 ? 'warn' : 'ok' },
    { label: 'Apps críticos (14d)', value: vm.critical_app_events_count ?? 0, hint: vm.critical_app_events_today ? `${vm.critical_app_events_today} hoje` : 'OperationsSuite, CAD, Libre', cls: (vm.critical_app_events_today || 0) > 0 ? 'bad' : (vm.critical_app_events_count || 0) > 0 ? 'warn' : 'ok' },
    { label: 'Erros disco (14d)', value: vm.disk_errors_count ?? 0, hint: vm.disk_errors_count ? 'Ver Event Viewer System' : '', cls: (vm.disk_errors_count || 0) > 0 ? 'bad' : 'ok' },
    { label: 'Temperatura', value: maxTemp != null ? `${Math.round(maxTemp)}°C` : '—', hint: maxTemp >= 90 ? 'Crítica' : maxTemp >= 80 ? 'Alta' : '', cls: maxTemp >= 90 ? 'bad' : maxTemp >= 80 ? 'warn' : 'ok' },
    { label: 'Uptime', value: uptime || '—', hint: (vm.uptime_seconds || 0) > 21 * 86400 ? 'Reiniciar recomendado' : 'OK', cls: (vm.uptime_seconds || 0) > 30 * 86400 ? 'warn' : 'ok' },
    { label: 'Disco mais cheio', value: worstDisk ? `${worstDisk.letter} ${worstDisk.free_percent}% livre` : '—', hint: worstDisk && worstDisk.free_percent < 15 ? 'Liberar espaço' : '', cls: worstDisk && worstDisk.free_percent < 10 ? 'bad' : worstDisk && worstDisk.free_percent < 20 ? 'warn' : 'ok' },
    { label: 'Inicialização', value: `${startup.length} programa(s)`, hint: startup.length > 15 ? 'Reduzir para acelerar boot' : 'OK', cls: startup.length > 25 ? 'bad' : startup.length > 12 ? 'warn' : 'ok' },
    { label: 'Programas instalados', value: vm.program_count || '—', hint: '', cls: 'ok' },
    { label: 'Erros Windows (14d)', value: vm.system_errors_count ?? 0, hint: vm.system_errors_count > 20 ? 'Ver Event Viewer' : '', cls: vm.system_errors_count > 50 ? 'bad' : vm.system_errors_count > 20 ? 'warn' : 'ok' },
    { label: 'Erros de apps (14d)', value: vm.application_errors_count ?? 0, hint: '', cls: vm.application_errors_count > 50 ? 'warn' : 'ok' },
    { label: 'BSOD / quedas', value: vm.has_recent_bsod ? 'BSOD detectado' : (vm.unexpected_shutdowns || 0) ? `${vm.unexpected_shutdowns} queda(s)` : 'Nenhum', hint: '', cls: vm.has_recent_bsod ? 'bad' : vm.unexpected_shutdowns > 0 ? 'warn' : 'ok' },
    { label: 'Atualizações Win', value: vm.hotfix_count != null ? `${vm.hotfix_count} patch(es)` : '—', hint: '', cls: 'ok' },
    { label: 'ESET', value: vm.eset_installed ? (vm.eset_info?.product_name || 'Ativo') : 'Ausente', hint: vm.eset_info?.version ? `v${vm.eset_info.version}` : '', cls: vm.eset_installed ? 'ok' : 'bad' },
    { label: 'Software padrão', value: `${vm.standard_apps_installed || 0}/${(vm.standard_apps || []).length}`, hint: '', cls: (vm.standard_apps_installed || 0) >= (vm.standard_apps || []).length ? 'ok' : 'warn' },
  ];

  return `<div class="perf-metrics-grid">${metrics.map(m => `
    <div class="perf-metric ${m.cls}">
      <span class="perf-metric-lbl">${esc(m.label)}</span>
      <span class="perf-metric-val">${esc(String(m.value))}</span>
      ${m.hint ? `<span class="perf-metric-hint">${esc(m.hint)}</span>` : ''}
    </div>`).join('')}</div>`;
}

function renderDiskTable(vm) {
  const disks = vm.logical_disks || [];
  if (!disks.length) return '<p class="empty-note">Sem dados de disco.</p>';
  return `<table class="data-table perf-disk-table"><thead><tr>
    <th>Unidade</th><th>Tamanho</th><th>Livre</th><th>% Livre</th><th>Sistema</th><th>Status TI</th>
  </tr></thead><tbody>${disks.map(d => {
    const pct = d.free_percent ?? 0;
    const st = pct < 10 ? 'Crítico — liberar já' : pct < 20 ? 'Atenção' : pct < 30 ? 'Monitorar' : 'OK';
    const cls = pct < 10 ? 'bad' : pct < 20 ? 'warn' : 'ok';
    return `<tr class="${cls}"><td>${esc(d.letter || '—')}</td><td>${esc(d.size_gb != null ? `${d.size_gb} GB` : '—')}</td>
      <td>${esc(d.free_gb != null ? `${d.free_gb} GB` : '—')}</td><td>${esc(pct + '%')}</td>
      <td>${esc(d.filesystem || '—')}</td><td>${esc(st)}</td></tr>`;
  }).join('')}</tbody></table>`;
}

function renderStartupTable(vm) {
  const items = (vm.startup_programs || []).slice(0, 20);
  if (!items.length) return '<p class="empty-note">Nenhum programa na inicialização detectado.</p>';
  return `<table class="data-table"><thead><tr><th>Programa</th><th>Origem</th></tr></thead>
    <tbody>${items.map(s => `<tr><td>${esc(s.name || '—')}</td><td>${esc(s.source || '—')}</td></tr>`).join('')}
    </tbody></table>${(vm.startup_programs || []).length > 20 ? `<p class="empty-note">+ ${vm.startup_programs.length - 20} outros (desative em Configurações > Aplicativos > Inicializar)</p>` : ''}`;
}

const TIMELINE_CATEGORY_LABELS = {
  known_app_hang: 'Travamento app crítico',
  known_app_crash: 'Falha app crítico',
  app_hang: 'Hang',
  app_crash: 'Falha',
  disk_saturation: 'SSD/Disco 100%',
  disk_queue: 'Fila de disco',
  disk_error: 'Erro disco/SSD',
  temp_high: 'Temperatura alta',
  temp_critical: 'Temp. crítica',
  erp_unreachable: 'OperationsSuite offline',
  cpu_high: 'CPU alta',
  memory_high: 'Memória alta',
  bsod: 'Tela azul',
  unexpected_shutdown: 'Queda energia',
  whea_hardware: 'Hardware WHEA',
  bugcheck: 'BugCheck',
  system_failure: 'Falha Windows',
};

function mergeDailyTimeline(vm) {
  const ev = vm.event_daily_timeline?.events || [];
  const perf = vm.perf_daily_timeline?.events || [];
  const seen = new Set();
  const merged = [];
  for (const e of [...ev, ...perf]) {
    const title = e.title || e.message || e.category;
    const k = `${e.time}|${e.category}|${title}`;
    if (seen.has(k)) continue;
    seen.add(k);
    merged.push({ ...e, title });
  }
  merged.sort((a, b) => (b.time || '').localeCompare(a.time || ''));
  return merged;
}

function formatTimeOnly(iso) {
  if (!iso) return '—';
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  } catch { return iso; }
}

function renderDailyTimeline(vm) {
  const date = vm.event_daily_timeline?.date || vm.perf_daily_timeline?.date || new Date().toISOString().slice(0, 10);
  const events = mergeDailyTimeline(vm);
  if (!events.length) {
    return `<div class="section daily-timeline-section">
      <h3>Linha do tempo — hoje (${esc(date)})</h3>
      <p class="section-desc">Travamentos, SSD 100%, temperatura e falhas ao longo do dia. Os eventos aparecem após coletas do agente e registros no Event Viewer.</p>
      <p class="empty-note">Nenhum evento registrado hoje ainda.</p>
    </div>`;
  }
  const byHour = {};
  for (const e of events) {
    const h = e.hour != null ? e.hour : (e.time ? new Date(e.time).getHours() : 0);
    if (!byHour[h]) byHour[h] = [];
    byHour[h].push(e);
  }
  const hours = Object.keys(byHour).map(Number).sort((a, b) => b - a);
  const rows = events.slice(0, 40).map(e => {
    const cat = TIMELINE_CATEGORY_LABELS[e.category] || e.category || '—';
    const sev = e.severity || 'info';
    const extra = [
      e.event_id != null ? `ID ${e.event_id}` : '',
      e.exception_code ? `cod ${e.exception_code}` : '',
      e.fault_module ? `mod ${e.fault_module}` : '',
      e.bugcheck ? `BSOD ${e.bugcheck}` : '',
      e.value != null ? `${e.value}${e.threshold != null ? ` (lim ${e.threshold})` : ''}` : '',
      e.app_label || '',
    ].filter(Boolean).join(' · ');
    return `<tr class="timeline-sev-${sev}">
      <td>${esc(formatTimeOnly(e.time))}</td>
      <td><span class="timeline-cat timeline-cat-${e.category || 'other'}">${esc(cat)}</span></td>
      <td>${esc(e.title || '—')}</td>
      <td class="timeline-extra">${esc(extra || '—')}</td>
    </tr>`;
  }).join('');
  const hourSummary = hours.map(h => {
    const items = byHour[h];
    const cats = [...new Set(items.map(i => TIMELINE_CATEGORY_LABELS[i.category] || i.category))];
    return `<li><strong>${String(h).padStart(2, '0')}h</strong> — ${items.length} evento(s): ${esc(cats.join(', '))}</li>`;
  }).join('');
  return `<div class="section daily-timeline-section">
    <h3>Linha do tempo — hoje (${esc(date)}) — ${events.length} evento(s)</h3>
    <p class="section-desc">Correlaciona travamentos (OperationsSuite, LibreOffice, AutoCAD, Fusion), SSD 100%, temperatura e erros de disco ao longo do dia.</p>
    ${hours.length ? `<ul class="timeline-hour-summary">${hourSummary}</ul>` : ''}
    <table class="data-table timeline-table"><thead><tr>
      <th>Hora</th><th>Tipo</th><th>Evento</th><th>Códigos / detalhes</th>
    </tr></thead><tbody>${rows}</tbody></table>
    ${events.length > 40 ? `<p class="empty-note">+ ${events.length - 40} eventos no relatório .md</p>` : ''}
  </div>`;
}

function renderCriticalAppEventsLog(vm) {
  const events = vm.critical_app_events || [];
  if (!events.length) {
    return `<div class="section critical-apps-log-section">
      <h3>Logs de apps críticos (14 dias)</h3>
      <p class="section-desc">Event Viewer — IDs 1000 (falha) e 1002 (travamento/hang) para OperationsSuite, Fusion 360, AutoCAD e LibreOffice.</p>
      <p class="empty-note">Nenhum travamento de app crítico nos últimos 14 dias.</p>
    </div>`;
  }
  return `<div class="section critical-apps-log-section">
    <h3>Logs de apps críticos (${events.length} em 14 dias${vm.critical_app_events_today ? `, ${vm.critical_app_events_today} hoje` : ''})</h3>
    <p class="section-desc">Event Viewer Application — falhas e hangs com código de exceção, módulo e horário exato.</p>
    <table class="data-table app-events-table"><thead><tr>
      <th>Data/Hora</th><th>Programa</th><th>Tipo</th><th>Event ID</th><th>Código</th><th>Módulo</th><th>Detalhe</th>
    </tr></thead><tbody>${events.slice(0, 25).map(e => {
      const type = e.category === 'known_app_hang' ? 'Travamento' : 'Falha';
      const cls = e.category === 'known_app_hang' ? 'warn' : 'bad';
      return `<tr class="${cls}">
        <td>${formatDate(e.time)}</td>
        <td>${esc(e.app_label || e.app_name || '—')}</td>
        <td>${esc(type)}</td>
        <td>${esc(e.event_id ?? '—')}</td>
        <td>${esc(e.exception_code || '—')}</td>
        <td>${esc(e.fault_module || '—')}</td>
        <td class="app-event-detail">${esc((e.detail || e.message || '').slice(0, 120))}</td>
      </tr>`;
    }).join('')}</tbody></table>
  </div>`;
}

function renderDiskIoSamples(vm) {
  const samples = vm.perf_disk_samples || [];
  const peak = vm.perf_peak_disk ?? vm.perf_disk_time;
  if (!samples.length && peak == null) return '';
  const rows = samples.map(s => {
    const v = s.value;
    const cls = v >= 90 ? 'bad' : v >= 70 ? 'warn' : 'ok';
    return `<tr class="${cls}"><td>${esc(formatTimeOnly(s.time))}</td><td>${esc(v)}%</td><td>${v >= 90 ? 'Saturado (SSD 100%)' : v >= 70 ? 'Alto' : 'Normal'}</td></tr>`;
  }).join('');
  return `<div class="section disk-io-section">
    <h3>Disco I/O — amostras na coleta${peak != null ? ` (pico ${Math.round(peak)}%)` : ''}</h3>
    <p class="section-desc">5 amostras de % Disk Time durante a coleta (~5s). Pico ≥90% indica SSD/HDD saturado — causa delay no OperationsSuite e apps pesados.</p>
    ${samples.length ? `<table class="data-table"><thead><tr><th>Hora</th><th>% Disk Time</th><th>Status</th></tr></thead><tbody>${rows}</tbody></table>` : ''}
    ${(vm.disk_events || []).length ? `<p class="section-desc warn-text">${vm.disk_events.length} erro(s) de disco no Event Viewer (System) nos últimos 14 dias.</p>` : ''}
  </div>`;
}

function renderTemperatureLog(vm) {
  const temps = vm.perf_temperatures || [];
  if (!temps.length) return '';
  return `<div class="section temperature-log-section">
    <h3>Temperatura na coleta</h3>
    <table class="data-table"><thead><tr><th>Fonte</th><th>Dispositivo</th><th>°C</th><th>Erros leitura</th><th>Desgaste SSD</th></tr></thead>
    <tbody>${temps.map(t => {
      const c = t.celsius;
      const cls = c >= 90 ? 'bad' : c >= 80 ? 'warn' : 'ok';
      const readErr = t.read_errors != null ? t.read_errors : '—';
      const wear = t.wear_percent != null ? `${t.wear_percent}%` : '—';
      return `<tr class="${cls}"><td>${esc(t.source || '—')}</td><td>${esc(t.instance || '—')}</td><td>${esc(c)}</td><td>${esc(readErr)}</td><td>${esc(wear)}</td></tr>`;
    }).join('')}</tbody></table>
  </div>`;
}

function renderReliabilityLog(vm) {
  const recs = (vm.reliability_records || []).filter(r => [1, 2, 4, 10].includes(r.record_type_id));
  if (!recs.length) return '';
  return `<div class="section reliability-log-section">
    <h3>Reliability Monitor — falhas recentes</h3>
    <table class="data-table"><thead><tr><th>Data/Hora</th><th>Tipo</th><th>Produto</th><th>Event ID</th><th>Mensagem</th></tr></thead>
    <tbody>${recs.slice(0, 15).map(r => `<tr>
      <td>${formatDate(r.time)}</td>
      <td>${esc(r.record_type || '—')}</td>
      <td>${esc(r.product || '—')}</td>
      <td>${esc(r.event_id ?? '—')}</td>
      <td>${esc((r.message || '').slice(0, 100))}</td>
    </tr>`).join('')}</tbody></table>
  </div>`;
}

const INCIDENT_CATEGORY_LABELS = {
  cpu_high: 'CPU alta',
  cpu_elevated: 'CPU elevada',
  memory_high: 'Memória alta',
  disk_critical: 'Disco crítico',
  disk_low: 'Pouco espaço',
  disk_saturation: 'Disco 100%',
  disk_queue: 'Fila de disco',
  temp_high: 'Temperatura',
  temp_critical: 'Temp. crítica',
  app_crash: 'App travou',
  app_hang: 'App hang',
  erp_unreachable: 'ERP offline',
  known_app_crash: 'App critico falhou',
  known_app_hang: 'App critico travou',
  bugcheck: 'BugCheck',
  system_failure: 'Falha Windows',
  bsod: 'BSOD',
  unexpected_shutdown: 'Queda energia',
  app_errors_high: 'Erros de apps',
  disk_error: 'Erro disco/SSD',
  whea_hardware: 'Hardware WHEA',
  smart_unhealthy: 'SMART',
};

function renderIncidentRegistry(incidents) {
  if (!incidents?.length) {
    return '<p class="empty-note">Nenhum incidente registrado ainda. Após coletas com o coletor <em>performance</em>, travamentos, picos de disco e temperatura aparecem aqui com plano de ação.</p>';
  }
  return `<table class="data-table incident-registry-table"><thead><tr>
    <th>Última vez</th><th>Tipo</th><th>Severidade</th><th>Detalhe</th><th>Valor</th><th>Plano de ação</th><th>#</th>
  </tr></thead><tbody>${incidents.slice(0, 30).map(inc => {
    const sev = inc.severity || 'info';
    const cls = sev === 'critical' ? 'bad' : sev === 'warning' ? 'warn' : '';
    const val = inc.metric_value != null ? `${Math.round(inc.metric_value * 10) / 10}` : '—';
    const rec = inc.recommendation || '—';
    const cat = INCIDENT_CATEGORY_LABELS[inc.category] || inc.title || inc.category;
    return `<tr class="${cls}">
      <td>${formatDate(inc.last_seen)}</td>
      <td>${esc(cat)}</td>
      <td><span class="incident-sev incident-sev-${sev}">${esc(sev)}</span></td>
      <td>${esc(inc.message)}</td>
      <td>${esc(val)}</td>
      <td class="incident-rec-cell">${esc(rec)}</td>
      <td>${inc.occurrence_count || 1}</td>
    </tr>`;
  }).join('')}</tbody></table>
  ${incidents.length > 30 ? `<p class="empty-note">+ ${incidents.length - 30} registros anteriores no relatório .md</p>` : ''}`;
}

function renderFreezeProneApps(vm) {
  const apps = vm.known_heavy_apps?.length
    ? vm.known_heavy_apps
    : (maintenancePlaybook?.freeze_prone_apps || []).map(a => ({
        id: a.id, label: a.label, installed: null, running_count: 0, remediation: a.remediation,
      }));
  if (!apps.length) return '';
  const erp = vm.erp_server;
  const erpIp = erp?.ip || companyProfile?.erp_server_ip || '10.0.0.50';
  const erpName = companyProfile?.erp_name || 'ERP';
  const offlineMsg = companyProfile?.ui?.erp_offline_warning || 'offline — ERP pode travar';
  const freezeDesc = companyProfile?.ui?.freeze_prone_description
    || 'ERP (LAN), Fusion 360, AutoCAD e LibreOffice Draw — monitorados no registro de incidentes.';
  const erpHtml = erp ? `<p class="erp-status ${erp.reachable ? 'ok' : 'warn'}">
    Servidor ERP <strong>${esc(erpIp)}</strong>:
    ${erp.reachable ? `online${erp.latency_ms != null ? ` (${Math.round(erp.latency_ms)} ms)` : ''}` : esc(offlineMsg)}
  </p>` : '';
  return `<div class="section admin-sub-section freeze-prone-section">
    <h4>Programas que costumam travar</h4>
    <p class="section-desc">${esc(freezeDesc)}</p>
    ${erpHtml}
    <table class="data-table"><thead><tr>
      <th>Programa</th><th>Instalado</th><th>Em execução</th><th>Plano se travar</th>
    </tr></thead><tbody>${apps.map(a => `<tr>
      <td>${esc(a.label)}</td>
      <td>${a.installed == null ? '—' : a.installed ? 'Sim' : 'Não'}</td>
      <td>${a.running_count > 0 ? `${a.running_count} processo(s)` : '—'}</td>
      <td class="incident-rec-cell">${esc((a.remediation || '').slice(0, 180))}${(a.remediation || '').length > 180 ? '…' : ''}</td>
    </tr>`).join('')}</tbody></table>
  </div>`;
}

function renderAutoRepairPanel() {
  const rs = getCollector('repair_status');
  if (!rs || !Object.keys(rs).length) {
    return `<div class="section auto-repair-section">
      <h3>Reparo automático</h3>
      <p class="empty-note">Coletor repair_status ainda não disponível. Execute <code>agendar-reparo-automatico.ps1</code> como Admin e aguarde a próxima coleta.</p>
    </div>`;
  }
  const taskOk = rs.scheduled_task_installed;
  const pending = rs.has_pending;
  const summary = rs.summary || {};
  const last = rs.last_repair || {};
  const comparisons = rs.comparisons || [];
  const statusCls = taskOk ? (pending ? 'warn' : 'ok') : 'bad';
  const statusText = taskOk
    ? (pending ? 'Tarefa ativa — reparo na fila' : 'Tarefa oculta ativa (BelarcInventoryRepair)')
    : 'Tarefa não instalada — rode agendar-reparo-automatico.ps1';
  let compRows = '';
  if (comparisons.length) {
    compRows = `<table class="data-table repair-comparison-table"><thead><tr>
      <th>Data</th><th>Perfil</th><th>Melhorou?</th><th>Disco min Δ</th><th>% Disco tempo Δ</th><th>Motivos</th>
    </tr></thead><tbody>${[...comparisons].reverse().slice(0, 10).map(c => {
      const dt = c.finished_at ? formatDate(c.finished_at) : '—';
      const improved = c.improved ? '<span class="pill ok">Sim</span>' : '<span class="pill warn">Não</span>';
      const diskDelta = c.delta?.disk_free_min_delta != null ? `${c.delta.disk_free_min_delta > 0 ? '+' : ''}${c.delta.disk_free_min_delta}%` : '—';
      const timeDelta = c.delta?.disk_time_delta != null ? `${c.delta.disk_time_delta > 0 ? '+' : ''}${c.delta.disk_time_delta}%` : '—';
      const reasons = (c.reasons || []).join(', ') || '—';
      return `<tr><td>${esc(dt)}</td><td>${esc(c.profile || '—')}</td><td>${improved}</td><td>${esc(String(diskDelta))}</td><td>${esc(String(timeDelta))}</td><td class="incident-rec-cell">${esc(reasons)}</td></tr>`;
    }).join('')}</tbody></table>`;
  } else {
    compRows = '<p class="empty-note">Nenhuma execução de reparo registrada ainda. Quando houver erro crítico, o agente enfileira e a tarefa oculta executa limpeza/reparação.</p>';
  }
  const logTail = (rs.repair_log_tail || []).slice(-8);
  const logHtml = logTail.length
    ? `<pre class="repair-log-tail">${logTail.map(l => esc(l)).join('\n')}</pre>`
    : '';
  const pendingHtml = pending && rs.pending
    ? `<p class="section-desc warn-note">Fila pendente desde ${esc(rs.pending.queued_at || '—')} — motivos: ${esc((rs.pending.reasons || []).join(', ') || '—')}</p>`
    : '';
  return `<div class="section auto-repair-section">
    <h3>Reparo automático (tarefa oculta)</h3>
    <p class="section-desc">Quando erros críticos são detectados (OperationsSuite, disco 100%, apps), o agente enfileira reparo. A tarefa <code>BelarcInventoryRepair</code> executa limpeza em segundo plano e grava comparação antes/depois.</p>
    <div class="auto-repair-status ${statusCls}">
      <span class="pill ${statusCls}">${esc(statusText)}</span>
      ${rs.scheduled_task_next_run ? `<span class="muted">Próxima execução: ${esc(formatDate(rs.scheduled_task_next_run))}</span>` : ''}
    </div>
    ${pendingHtml}
    <div class="mgmt-fields-grid" style="margin:0.75rem 0">
      <div class="mgmt-field"><span class="mgmt-label">Total reparos</span><span class="mgmt-value">${summary.total_repairs ?? comparisons.length ?? 0}</span></div>
      <div class="mgmt-field"><span class="mgmt-label">Último reparo</span><span class="mgmt-value">${last.last_completed ? formatDate(last.last_completed) : '—'}</span></div>
      <div class="mgmt-field"><span class="mgmt-label">Último melhorou?</span><span class="mgmt-value">${last.improved === true ? 'Sim' : last.improved === false ? 'Não' : '—'}</span></div>
    </div>
    <h4>Logs de comparação (antes / depois)</h4>
    ${compRows}
    ${logHtml ? `<h4>Log reparo (últimas linhas)</h4>${logHtml}` : ''}
    <p class="section-desc">Arquivos locais: <code>%ProgramData%\\BelarcInventory\\repair-comparison.jsonl</code> · <code>repair-auto.log</code></p>
  </div>`;
}

function renderMaintenancePanel() {
  const pb = maintenancePlaybook;
  if (!pb) {
    return '<p class="empty-note">Carregando playbook de manutenção…</p>';
  }
  const profiles = (pb.profiles || []).map(p => `
    <div class="maint-profile-card">
      <strong>${esc(p.label)}</strong>
      <p>${esc(p.description)}</p>
      <code class="maint-cmd">${esc(p.command)}</code>
      <button type="button" class="btn-copy-cmd" data-cmd="${esc(p.command)}">Copiar comando</button>
    </div>`).join('');
  const tasks = (pb.tasks || []).slice(0, 8).map(t => `
    <tr><td>${esc(t.label)}</td><td>${esc(t.profile)}</td>
      <td><code class="maint-cmd-inline">${esc(t.command)}</code></td>
      <td>${t.requires_admin ? 'Admin' : 'Usuário'}</td></tr>`).join('');
  return `<div class="maint-profiles-grid">${profiles}</div>
    <p class="section-desc">Execute na pasta <code>belarc-inventory</code>. O script roda em segundo plano e grava log em <code>%ProgramData%\\BelarcInventory\\maintenance.log</code></p>
    <table class="data-table maint-tasks-table"><thead><tr>
      <th>Tarefa</th><th>Perfil</th><th>Comando</th><th>Privilégio</th>
    </tr></thead><tbody>${tasks}</tbody></table>`;
}

function renderPerfTopProcesses(vm) {
  const cpu = vm.perf_top_cpu || [];
  const mem = vm.perf_top_mem || [];
  if (!cpu.length && !mem.length) return '';
  let html = '<div class="section admin-sub-section"><h4>Processos no momento da coleta</h4><div class="perf-top-grid">';
  if (cpu.length) {
    html += `<div><strong>Top CPU</strong><ul class="perf-top-list">${cpu.slice(0, 5).map(p =>
      `<li>${esc(p.name)} — ${esc(p.memory_mb)} MB RAM</li>`).join('')}</ul></div>`;
  }
  if (mem.length) {
    html += `<div><strong>Top RAM</strong><ul class="perf-top-list">${mem.slice(0, 5).map(p =>
      `<li>${esc(p.name)} — ${esc(p.memory_mb)} MB</li>`).join('')}</ul></div>`;
  }
  return html + '</div></div>';
}

function renderPhysicalDisks(vm) {
  const pds = vm.physical_disks || [];
  if (!pds.length) return '';
  return `<div class="section admin-sub-section"><h4>Discos físicos</h4>
    ${dataTable(pds.slice(0, 6), [
      { label: 'Modelo', key: 'model' },
      { label: 'GB', key: 'size_gb' },
      { label: 'Tipo', render: r => r.media_type || '—' },
      { label: 'Conformidade', render: r => r.health_status || r.status || '—' },
      { label: 'Temp.', render: r => r.reliability?.temperature_celsius != null ? `${r.reliability.temperature_celsius}°C` : '—' },
      { label: 'Desgaste', render: r => r.reliability?.wear_percent != null ? `${r.reliability.wear_percent}%` : '—' },
    ])}</div>`;
}

function renderRamModules(vm) {
  const mods = vm.ram_modules || [];
  if (!mods.length) return '';
  return `<div class="section admin-sub-section"><h4>Módulos de memória</h4>
    ${dataTable(mods, [
      { label: 'Slot', key: 'locator' },
      { label: 'GB', render: r => r.capacity_gb },
      { label: 'MHz', key: 'speed_mhz' },
      { label: 'Fabricante', key: 'manufacturer' },
    ])}</div>`;
}

function renderFolderList(vm) {
  const folders = vm.folder_access || [];
  if (!folders.length) return '<p class="empty-note">Nenhuma pasta mapeada.</p>';
  return `<ul class="folder-list">${folders.map(f => `<li>${esc(f)}</li>`).join('')}</ul>`;
}

function renderAgentStatusNote(vm) {
  if (vm.status === 'online') {
    return '<p class="ok-note">Agente conectado — dados atualizados automaticamente.</p>';
  }
  return `<div class="agent-offline-banner">
    <strong>PC offline no inventário</strong> — última coleta: ${formatDate(vm.last_seen)}.
    Para manter online: instale <code>BelarcInventory.exe</code> como Administrador ou execute <code>sc query BelarcInventoryAgent</code>.
  </div>`;
}

const CATEGORY_LABELS = {
  escritorio: 'Escritório', erp: 'ERP / Sistemas', seguranca: 'Segurança', acesso: 'Acesso remoto',
  email: 'E-mail', engenharia: 'Engenharia', utilitario: 'Utilitários', design: 'Design', dev: 'Desenvolvimento',
  bancario: 'Bancário',
};

const CATEGORY_ORDER = ['bancario', 'escritorio', 'erp', 'seguranca', 'acesso', 'email', 'engenharia', 'utilitario', 'design', 'dev', 'outros'];
const APP_DISPLAY_NAMES = { affinity: 'Affinity Canva' };

const MAINTENANCE_LABELS = {
  normal: 'Normal — sem pendências',
  em_manutencao: 'Em manutenção',
  agendada: 'Manutenção agendada',
  aguardando_peca: 'Aguardando peça',
  aguardando_usuario: 'Aguardando usuário',
  substituir: 'Substituir equipamento',
};

const ALERT_CATEGORY_LABELS = {
  blacklist: 'Software não autorizado',
  antivirus: 'Antivírus',
  firewall: 'Firewall',
  certificate: 'Certificado',
  disk: 'Disco / armazenamento',
  bsod: 'Estabilidade / tela azul',
  system_errors: 'Erros do Windows',
  performance: 'Desempenho / otimização',
  updates: 'Atualizações Windows',
  connectivity: 'Conexão com servidor',
  software_policy: 'Software padrão',
  bitlocker: 'BitLocker',
  cadastro: 'Cadastro TI',
};

const ALERT_ACTIONS = {
  blacklist: 'Desinstale o programa não autorizado pelo Painel de Controle ou Configurações > Aplicativos.',
  antivirus: 'Garanta ESET ativo ou reative o Windows Defender. Reinicie o serviço eSocial/ekrn se necessário.',
  firewall: 'Ative o Firewall do Windows em todos os perfis (Domínio, Privado, Público).',
  certificate: 'Renove o certificado digital antes do vencimento (e-CNPJ, e-mail, etc.).',
  disk: 'Libere espaço: Lixeira, Downloads, %temp%, Desinstalar programas grandes, mover arquivos para NAS.',
  bsod: 'Atualize drivers de vídeo/chipset, verifique RAM com mdsched.exe e evite superaquecimento.',
  system_errors: 'Abra Visualizador de Eventos (eventvwr) e investigue erros críticos recentes.',
  performance: 'Prioridade: menos programas na inicialização, mais RAM/SSD, reinício periódico, fechar abas/apps pesados.',
  updates: 'Configurações > Windows Update > Verificar atualizações e instalar pendências.',
  connectivity: 'Verifique se o serviço BelarcInventoryAgent está rodando: sc query BelarcInventoryAgent',
};

function normalizeAlertKey(a) {
  let msg = (a.message || '').trim();
  if (a.category === 'certificate') {
    const cn = msg.match(/CN=([^,]+)/i);
    if (cn) msg = cn[1].trim();
  }
  if (a.category === 'blacklist') {
    msg = msg.replace(/.*(?:detectado|autorizado)[:\s]*/i, '').trim().toLowerCase();
  }
  if (a.category === 'bsod' && msg.includes('desligamento')) {
    msg = 'desligamentos_inesperados';
  }
  if (a.category === 'antivirus' && /defender/i.test(msg)) {
    msg = 'defender_status';
  }
  if (a.category === 'firewall') {
    msg = 'firewall_status';
  }
  return `${a.category}|${msg}`;
}

function dedupeAlerts(list) {
  const seen = new Set();
  const out = [];
  for (const a of list) {
    const k = `${a.machine_id || ''}|${normalizeAlertKey(a)}`;
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(a);
  }
  return out.sort((x, y) => {
    const ord = { critical: 0, warning: 1, info: 2 };
    const sx = ord[x.severity] ?? 3;
    const sy = ord[y.severity] ?? 3;
    return sx - sy || (x.category || '').localeCompare(y.category || '');
  });
}

function alertActionHint(category) {
  return ALERT_ACTIONS[category] || 'Corrija conforme orientação da TI.';
}

function getPerformanceRecommendations(vm, pcAlerts) {
  const recs = [];
  const cats = new Set(pcAlerts.map(a => a.category));
  const msgs = pcAlerts.map(a => a.message.toLowerCase()).join(' ');

  if (cats.has('performance') || msgs.includes('inicialização') || msgs.includes('startup')) {
    recs.push('Reduzir programas na inicialização — ganho imediato no tempo de boot.');
  }
  if (cats.has('disk') || msgs.includes('livre') || msgs.includes('espaço')) {
    recs.push('Liberar espaço em disco C: — melhora atualizações, swap e velocidade geral.');
  }
  if (msgs.includes('ram') || msgs.includes('hdd') || msgs.includes('ssd')) {
    recs.push('Avaliar upgrade de RAM (16 GB) ou SSD se ainda usa disco mecânico.');
  }
  if (cats.has('bsod') || cats.has('system_errors') || vm.has_recent_bsod) {
    recs.push('Estabilidade: reiniciar o PC, atualizar drivers e checar temperatura/energia.');
  }
  if (cats.has('updates')) {
    recs.push('Instalar atualizações Windows pendentes — correções de segurança e desempenho.');
  }
  if (cats.has('blacklist')) {
    recs.push('Remover software não autorizado — reduz risco e uso de rede em segundo plano.');
  }
  if (!vm.eset_installed) {
    recs.push('Instalar/reativar ESET corporativo — proteção sem depender do Defender.');
  }
  if ((vm.uptime_seconds || 0) > 21 * 86400) {
    recs.push('Agendar reinício fora do expediente — libera memória e finaliza atualizações.');
  }
  if (vm.erp_server && vm.erp_server.reachable === false) {
    const erpIp = companyProfile?.erp_server_ip || vm.erp_server.ip || '10.0.0.50';
    const erpName = companyProfile?.erp_name || 'ERP';
    recs.push(`Servidor ${erpName} (${erpIp}) inalcançável — verificar rede antes de usar o ERP.`);
  }
  if ((vm.perf_peak_disk || vm.perf_disk_time || 0) >= 90) {
    recs.push('SSD/HDD em 100% na coleta — verificar antivírus, backup, indexação e processos de I/O (causa delay no OperationsSuite).');
  }
  if ((vm.critical_app_events_today || 0) > 0) {
    recs.push(`Travamentos de apps críticos hoje (${vm.critical_app_events_today}) — consulte a linha do tempo e logs abaixo.`);
  } else if ((vm.critical_app_events_count || 0) > 0) {
    recs.push('Apps críticos com falhas nos últimos 14 dias — revisar logs OperationsSuite/CAD/LibreOffice na seção abaixo.');
  }
  if ((vm.disk_errors_count || 0) > 0) {
    recs.push('Erros de disco no Event Viewer — backup imediato e verificar SMART do SSD.');
  }
  const heavyIssues = (currentIncidents || []).filter(i =>
    ['known_app_crash', 'known_app_hang', 'erp_unreachable', 'disk_saturation', 'disk_error'].includes(i.category));
  if (heavyIssues.length) {
    recs.push('Programas críticos com falhas recentes — execute .\\executar-manutencao.ps1 -Perfil AppsCriticos');
  }
  if ((vm.known_heavy_apps || []).some(a => a.running_count > 0 && (a.processes || []).some(p => (p.memory_mb || 0) > 800))) {
    const critical = companyProfile?.ui?.critical_apps_summary || 'ERP/Fusion/AutoCAD/Libre';
    recs.push(`${critical} usando muita RAM — fechar abas/projetos ou reiniciar o programa.`);
  }
  if (!recs.length && pcAlerts.length) {
    recs.push('Revise cada alerta abaixo e aplique a ação sugerida na coluna de categorias.');
  }
  if (!recs.length) {
    recs.push('PC em boa forma — manter reinícios mensais e Windows Update em dia.');
  }
  return [...new Set(recs)];
}

function healthScoreExplain(score, breakdown) {
  if (score == null || score === '') return 'Aguardando coleta para calcular a pontuação.';
  const band = breakdown?.band || scoreBand(score);
  const hints = {
    Excelente: 'PC em conformidade com as políticas corporativas.',
    Bom: 'Situação estável — pequenos ajustes opcionais.',
    Atenção: 'Revisar alertas e plano de correção.',
    Problema: 'Correções prioritárias necessárias.',
    Crítico: 'Vários problemas detectados. Priorize a correção deste PC.',
  };
  const base = hints[band] || 'Score calculado pelo servidor com base nos coletores.';
  const notes = (breakdown?.notes || []).filter(Boolean);
  if (score < 75 && notes.length) {
    const focus = notes.slice(0, 2).join('; ');
    return `${band} — ${focus}. ${base}`;
  }
  return `${band} — ${base}`;
}

function classifyCertClient(cert) {
  if (cert.cert_class) {
    const map = { corporate: 'corporate', root_historical: 'root', self_signed: 'self_signed', other: 'other' };
    return map[cert.cert_class] || cert.cert_class;
  }
  const store = (cert.store || '');
  const subject = (cert.subject || '').toUpperCase();
  const issuer = (cert.issuer || '').toUpperCase();
  const hasPk = !!cert.has_private_key;
  if (store.includes('\\Root') && !hasPk) return 'root';
  if (hasPk && (subject.includes('ICP-BRASIL') || subject.includes('E-CNPJ') || subject.includes('RFB') || /CN=[^:]+:\d{11,}/.test(subject))) return 'corporate';
  if (hasPk && (subject.includes('PROJETO') || subject.includes('DESKTOP') || issuer === subject)) return 'self_signed';
  if (store.includes('\\My')) return 'other';
  return 'root';
}

function renderCertInventoryTable() {
  const certs = getCollector('certificates');
  const all = [...(certs.expired || []), ...(certs.expiring_soon || []), ...(certs.certificates || [])];
  const seen = new Set();
  const unique = all.filter(c => {
    const k = c.thumbprint || c.subject;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  }).slice(0, 80);
  if (!unique.length) return '<p class="empty-note">Nenhum certificado coletado.</p>';
  return `<table class="data-table cert-table"><thead><tr><th>Classe</th><th>Nome</th><th>Validade</th><th>Store</th></tr></thead><tbody>
    ${unique.map(c => {
      const cls = classifyCertClient(c);
      const badge = cls === 'corporate' ? 'corp' : cls === 'self_signed' ? 'self' : cls === 'root' ? 'root' : 'other';
      const lbl = cls === 'corporate' ? 'Corporativo' : cls === 'self_signed' ? 'Autoassinado' : cls === 'root' ? 'Raiz/Histórico' : 'Outro';
      const days = c.days_left;
      const valid = days != null ? (days < 0 ? `Expirado (${-days}d)` : `${days}d`) : '—';
      const name = (c.subject || '').replace(/^CN=/i, '').split(',')[0].slice(0, 48);
      return `<tr class="cert-${badge}"><td><span class="cert-badge ${badge}">${lbl}</span></td><td>${esc(name)}</td><td>${esc(valid)}</td><td>${esc((c.store || '').replace('Cert:\\', ''))}</td></tr>`;
    }).join('')}
  </tbody></table>
  <p class="section-desc">Certificados raiz expirados (Baltimore, Microsoft Root…) são inventário — não afetam o score.</p>`;
}

function renderScoreBreakdown(bd) {
  if (!bd) return '';
  const rows = [
    ['Disco SMART', bd.smart_disk, 30],
    ['Segurança', bd.security ?? bd.antivirus, 25],
    ['Windows Update', bd.windows_update, 15],
    ['Eventos críticos', bd.critical_events, 10],
    ['Espaço em disco', bd.disk_free, 10],
    ['Temperaturas', bd.temperature ?? 4, 5],
    ['Certificados corp.', bd.certificates, 5],
  ];
  const band = bd.band || scoreBand(bd.total);
  return `<div class="section admin-score-section">
    <h3>Conformidade TI — ${bd.total}% <span class="score-band-label ${scoreClass(bd.total)}">${esc(band)}</span></h3>
    <p class="section-desc">Score corporativo — SMART, segurança, updates, eventos, disco, temperatura e certificados. Inventário e alertas são camadas separadas.</p>
    <table class="data-table score-breakdown-table"><thead><tr><th>Categoria</th><th>Pontos</th><th>Máx.</th></tr></thead>
    <tbody>${rows.map(([lbl, pts, max]) => {
      const pct = max ? Math.round((pts / max) * 100) : 0;
      const cls = pct >= 80 ? 'ok' : pct >= 50 ? 'warn' : 'bad';
      return `<tr class="${cls}"><td>${esc(lbl)}</td><td>${pts}</td><td>${max}</td></tr>`;
    }).join('')}</tbody></table>
    ${(bd.notes || []).length ? `<ul class="score-notes">${bd.notes.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : ''}
  </div>`;
}

function alertSeverityLabel(sev) {
  const map = { critical: 'Crítico', warning: 'Atenção', info: 'Informação' };
  return map[sev] || sev;
}

function alertSeverityClass(sev) {
  if (sev === 'critical') return 'bad';
  if (sev === 'warning') return 'warn';
  return 'muted';
}

function filterStaleAlerts(list, vm) {
  if (!vm) return list;
  return list.filter(a => {
    if (a.category === 'antivirus' && /defender/i.test(a.message || '') && vm.eset_installed) {
      return false;
    }
    return true;
  });
}

function getMachineAlerts(machineId, vm) {
  const list = dedupeAlerts(alerts.filter(a => a.machine_id === machineId));
  return filterStaleAlerts(list, vm);
}

function alertCategoryIcon(category) {
  const icons = {
    blacklist: '🚫',
    antivirus: '🛡',
    firewall: '🔥',
    certificate: '📜',
    disk: '💾',
    bsod: '💥',
    system_errors: '⚠',
    performance: '⚡',
    updates: '🔄',
    connectivity: '📡',
    software_policy: '📦',
    bitlocker: '🔐',
    cadastro: '📋',
  };
  return icons[category] || '•';
}

function renderDrawerAlertCard(a) {
  const cat = ALERT_CATEGORY_LABELS[a.category] || a.category;
  const action = alertActionHint(a.category);
  const icon = alertCategoryIcon(a.category);
  return `<article class="fleet-alert-card sev-${a.severity}">
    <div class="fleet-alert-card-head">
      <div class="fleet-alert-badges">
        <span class="fleet-alert-sev sev-${a.severity}">${esc(alertSeverityLabel(a.severity))}</span>
        <span class="fleet-alert-cat"><span class="fleet-alert-cat-icon" aria-hidden="true">${icon}</span>${esc(cat)}</span>
      </div>
      <button type="button" class="fleet-alert-open-pc" data-open-machine="${esc(a.machine_id)}" title="Abrir detalhes do PC">
        ${esc(a.hostname)} <span class="fleet-alert-open-arrow">→</span>
      </button>
    </div>
    <p class="fleet-alert-message">${esc(a.message)}</p>
    <div class="fleet-alert-action">
      <span class="fleet-alert-action-label">Ação sugerida</span>
      <p>${esc(action)}</p>
    </div>
  </article>`;
}

function renderAlertListItem(a, compact = false) {
  const cat = ALERT_CATEGORY_LABELS[a.category] || a.category;
  const action = alertActionHint(a.category);
  return `<li class="alert-item sev-${a.severity}${compact ? ' compact' : ''}">
    <span class="alert-sev">${esc(alertSeverityLabel(a.severity))}</span>
    <span class="alert-cat">${esc(cat)}</span>
    <div class="alert-body">
      <span class="alert-msg">${esc(a.message)}</span>
      ${compact ? `<span class="alert-action-hint">${esc(action)}</span>` : ''}
    </div>
    ${compact ? '' : `<span class="alert-host">${esc(a.hostname)}</span>`}
  </li>`;
}

let drawerSevFilter = 'all';
let drawerCatFilter = 'all';
let drawerSearchQuery = '';

function countAlertsBySeverity(list) {
  return {
    critical: list.filter(a => a.severity === 'critical').length,
    warning: list.filter(a => a.severity === 'warning').length,
    info: list.filter(a => a.severity === 'info').length,
  };
}

function filterDrawerAlerts(list) {
  let out = dedupeAlerts(list);
  if (drawerSevFilter !== 'all') out = out.filter(a => a.severity === drawerSevFilter);
  if (drawerCatFilter !== 'all') out = out.filter(a => a.category === drawerCatFilter);
  const q = drawerSearchQuery.toLowerCase().trim();
  if (q) {
    out = out.filter(a => {
      const hay = [
        a.hostname,
        a.message,
        a.category,
        ALERT_CATEGORY_LABELS[a.category],
        alertActionHint(a.category),
      ].filter(Boolean).join(' ').toLowerCase();
      return hay.includes(q);
    });
  }
  return out;
}

function renderAlertsDrawerSummary(allList, filteredList) {
  const totalEl = document.getElementById('alerts-drawer-total');
  const summaryEl = document.getElementById('alerts-drawer-summary');
  if (!summaryEl) return;
  const counts = countAlertsBySeverity(allList);
  if (totalEl) {
    totalEl.textContent = allList.length ? `${allList.length} alerta${allList.length !== 1 ? 's' : ''}` : 'Nenhum';
    totalEl.classList.toggle('has-alerts', allList.length > 0);
  }
  summaryEl.innerHTML = `
    <button type="button" class="alerts-stat-chip critical${drawerSevFilter === 'critical' ? ' active' : ''}" data-sev="critical">
      <span class="alerts-stat-num">${counts.critical}</span>
      <span class="alerts-stat-lbl">Crítico</span>
    </button>
    <button type="button" class="alerts-stat-chip warning${drawerSevFilter === 'warning' ? ' active' : ''}" data-sev="warning">
      <span class="alerts-stat-num">${counts.warning}</span>
      <span class="alerts-stat-lbl">Atenção</span>
    </button>
    <button type="button" class="alerts-stat-chip info${drawerSevFilter === 'info' ? ' active' : ''}" data-sev="info">
      <span class="alerts-stat-num">${counts.info}</span>
      <span class="alerts-stat-lbl">Info</span>
    </button>
    <div class="alerts-stat-chip muted static">
      <span class="alerts-stat-num">${filteredList.length}</span>
      <span class="alerts-stat-lbl">Exibindo</span>
    </div>`;
  summaryEl.querySelectorAll('.alerts-stat-chip:not(.static)').forEach(btn => {
    btn.onclick = () => {
      drawerSevFilter = drawerSevFilter === btn.dataset.sev ? 'all' : btn.dataset.sev;
      syncAlertsDrawerFiltersUi();
      renderAlertsDrawer();
    };
  });
}

function syncAlertsDrawerFiltersUi() {
  document.querySelectorAll('#alerts-sev-pills .alerts-pill').forEach(p => {
    p.classList.toggle('active', p.dataset.sev === drawerSevFilter);
  });
  const catSel = document.getElementById('alerts-filter-category');
  if (catSel) catSel.value = drawerCatFilter;
  const searchEl = document.getElementById('alerts-filter-search');
  if (searchEl && searchEl.value !== drawerSearchQuery) searchEl.value = drawerSearchQuery;
}

function renderAlertsDrawer() {
  const body = document.getElementById('alerts-drawer-body');
  const allList = dedupeAlerts(alerts);
  const list = filterDrawerAlerts(alerts);
  renderAlertsDrawerSummary(allList, list);
  syncAlertsDrawerFiltersUi();
  if (!list.length) {
    body.innerHTML = `<div class="fleet-alerts-empty">
      <div class="fleet-alerts-empty-icon">✓</div>
      <p>${allList.length ? 'Nenhum alerta corresponde aos filtros.' : 'Nenhum alerta ativo na frota.'}</p>
      ${allList.length ? '<button type="button" class="btn secondary alerts-clear-filters" id="alerts-clear-filters">Limpar filtros</button>' : ''}
    </div>`;
    document.getElementById('alerts-clear-filters')?.addEventListener('click', () => {
      drawerSevFilter = 'all';
      drawerCatFilter = 'all';
      drawerSearchQuery = '';
      renderAlertsDrawer();
    });
    return;
  }
  body.innerHTML = `<div class="fleet-alerts-list">${list.map(a => renderDrawerAlertCard(a)).join('')}</div>`;
  body.querySelectorAll('[data-open-machine]').forEach(btn => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.openMachine;
      if (id) {
        toggleAlertsDrawer(false);
        selectMachine(id);
      }
    });
  });
}

function initAlertsDrawerFilters() {
  const catSel = document.getElementById('alerts-filter-category');
  if (!catSel) return;
  const cats = [...new Set(alerts.map(a => a.category).filter(Boolean))].sort();
  catSel.innerHTML = '<option value="all">Todas categorias</option>'
    + cats.map(c => `<option value="${esc(c)}">${esc(ALERT_CATEGORY_LABELS[c] || c)}</option>`).join('');
  catSel.value = drawerCatFilter;
}

function toggleAlertsDrawer(show) {
  const drawer = document.getElementById('alerts-drawer');
  const open = show ?? drawer.classList.contains('hidden');
  drawer.classList.toggle('hidden', !open);
  drawer.setAttribute('aria-hidden', open ? 'false' : 'true');
  document.body.classList.toggle('alerts-modal-open', open);
  if (open) {
    initAlertsDrawerFilters();
    renderAlertsDrawer();
  }
}

function getBankingApp(vm) {
  const id = bankingAppId();
  return (vm.standard_apps || []).find(a => a.id === id) || { installed: vm.banking_app_installed, version: vm.banking_app_version };
}

function banking_appStatusText(banking_app) {
  if (!banking_app?.installed) return 'Não instalado';
  return banking_app.version ? `Instalado v${banking_app.version}` : 'Instalado';
}

function appDisplayName(id, fallback) {
  if (id === bankingAppId()) return bankingAppLabel();
  return APP_DISPLAY_NAMES[id] || fallback || id;
}

function appCardSubtext(a) {
  if (!a.installed) return '';
  if (a.version) return `v${a.version}`;
  const d = a.detail || '';
  if (!d || d.includes(':\\') || d.includes('/') || d.length > 40) return '';
  return d;
}

function shortenOfficeName(product) {
  if (!product) return 'Microsoft Office';
  const p = product.toLowerCase();
  if (p.includes('professional') && p.includes('2019')) return 'Office 2019 Professional';
  if (p.includes('office 19') || p.includes('office19')) return 'Office 2019';
  if (p.includes('excel')) return 'Microsoft Excel';
  if (p.includes('word')) return 'Microsoft Word';
  if (product.includes(',')) return product.split(',')[0].trim();
  if (product.length > 36) return product.slice(0, 34) + '…';
  return product;
}

function formatKeyPartial(key) {
  if (!key) return '—';
  return key.length <= 5 ? `***-${key}` : key;
}

function renderEsetCard(eset) {
  if (!eset?.installed) {
    return `<div class="eset-card missing"><div class="eset-icon">🛡</div><div><div class="eset-title">ESET não detectado</div><div class="eset-sub">Antivírus corporativo ausente neste PC</div></div></div>`;
  }
  return `<div class="eset-card installed">
    <div class="eset-icon">🛡</div>
    <div class="eset-body">
      <div class="eset-title">${esc(eset.product_name || 'ESET Endpoint Security')}</div>
      <div class="eset-meta">
        ${eset.version ? `<span>Versão ${esc(eset.version)}</span>` : ''}
        ${eset.agent_version ? `<span>Agent ${esc(eset.agent_version)}</span>` : ''}
        ${eset.service_running ? '<span class="pill ok">Serviço ativo</span>' : '<span class="pill warn">Serviço parado</span>'}
        ${eset.real_time_active === true ? '<span class="pill ok">Tempo real</span>' : ''}
      </div>
      ${eset.install_path ? `<div class="eset-path">${esc(eset.install_path)}</div>` : ''}
    </div>
  </div>`;
}

function renderStandardAppsGrid(apps) {
  if (!apps?.length) return '<p class="empty-note">Nenhum programa padrão configurado.</p>';
  const installed = apps.filter(a => a.installed).length;
  const byCat = {};
  apps.forEach(a => {
    const cat = a.category || 'outros';
    (byCat[cat] = byCat[cat] || []).push(a);
  });
  const sortedCats = Object.keys(byCat).sort((a, b) => {
    const ia = CATEGORY_ORDER.indexOf(a);
    const ib = CATEGORY_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
  return `<div class="apps-summary-bar"><span class="apps-count">${installed} de ${apps.length} instalados</span>
    <div class="apps-progress"><div class="apps-progress-fill" style="width:${Math.round((installed / apps.length) * 100)}%"></div></div></div>
    ${sortedCats.map(cat => {
      const items = byCat[cat];
      return `
      <div class="apps-category">
        <div class="apps-category-title">${esc(CATEGORY_LABELS[cat] || cat)}</div>
        <div class="apps-grid">${items.map(a => `
          <div class="app-card ${a.installed ? 'installed' : 'missing'}" title="${esc(a.detail || '')}">
            <div class="app-card-top">
              <span class="app-status">${a.installed ? '✓' : '✗'}</span>
              <span class="app-name">${esc(APP_DISPLAY_NAMES[a.id] || a.label)}</span>
            </div>
            ${appCardSubtext(a) ? `<span class="app-ver">${esc(appCardSubtext(a))}</span>` : ''}
          </div>`).join('')}
        </div>
      </div>`;
    }).join('')}`;
}

function renderLicenseKeysCard(vm) {
  const lk = vm.license_keys || {};
  const winKey = formatKeyPartial(lk.windows_key_partial || vm.windows_key_partial);
  const cards = [{
    type: 'windows',
    icon: '⊞',
    label: 'Windows',
    sub: 'Sistema operacional',
    key: winKey,
    badge: null,
  }];

  const officeKeys = (lk.office_keys || []).filter(o => o.key_partial);
  const seen = new Set();
  for (const o of officeKeys) {
    const label = shortenOfficeName(o.product);
    const dedupe = `${label}-${o.key_partial}`;
    if (seen.has(dedupe)) continue;
    seen.add(dedupe);
    cards.push({
      type: 'office',
      icon: '◆',
      label,
      sub: 'Microsoft Office',
      key: formatKeyPartial(o.key_partial),
      badge: o.license_status && !o.license_status.startsWith('---') ? o.license_status : null,
    });
  }

  if (officeKeys.length === 0) {
    const excel = vm.standard_apps?.find(a => a.id === 'excel');
    const word = vm.standard_apps?.find(a => a.id === 'word');
    if (excel?.installed) cards.push({ type: 'office', icon: '◆', label: 'Excel', sub: 'Pacote Office', key: 'Instalado', badge: null, muted: true });
    if (word?.installed) cards.push({ type: 'office', icon: '◆', label: 'Word', sub: 'Pacote Office', key: 'Instalado', badge: null, muted: true });
  }

  return `<div class="license-keys-grid">${cards.map(c => `
    <div class="license-key-card license-key-${c.type}${c.muted ? ' muted-key' : ''}">
      <div class="license-key-top">
        <span class="license-key-icon">${c.icon}</span>
        <div class="license-key-info">
          <div class="license-key-label">${esc(c.label)}</div>
          <div class="license-key-sub">${esc(c.sub)}</div>
        </div>
        ${c.badge ? `<span class="license-key-badge">${esc(c.badge)}</span>` : ''}
      </div>
      <div class="license-key-value">${esc(c.key)}</div>
      <div class="license-key-hint">Últimos 5 caracteres da chave</div>
    </div>`).join('')}</div>`;
}

function kvGrid(pairs) {
  const items = pairs.filter(([, v]) => v != null && v !== '' && v !== '—');
  if (!items.length) return '<p class="empty-note">Sem dados disponíveis.</p>';
  return `<div class="kv-grid">${items.map(([k, v]) =>
    `<div class="kv"><div class="k">${esc(k)}</div><div class="v">${esc(v)}</div></div>`
  ).join('')}</div>`;
}

function dataTable(items, columns) {
  if (!items?.length) return '<p class="empty-note">Nenhum item detectado.</p>';
  return `<table class="data-table"><thead><tr>${columns.map(c => `<th>${esc(c.label)}</th>`).join('')}</tr></thead>
    <tbody>${items.map(row => `<tr>${columns.map(c => `<td>${esc(c.render ? c.render(row) : row[c.key] ?? '—')}</td>`).join('')}</tr>`).join('')}
    </tbody></table>`;
}

function summaryRow(label, value, cls = '') {
  const display = value == null || value === '' ? '—' : value;
  return `<div class="summary-row"><span class="label">${esc(label)}</span><span class="value ${cls}">${esc(display)}</span></div>`;
}

function renderMachineList() {
  const q = document.getElementById('search').value.toLowerCase();
  const statusFilter = document.getElementById('filter-status').value;
  const list = document.getElementById('machine-list');

  const filtered = machines.filter(m => {
    if (statusFilter !== 'all' && m.status !== statusFilter) return false;
    const hay = [m.hostname, m.lan_ip, m.tailscale_ip, m.anydesk_id, m.logged_user, m.model,
      m.admin?.owner_name, m.display_email, m.admin?.ramal, m.admin?.network_cable,
      ...(m.thunderbird_emails || [])].filter(Boolean).join(' ').toLowerCase();
    return !q || hay.includes(q);
  });

  list.innerHTML = filtered.map(m => `
    <li class="machine-item ${m.id === selectedId ? 'active' : ''}" data-id="${m.id}">
      <div class="name">
        <span class="dot ${m.status}"></span>
        ${esc(m.hostname)}
        ${m.admin?.owner_name ? `<span class="owner-tag">${esc(m.admin.owner_name)}</span>` : ''}
      </div>
      <div class="sub">
        <span>${esc(m.lan_ip || m.ip_address || 'sem IP')}</span>
        ${m.admin?.ramal ? `<span class="tag">Ramal ${esc(m.admin.ramal)}</span>` : ''}
        ${m.tailscale_connected ? '<span class="tag ts">Tailscale</span>' : (m.tailscale_installed ? '<span class="tag">TS off</span>' : '')}
        ${m.anydesk_id ? `<span class="tag ad">AD ${esc(m.anydesk_id)}</span>` : ''}
        ${getBankingApp(m).installed ? `<span class="tag banking_app">${esc(bankingAppLabel())}</span>` : `<span class="tag banking_app-off">Sem ${esc(bankingAppLabel())}</span>`}
        ${(m.thunderbird_emails || []).length ? `<span class="tag">${(m.thunderbird_emails || []).length} email(s)</span>` : ''}
        ${m.health_score != null ? `<span class="tag score-tag ${scoreClass(m.health_score)}">${m.health_score}%</span>` : ''}
        ${alertCountForMachine(m.id) ? `<span class="tag alert-tag">${alertCountForMachine(m.id)} alerta(s)</span>` : ''}
        ${m.has_recent_bsod ? '<span class="tag bsod">BSOD</span>' : ''}
      </div>
    </li>
  `).join('');

  list.querySelectorAll('.machine-item').forEach(el => {
    el.addEventListener('click', () => selectMachine(el.dataset.id));
  });
}

function renderStats() {
  document.getElementById('stat-total').textContent = machines.length;
  document.getElementById('stat-online').textContent = machines.filter(m => m.status === 'online').length;
  document.getElementById('stat-offline').textContent = machines.filter(m => m.status !== 'online').length;
  const fleetAlerts = dedupeAlerts(alerts);
  document.getElementById('stat-alerts').textContent = fleetAlerts.length;
  document.getElementById('stat-bsod').textContent = machines.filter(m => m.has_recent_bsod).length;
  const healthEl = document.getElementById('stat-health');
  if (healthEl) {
    healthEl.textContent = fleetHealth?.avg_score != null ? `${fleetHealth.avg_score}%` : '—';
    healthEl.className = `stat-val ${scoreClass(fleetHealth?.avg_score)}`;
    const wrap = document.getElementById('stat-health-wrap');
    if (wrap && fleetHealth?.bands) {
      const b = fleetHealth.bands;
      wrap.title = `Excelente ${b.excellent} · Bom ${b.good} · Atenção ${b.attention} · Problema ${b.problem} · Crítico ${b.critical}`;
    }
  }
  const btn = document.getElementById('stat-alerts-btn');
  if (btn) {
    btn.title = fleetAlerts.length
      ? `${fleetAlerts.length} alerta(s) na frota — clique para ver a lista`
      : 'Nenhum alerta ativo na frota';
  }
}

function renderSummaryGrid(vm) {
  const tsStatus = vm.tailscale_connected ? 'Conectado' : (vm.tailscale_installed ? 'Instalado / off' : 'Não instalado');
  const tsCls = vm.tailscale_connected ? 'ok' : (vm.tailscale_installed ? 'warn' : 'muted');
  const bsodText = vm.has_recent_bsod ? (vm.last_bugcheck_code || vm.last_bsod_summary || 'Detectado') : 'Nenhum';
  const bsodCls = vm.has_recent_bsod ? 'bad' : 'ok';
  const winKey = vm.windows_product_key || vm.windows_key_partial || '—';
  const emails = (vm.thunderbird_emails || []).join(', ') || '—';

  const email = vm.display_email || (vm.thunderbird_emails || []).join(', ') || '—';

  document.getElementById('summary-grid').innerHTML = `
    <div class="summary-group highlight-user">
      <div class="summary-group-title">Usuário / Cadastro TI</div>
      <div class="summary-rows">
        ${summaryRow('Responsável', vm.owner_name || '—', vm.owner_name ? 'ok' : 'muted')}
        ${summaryRow('Ramal', vm.ramal)}
        ${summaryRow('E-mail principal', email, vm.display_email ? 'ok' : 'muted')}
        ${summaryRow('Cabo de rede', vm.network_cable)}
      </div>
    </div>
    <div class="summary-group">
      <div class="summary-group-title">Rede</div>
      <div class="summary-rows">
        ${summaryRow('IP LAN', vm.lan_ip)}
        ${summaryRow('Tailscale IP', vm.tailscale_ip || '—', vm.tailscale_connected ? 'ok' : 'muted')}
        ${summaryRow('Tailscale', tsStatus, tsCls)}
      </div>
    </div>
    <div class="summary-group">
      <div class="summary-group-title">Acesso remoto</div>
      <div class="summary-rows">
        ${summaryRow('AnyDesk ID', vm.anydesk_id)}
        ${summaryRow('AnyDesk', vm.anydesk_id ? (vm.anydesk_running ? 'Ativo' : 'Instalado') : '—', vm.anydesk_running ? 'ok' : 'muted')}
        ${summaryRow('TeamViewer', vm.teamviewer_id || '—')}
      </div>
    </div>
    <div class="summary-group">
      <div class="summary-group-title">Sistema</div>
      <div class="summary-rows">
        ${summaryRow('Sistema', vm.os_version)}
        ${summaryRow('Uptime', formatUptime(vm.uptime_seconds))}
        ${summaryRow('Usuário', vm.logged_user)}
        ${summaryRow('Último BSOD', bsodText, bsodCls)}
        ${summaryRow('Chave Windows', formatKeyPartial(vm.windows_key_partial || winKey))}
        ${summaryRow('ESET', vm.eset_info?.product_name || vm.eset_product || (vm.eset_installed ? 'Instalado' : 'Não detectado'), vm.eset_installed ? 'ok' : 'muted')}
        ${summaryRow(bankingAppLabel(), banking_appStatusText(getBankingApp(vm)), getBankingApp(vm).installed ? 'ok' : 'muted')}
        ${summaryRow('Software padrão', `${vm.standard_apps_installed || 0}/${(vm.standard_apps || []).length}`)}
      </div>
    </div>`;
}

function updateDetailView(skipTabRender = false) {
  if (!currentDetail) return;
  const vm = buildViewModel();
  const m = currentDetail.machine;

  document.getElementById('empty-state').classList.add('hidden');
  document.getElementById('detail-content').classList.remove('hidden');

  document.getElementById('detail-hostname').textContent = vm.hostname || '—';
  const owner = vm.owner_name ? ` · ${vm.owner_name}` : '';
  document.getElementById('detail-meta').textContent =
    `${vm.status === 'online' ? 'Online' : 'Offline'} · ${vm.logged_user || 'sem usuário'}${owner} · Último visto ${formatDate(vm.last_seen)}`;
  const score = vm.health_score;
  document.getElementById('detail-score').textContent = score != null ? `${score}%` : '—';
  document.getElementById('detail-score').className = `score-badge ${scoreClass(score)}`;
  document.getElementById('detail-score-hint').textContent = healthScoreExplain(score, currentDetail?.score_breakdown);
  document.getElementById('detail-report').href = `/api/machines/${m.id}/report`;
  const iaBtn = document.getElementById('detail-ia-prompt');
  if (iaBtn) iaBtn.href = `/api/machines/${m.id}/compliance-prompt`;

  document.getElementById('summary-grid').classList.toggle('hidden', ['admin', 'inventory', 'alerts-tab', 'health'].includes(activeTab));
  if (activeTab !== 'admin') renderSummaryGrid(vm);

  document.querySelectorAll('.tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === activeTab);
  });
  if (!skipTabRender && !(adminFormDirty && activeTab === 'admin')) {
    renderTab(activeTab);
  }
}

async function selectMachine(id) {
  selectedId = id;
  setView('inventory');
  activeTab = 'admin';
  adminFormDirty = !!loadAdminDraft();
  renderMachineList();
  const [detail, incs, playbook] = await Promise.all([
    fetchJson(`/api/machines/${id}`),
    fetchJson(`/api/machines/${id}/incidents`).catch(() => []),
    fetchJson('/api/maintenance/playbook').catch(() => null),
  ]);
  currentDetail = detail;
  currentIncidents = incs;
  maintenancePlaybook = playbook;
  updateDetailView();
}

async function refreshDashboard() {
  const prevId = selectedId;
  const prevTab = activeTab;
  try {
    await ensureCompanyProfile();
    const [dash, alertList, fleet] = await Promise.all([
      fetchJson('/api/dashboard'),
      fetchJson('/api/alerts?unresolved=true'),
      fetchJson('/api/fleet/health').catch(() => null),
    ]);
    machines = dash;
    alerts = alertList;
    fleetHealth = fleet;
    renderStats();
    initAlertsDrawerFilters();
    renderMachineList();
    if (currentView === 'management') {
      renderManagementReport();
    }
    if (prevId && machines.find(m => m.id === prevId) && currentView === 'inventory') {
      selectedId = prevId;
      activeTab = prevTab;
      const [detail, incs, playbook] = await Promise.all([
        fetchJson(`/api/machines/${prevId}`),
        fetchJson(`/api/machines/${prevId}/incidents`).catch(() => []),
        fetchJson('/api/maintenance/playbook').catch(() => maintenancePlaybook),
      ]);
      currentDetail = detail;
      currentIncidents = incs;
      if (playbook) maintenancePlaybook = playbook;
      if (adminFormDirty && prevTab === 'admin') {
        if (currentDetail.admin) Object.assign(currentDetail.admin, loadAdminDraft() || {});
        updateDetailView(true);
      } else {
        updateDetailView();
      }
    }
  } catch (e) {
    console.error('refresh failed:', e);
  }
}

function renderTab(tab) {
  activeTab = tab;
  document.querySelectorAll('.tab').forEach(t => t.classList.toggle('active', t.dataset.tab === tab));
  const panels = document.getElementById('tab-panels');
  const vm = buildViewModel();
  const m = currentDetail.machine;

  if (tab === 'admin') {
    document.getElementById('summary-grid').classList.add('hidden');
    const draft = loadAdminDraft();
    const a = { ...(vm.admin || {}), ...(adminFormDirty && draft ? draft : {}) };
    const maintStatus = a.maintenance_status || 'normal';
    const maintOptions = Object.entries(MAINTENANCE_LABELS).map(([k, lbl]) =>
      `<option value="${k}"${maintStatus === k ? ' selected' : ''}>${esc(lbl)}</option>`
    ).join('');

    panels.innerHTML = `
      <div class="admin-home layer-cadastro">
        <p class="layer-banner">Cadastro manual — não altera score nem inventário automático.</p>
        ${renderAgentStatusNote(vm)}
        <form id="admin-form" class="admin-form">
          <div class="section admin-form-section">
            <h3>Cadastro do usuário</h3>
            <p class="section-desc">Dados fixos do responsável por este computador.</p>
            <div class="form-grid">
              <label>Nome do responsável<input name="owner_name" value="${esc(a.owner_name || '')}" placeholder="Ex: João Silva" /></label>
              <label>Ramal<input name="ramal" value="${esc(a.ramal || '')}" placeholder="Ex: 2101" /></label>
              <label>E-mail principal<input name="primary_email" type="email" value="${esc(a.primary_email || '')}" placeholder="${esc((vm.thunderbird_emails || [])[0] || 'email@empresa.com.br')}" /></label>
              <label>Cabo de rede<input name="network_cable" value="${esc(a.network_cable || '')}" placeholder="Ex: CAB-001 / porta 12" /></label>
            </div>
          </div>

          <div class="section admin-form-section">
            <h3>Acessos e referências</h3>
            <p class="section-desc">O Belarc Inventory registra usuários e referências operacionais. Senhas não são armazenadas.</p>
            <div class="form-grid">
              <label>${esc(companyProfile?.ui?.erp_user || 'ERP — usuário')}<input name="cybersul_user" value="${esc(a.cybersul_user || '')}" placeholder="Login ERP" /></label>
              <label>${esc(companyProfile?.ui?.erp_access_reference || 'Referência de acesso')}<input name="notes" value="${esc(a.notes || '')}" placeholder="Ex.: item no cofre de senhas ou procedimento interno" /></label>
              <label>Outro usuário técnico<input name="nas_user" value="${esc(a.nas_user || '')}" placeholder="Usuário de serviço, se aplicável" /></label>
            </div>
          </div>

          <div class="section admin-form-section maintenance-section">
            <h3>Manutenção</h3>
            <p class="section-desc">Controle de chamados, troca de peças e intervenções da TI.</p>
            <div class="form-grid">
              <label class="full-width">Situação
                <select name="maintenance_status">${maintOptions}</select>
              </label>
              <label class="full-width">Detalhes da manutenção
                <textarea name="maintenance_notes" rows="3" placeholder="Ex: Troca de SSD agendada para 15/06, técnico João...">${esc(a.maintenance_notes || '')}</textarea>
              </label>
            </div>
          </div>

          <div class="section admin-form-section">
            <h3>Portal de chamados</h3>
            <p class="section-desc">Todo PC pode abrir chamado para qualquer setor. Marque abaixo os setores cujos chamados este PC deve receber; não libera Inventário, Diretório ou Dashboard BI.</p>
            <div class="form-grid">
              <fieldset class="full-width"><legend>Este PC recebe chamados destinados a</legend>
                ${['ti','desenho','projeto','producao'].map(d => `<label><input type="checkbox" name="ticket_receive_departments" value="${d}"${(a.ticket_receive_departments || []).includes(d) ? ' checked' : ''}> ${esc({ti:'TI',desenho:'Desenho',projeto:'Projeto',producao:'Produção'}[d])}</label>`).join('')}
              </fieldset>
            </div>
            <p class="section-desc">No cliente simples haverá “Novo chamado”, “Meus chamados” e “Chamados recebidos”. Os marcados acima recebem notificações e aparecem na aba Recebidos.</p>
          </div>

          <div class="section admin-form-section">
            <h3>Observações e comentários</h3>
            <p class="section-desc">Informações gerais visíveis no relatório e comentários internos da equipe TI.</p>
            <label class="full-width">Observações gerais
              <textarea name="notes" rows="3" placeholder="Certificados digitais, impressora padrão, particularidades do usuário...">${esc(a.notes || '')}</textarea>
            </label>
            <label class="full-width">Comentários internos TI
              <textarea name="ti_comments" rows="3" placeholder="Anotações da equipe: histórico de suporte, decisões, próximos passos...">${esc(a.ti_comments || '')}</textarea>
            </label>
          </div>

          <div class="form-actions sticky-actions">
            <button type="submit" class="btn">Salvar cadastro</button>
            <span id="admin-save-status" class="save-status"></span>
            ${a.updated_at ? `<span class="save-hint">Última alteração: ${formatDate(a.updated_at)}</span>` : ''}
          </div>
        </form>
      </div>`;
    const form = document.getElementById('admin-form');
    form.onsubmit = saveAdmin;
    form.addEventListener('input', () => {
      adminFormDirty = true;
      saveAdminDraft();
    });
    form.addEventListener('change', () => {
      adminFormDirty = true;
      saveAdminDraft();
    });
  } else if (tab === 'inventory') {
    document.getElementById('summary-grid').classList.add('hidden');
    const id = getCollector('identity');
    const hw = getCollector('hardware');
    const rt = getCollector('runtime');
    const sys = hw.system || {};
    panels.innerHTML = `
      <div class="layer-banner layer-inventory">Inventário — dados para consulta. Não altera o score de saúde.</div>
      <div class="section"><h3>Identificação</h3>${kvGrid([
        ['Hostname', vm.hostname], ['Serial', vm.serial || id.serial], ['MAC', vm.mac_primary],
        ['Usuário', vm.logged_user], ['IP', vm.lan_ip], ['Modelo', vm.model],
        ['Fabricante', sys.manufacturer], ['Sistema', vm.os_version], ['Build', vm.os_build],
        ['Uptime', formatUptime(vm.uptime_seconds)], ['Último visto', formatDate(vm.last_seen)],
      ])}</div>
      <div class="section"><h3>Hardware resumido</h3>${kvGrid([
        ['CPU', vm.cpu_info?.name], ['RAM', vm.ram_total_gb ? `${vm.ram_total_gb} GB` : null],
        ['GPU', vm.gpu_info?.name], ['Monitores', (hw.monitors || []).length || null],
        ['TPM', hw.tpm?.present ? `v${hw.tpm.version}` : 'Não'], ['Secure Boot', hw.secure_boot?.enabled != null ? (hw.secure_boot.enabled ? 'Ativo' : 'Inativo') : null],
      ])}</div>
      <div class="section"><h3>Software &amp; rede</h3>${kvGrid([
        ['Programas', vm.program_count], ['Software padrão', `${vm.standard_apps_installed}/${(vm.standard_apps || []).length}`],
        ['ESET', vm.eset_info?.product_name], ['Tailscale', vm.tailscale_connected ? 'Conectado' : (vm.tailscale_installed ? 'Off' : 'Não')],
        ['AnyDesk', vm.anydesk_id], ['Pastas SMB', (vm.folder_access || []).length],
      ])}${(vm.folder_access || []).length ? renderFolderList(vm) : ''}</div>
      <div class="section"><h3>Runtime</h3>${kvGrid([
        ['Docker', rt.docker?.installed ? (rt.docker.running ? `Ativo (${rt.docker.containers} containers)` : 'Parado') : 'Não'],
        ['WSL', rt.wsl?.installed ? `${(rt.wsl.distros || []).length} distro(s)` : 'Não'],
        ['Hyper-V', rt.hyperv?.enabled ? `${rt.hyperv.vms} VM(s)` : 'Desligado'],
        ['Ollama', rt.ollama?.installed ? (rt.ollama.running ? 'Rodando' : 'Parado') : 'Não'],
      ])}</div>
      <div class="section"><h3>Certificados (${getCollector('certificates').total_count || '—'} total)</h3>
        ${renderCertInventoryTable()}
      </div>`;
  } else if (tab === 'alerts-tab') {
    document.getElementById('summary-grid').classList.add('hidden');
    const pcAlerts = getMachineAlerts(m.id, vm);
    const byCat = {};
    pcAlerts.forEach(a => { (byCat[a.category] ||= []).push(a); });
    panels.innerHTML = `
      <div class="layer-banner layer-alerts">Alertas — avisos da TI. Geram atenção mas não alteram o score de conformidade.</div>
      <div class="section admin-alerts-section">
        <h3>Alertas ativos (${pcAlerts.length})</h3>
        ${pcAlerts.length
          ? `<ul class="alert-list">${pcAlerts.map(a => renderAlertListItem(a, true)).join('')}</ul>`
          : '<p class="empty-note ok-note">Nenhum alerta ativo para este PC.</p>'}
      </div>
      ${Object.keys(byCat).length ? `<div class="section"><h3>Por categoria</h3>${Object.entries(byCat).map(([cat, items]) =>
        `<p><strong>${esc(ALERT_CATEGORY_LABELS[cat] || cat)}</strong>: ${items.length}</p>`).join('')}</div>` : ''}`;
  } else if (tab === 'health') {
    document.getElementById('summary-grid').classList.add('hidden');
    const pcAlerts = getMachineAlerts(m.id, vm);
    const perfRecs = getPerformanceRecommendations(vm, pcAlerts);
    panels.innerHTML = `
      <div class="layer-banner layer-health">Conformidade TI — score corporativo (SMART 30%, segurança 25%, updates 15%, eventos 10%, disco 10%, temp. 5%, cert. corp. 5%)</div>
      ${renderScoreBreakdown(currentDetail?.score_breakdown)}
      ${renderDailyTimeline(vm)}
      <div class="section admin-metrics-section"><h3>Indicadores</h3>${renderPerfMetrics(vm)}</div>
      ${renderDiskIoSamples(vm)}
      ${renderTemperatureLog(vm)}
      ${renderCriticalAppEventsLog(vm)}
      ${renderReliabilityLog(vm)}
      <div class="section admin-disk-section"><h3>Armazenamento</h3>${renderDiskTable(vm)}${renderPhysicalDisks(vm)}</div>
      <div class="section admin-startup-section"><h3>Inicialização</h3>${renderStartupTable(vm)}</div>
      ${renderRamModules(vm)}
      <div class="section admin-perf-section"><h3>Recomendações TI</h3>
        <ul class="perf-rec-list">${perfRecs.map(r => `<li class="perf-rec-item">${esc(r)}</li>`).join('')}</ul>
      </div>
      ${renderFreezeProneApps(vm)}
      ${renderPerfTopProcesses(vm)}
      ${renderAutoRepairPanel()}
      <div class="section maintenance-panel-section">
        <h3>Manutenção e limpeza do PC</h3>
        ${renderMaintenancePanel()}
      </div>
      <div class="section incident-registry-section">
        <h3>Registro de incidentes (${currentIncidents.length})</h3>
        <p class="section-desc">Histórico de travamentos, picos de CPU/disco, temperatura e falhas — para diagnóstico e plano de ação quando o problema voltar.</p>
        ${renderIncidentRegistry(currentIncidents)}
      </div>`;
  } else if (tab === 'network-remote') {
    document.getElementById('summary-grid').classList.remove('hidden');
    renderSummaryGrid(vm);
    const net = getCollector('network');
    const ra = getCollector('remote_access');
    const ts = ra.tailscale || {};
    const ad = ra.anydesk || {};
    const tv = ra.teamviewer || {};
    panels.innerHTML = `
      <div class="section"><h3>Rede local</h3>${kvGrid([
        ['IP principal', vm.lan_ip],
        ['Todos IPs LAN', (vm.lan_ips || []).join(', ') || null],
      ])}
        ${dataTable(net.adapters || [], [
          { label: 'Interface', key: 'description' },
          { label: 'IP', render: r => (r.ips || []).join(', ') },
          { label: 'MAC', key: 'mac' },
          { label: 'DHCP', render: r => r.dhcp ? 'Sim' : 'Não' },
        ])}
      </div>
      ${(net.smb_mappings || []).length ? `<div class="section"><h3>Mapeamentos SMB</h3>${dataTable(net.smb_mappings, [
        { label: 'Local', key: 'local' }, { label: 'Remoto', key: 'remote' },
      ])}</div>` : ''}
      <div class="section"><h3>Tailscale</h3>${kvGrid([
        ['Instalado', ts.installed ? 'Sim' : 'Não'],
        ['Conectado', ts.connected ? 'Sim' : 'Não'],
        ['Estado', ts.backend_state],
        ['IP', (ts.tailscale_ips || []).join(', ') || vm.tailscale_ip],
        ['DNS', ts.dns_name || vm.tailscale_dns],
        ['Inicia com Windows', ts.startup_automatic ? 'Sim' : 'Não'],
        ['Versão', ts.version],
      ])}</div>
      <div class="section"><h3>AnyDesk</h3>${kvGrid([
        ['Instalado', ad.installed ? 'Sim' : 'Não'],
        ['ID', vm.anydesk_id],
        ['Serviço', ad.service_running ? 'Rodando' : 'Parado'],
        ['Inicia com Windows', ad.startup_automatic ? 'Sim' : 'Não'],
        ['Versão', ad.version],
      ])}</div>
      <div class="section"><h3>TeamViewer</h3>${kvGrid([
        ['Instalado', tv.installed ? 'Sim' : 'Não'],
        ['Client ID', tv.client_id],
      ])}</div>`;
  } else if (tab === 'software') {
    document.getElementById('summary-grid').classList.remove('hidden');
    renderSummaryGrid(vm);
    const sw = getCollector('software');
    const lic = getCollector('licensing');
    const win = lic.windows || {};
    panels.innerHTML = `
      <div class="section license-section"><h3>Licenças do sistema</h3>
        <p class="section-desc">Chaves parciais para identificação — exibimos apenas os 5 últimos caracteres.</p>
        ${renderLicenseKeysCard(vm)}
      </div>
      <div class="section"><h3>Antivírus ESET</h3>${renderEsetCard(vm.eset_info)}</div>
      <div class="section standard-apps-section">
        <h3>Software Padrão</h3>
        <p class="section-desc">Programas padrão da empresa — ✓ instalado · ✗ ausente.</p>
        ${renderStandardAppsGrid(vm.standard_apps)}
      </div>
      <div class="section"><h3>Todos os programas (${sw.program_count || (sw.programs || []).length})</h3>
        ${dataTable((sw.programs || []).slice(0, 100), [
          { label: 'Nome', key: 'display_name' },
          { label: 'Versão', key: 'version' },
          { label: 'Publisher', key: 'publisher' },
        ])}
      </div>
      <div class="section"><h3>Licença Windows</h3>${kvGrid([
        ['Edição', win.edition],
        ['Ativação', win.activation],
        ['Chave completa', win.product_key || vm.windows_product_key],
        ['Chave parcial', win.product_key_partial || vm.windows_key_partial],
        ['Canal', win.license_channel],
      ])}</div>
      <div class="section"><h3>Microsoft Office</h3>
        ${dataTable(lic.office || [], [
          { label: 'Produto', render: r => r.license_name || r.product_id || '—' },
          { label: 'Status', key: 'license_status' },
          { label: 'Chave parcial', key: 'key_partial' },
        ])}
      </div>`;
  } else if (tab === 'users') {
    document.getElementById('summary-grid').classList.remove('hidden');
    renderSummaryGrid(vm);
    const log = getCollector('logins');
    const em = getCollector('email');
    const tbSum = em.thunderbird_summary || {};
    const domain = log.domain_info || {};
    panels.innerHTML = `
      <div class="section"><h3>Domínio / Rede</h3>${kvGrid([
        ['Domínio', domain.domain], ['Workgroup', domain.workgroup],
        ['Membro de domínio', domain.part_of_domain != null ? (domain.part_of_domain ? 'Sim' : 'Não') : null],
        ['Servidor de logon', domain.logon_server],
      ])}</div>
      <div class="section"><h3>Contas locais</h3>
        ${dataTable(log.local_users || [], [
          { label: 'Usuário', key: 'name' },
          { label: 'Ativo', render: r => r.enabled ? 'Sim' : 'Não' },
          { label: 'Admin', render: r => r.is_admin ? 'Sim' : 'Não' },
          { label: 'Último logon', render: r => formatDate(r.last_logon) },
        ])}
      </div>
      <div class="section"><h3>Sessões ativas</h3>
        ${dataTable(log.active_sessions || [], [
          { label: 'Usuário', key: 'user' }, { label: 'Estado', key: 'state' },
          { label: 'Desde', render: r => formatDate(r.logon_time || r.start_time) },
        ])}
      </div>
      <div class="section"><h3>Thunderbird</h3>${kvGrid([
        ['Instalado', tbSum.installed ? 'Sim' : (vm.thunderbird_emails.length ? 'Sim' : 'Não')],
        ['Perfil padrão', tbSum.default_profile || vm.thunderbird_profile],
        ['E-mails', (vm.thunderbird_emails || []).join(', ')],
        ['Perfis', tbSum.profile_count],
      ])}
        ${(em.thunderbird || []).map(p => `
          <div style="margin-top:0.75rem">
            <strong>${esc(p.profile_name)}</strong>${p.is_default ? ' <span class="badge yes">Padrão</span>' : ''}
            ${kvGrid([['E-mails', (p.emails || []).join(', ')], ['Caminho', p.profile_path]])}
            ${dataTable(p.accounts || [], [
              { label: 'Conta', key: 'account_id' },
              { label: 'E-mails', render: r => Array.isArray(r.emails) ? r.emails.join(', ') : r.emails },
              { label: 'Servidor', key: 'server' },
              { label: 'Tipo', key: 'server_type' },
            ])}
          </div>`).join('')}
      </div>
      <div class="section"><h3>Outlook</h3>
        ${(em.outlook || []).length === 0 ? '<p class="empty-note">Nenhum perfil Outlook detectado.</p>' :
          (em.outlook || []).map(p => `<div style="margin-bottom:0.75rem"><strong>${esc(p.profile)}</strong>
            ${dataTable(p.accounts || [], [{ label: 'E-mail', key: 'email' }, { label: 'Nome', key: 'display_name' }])}
          </div>`).join('')}
      </div>`;
  } else if (tab === 'hardware') {
    document.getElementById('summary-grid').classList.remove('hidden');
    renderSummaryGrid(vm);
    const hw = getCollector('hardware');
    const per = getCollector('peripherals');
    const sys = hw.system || {};
    const board = hw.motherboard || {};
    const bios = hw.bios || {};
    panels.innerHTML = `
      <div class="section"><h3>Sistema</h3>${kvGrid([
        ['Fabricante', sys.manufacturer], ['Modelo', sys.model || vm.model],
        ['SKU', sys.system_sku], ['Chassis', hw.chassis?.type_name],
      ])}</div>
      <div class="section"><h3>Placa-mãe & BIOS</h3>${kvGrid([
        ['Placa-mãe', board.product ? `${board.manufacturer} ${board.product}` : null],
        ['Serial placa', board.serial],
        ['BIOS', bios.version],
        ['Secure Boot', hw.secure_boot?.enabled != null ? (hw.secure_boot.enabled ? 'Ativo' : 'Inativo') : null],
        ['TPM', hw.tpm?.present ? `v${hw.tpm.version}` : 'Não detectado'],
      ])}</div>
      <div class="section"><h3>CPU & RAM</h3>
        ${dataTable(hw.cpu || [], [
          { label: 'Modelo', key: 'name' }, { label: 'Cores', key: 'cores' }, { label: 'Threads', key: 'logical_processors' },
        ])}
        ${kvGrid([['RAM total', hw.ram?.total_gb ? `${hw.ram.total_gb} GB` : null]])}
        ${dataTable(hw.ram?.modules || [], [
          { label: 'Slot', key: 'locator' }, { label: 'GB', render: r => r.capacity_gb },
          { label: 'MHz', key: 'speed_mhz' }, { label: 'Fabricante', key: 'manufacturer' },
        ])}
      </div>
      <div class="section"><h3>GPU & Monitores</h3>
        ${dataTable(hw.gpu || [], [
          { label: 'GPU', key: 'name' }, { label: 'VRAM MB', key: 'adapter_ram_mb' }, { label: 'Driver', key: 'driver_version' },
        ])}
        ${dataTable(hw.monitors || [], [
          { label: 'Nome', key: 'name' }, { label: 'Resolução', key: 'current_resolution' }, { label: 'Serial', key: 'serial' },
        ])}
      </div>
      <div class="section"><h3>Discos</h3>
        ${dataTable(hw.physical_disks || [], [
          { label: 'Modelo', key: 'model' }, { label: 'GB', key: 'size_gb' }, { label: 'Tipo', key: 'media_type' },
        ])}
        ${dataTable(hw.logical_disks || hw.disks || [], [
          { label: 'Unidade', key: 'letter' }, { label: 'GB', key: 'size_gb' },
          { label: 'Livre', render: r => r.free_gb != null ? `${r.free_gb} GB (${r.free_percent}%)` : '—' },
        ])}
      </div>
      <div class="section"><h3>Periféricos</h3>
        ${kvGrid([
          ['Impressoras', per.device_counts?.printers],
          ['USB', per.device_counts?.usb],
          ['Bluetooth', per.device_counts?.bluetooth],
        ])}
        ${dataTable(per.printers || [], [
          { label: 'Impressora', key: 'name' }, { label: 'Porta', key: 'port' },
          { label: 'Padrão', render: r => r.default ? 'Sim' : 'Não' },
        ])}
      </div>`;
  } else if (tab === 'security') {
    document.getElementById('summary-grid').classList.add('hidden');
    const sec = getCollector('security');
    const hw = getCollector('hardware');
    const ev = getCollector('event_logs');
    const sum = ev.summary || {};
    const def = sec.defender || {};
    const eset = sec.eset || vm.eset_info || {};
    const fw = sec.firewall || {};
    panels.innerHTML = `
      <div class="layer-banner layer-inventory">Segurança — inventário e alertas (BitLocker é informativo, não afeta score).</div>
      <div class="section"><h3>ESET corporativo</h3>${renderEsetCard(vm.eset_info)}</div>
      <div class="section"><h3>Windows Defender</h3>${kvGrid([
        ['Ativo', def.enabled != null ? (def.enabled ? 'Sim' : 'Não') : null],
        ['Tempo real', def.real_time_protection != null ? (def.real_time_protection ? 'Sim' : 'Não') : null],
        ['Defs atualizadas', def.definitions_up_to_date != null ? (def.definitions_up_to_date ? 'Sim' : 'Não') : null],
      ])}</div>
      <div class="section"><h3>Firewall</h3>${kvGrid([
        ['Ativo', fw.enabled != null ? (fw.enabled ? 'Sim' : 'Não') : null],
        ['Perfil domínio', fw.domain_profile], ['Perfil privado', fw.private_profile], ['Perfil público', fw.public_profile],
      ])}</div>
      <div class="section"><h3>BitLocker</h3>
        ${(sec.bitlocker || []).length
          ? dataTable(sec.bitlocker, [
              { label: 'Volume', key: 'mount_point' },
              { label: 'Proteção', key: 'protection_status' },
              { label: 'Criptografia %', key: 'encryption_percent' },
            ])
          : '<p class="empty-note">BitLocker não detectado ou sem permissão.</p>'}
      </div>
      <div class="section"><h3>TPM &amp; Secure Boot</h3>${kvGrid([
        ['TPM', hw.tpm?.present ? `Presente v${hw.tpm.version}` : 'Ausente'],
        ['TPM ativo', hw.tpm?.enabled != null ? (hw.tpm.enabled ? 'Sim' : 'Não') : null],
        ['Secure Boot', hw.secure_boot?.enabled != null ? (hw.secure_boot.enabled ? 'Ativo' : 'Inativo') : null],
      ])}</div>
      <div class="section"><h3>Administradores locais</h3>
        ${dataTable(sec.local_admins || [], [
          { label: 'Usuário', key: 'name' }, { label: 'Origem', key: 'principal_source' },
        ])}
      </div>
      <div class="section"><h3>Erros &amp; BSOD (${sum.period_days || 14} dias)</h3>${kvGrid([
        ['Telas azuis', sum.bsod_count], ['Erros System', sum.system_errors_count],
        ['Erros Application', sum.application_errors_count], ['Minidumps', sum.minidump_count],
        ['Último BSOD', sum.last_bsod_summary], ['Código', sum.last_bugcheck_code],
      ])}
        ${dataTable(ev.bsod_events || [], [
          { label: 'Data', render: r => formatDate(r.time) },
          { label: 'Resumo', key: 'summary' },
          { label: 'Código', render: r => r.bugcheck?.code || '—' },
        ])}
      </div>`;
  } else if (tab === 'all') {
    document.getElementById('summary-grid').classList.remove('hidden');
    renderSummaryGrid(vm);
    panels.innerHTML = `<div class="section"><pre style="font-size:0.72rem;overflow:auto;max-height:70vh;white-space:pre-wrap">${esc(JSON.stringify(currentDetail.collectors, null, 2))}</pre></div>`;
  }
}

async function saveAdmin(e) {
  e.preventDefault();
  const form = e.target;
  const status = document.getElementById('admin-save-status');
  const body = Object.fromEntries(new FormData(form).entries());
  body.ticket_receive_departments = [...form.querySelectorAll('input[name="ticket_receive_departments"]:checked')].map(input => input.value);
  try {
    const saved = await fetch(`/api/machines/${currentDetail.machine.id}/admin`, {
      method: 'PATCH',
      headers: { ...authHeaders(), 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!saved.ok) throw new Error(await saved.text());
    currentDetail.admin = await saved.json();
    clearAdminDraft();
    status.textContent = 'Salvo!';
    status.className = 'save-status ok';
    adminFormDirty = false;
    await refreshDashboard();
  } catch (err) {
    status.textContent = 'Erro ao salvar';
    status.className = 'save-status bad';
    console.error(err);
  }
  setTimeout(() => { status.textContent = ''; }, 3000);
}

const TICKET_STATUS = {
  open: 'Aberto', in_progress: 'Em andamento', waiting: 'Aguardando', done: 'Concluído',
};
const TICKET_TONE = { open: 'open', in_progress: 'progress', waiting: 'wait', done: 'done' };

function ticketAge(ticket) {
  const ms = Date.now() - new Date(ticket.created_at).getTime();
  const mins = Math.max(0, Math.round(ms / 60000));
  if (mins < 60) return `${mins || 1} min`;
  const hours = Math.round(mins / 60);
  if (hours < 48) return `${hours} h`;
  return `${Math.round(hours / 24)} dias`;
}

function ticketMatchesFilters(ticket) {
  const query = (document.getElementById('tickets-search')?.value || '').trim().toLowerCase();
  const activeStatus = document.querySelector('#tickets-status-pills .tf-pill.active')?.dataset.tfStatus || 'all';
  const priority = document.getElementById('tickets-filter-priority')?.value || 'all';
  const from = document.getElementById('tf-date-from')?.value || '';
  const to = document.getElementById('tf-date-to')?.value || '';
  if (activeStatus !== 'all' && ticket.status !== activeStatus) return false;
  if (priority !== 'all' && ticket.priority !== priority) return false;
  if (from && ticket.created_at.slice(0, 10) < from) return false;
  if (to && ticket.created_at.slice(0, 10) > to) return false;
  if (!query) return true;
  return [ticket.code, ticket.title, ticket.hostname_snapshot, ticket.owner_name_snapshot, ticket.department]
    .filter(Boolean).join(' ').toLowerCase().includes(query);
}

function renderKanbanCard(ticket) {
  const active = ticket.id === selectedTicketId ? ' is-selected' : '';
  const priority = ticket.priority || 'normal';
  return `<button type="button" class="kanban-card priority-${esc(priority)}${active}" data-ticket-open="${esc(ticket.id)}">
    <div class="kanban-card-top"><span class="kanban-card-code">${esc(ticket.code)}</span><span class="kanban-prio prio-${esc(priority)}">${esc(priority === 'high' ? 'Alta' : priority === 'low' ? 'Baixa' : 'Normal')}</span></div>
    <div class="kanban-card-title">${esc(ticket.title)}</div>
    <div class="kanban-card-meta"><span class="kanban-host">${esc(ticket.hostname_snapshot || 'PC não informado')}</span><span class="kanban-ago">${esc(ticketAge(ticket))}</span></div>
    <div class="kanban-owner">${esc(ticket.owner_name_snapshot || 'Usuário do computador')}</div>
    <div class="kanban-card-stats"><span>${esc(ticket.department || 'ti')}</span><span>${(ticket.comments || []).length} conversa(s)</span><span>${(ticket.attachments || []).length} anexo(s)</span></div>
  </button>`;
}

function renderTicketDetail(ticket) {
  const detail = document.getElementById('ticket-detail');
  if (!ticket) { detail.classList.add('hidden'); detail.innerHTML = ''; return; }
  const comments = (ticket.comments || []).length
    ? ticket.comments.map(comment => `<article class="ticket-comment"><strong>${esc(comment.author_name || 'Sistema')}</strong><span>${esc(formatDate(comment.created_at))}</span><p>${esc(comment.body)}</p></article>`).join('')
    : '<p class="empty-note">Nenhuma conversa registrada.</p>';
  const attachments = (ticket.attachments || []).length
    ? `<ul class="ticket-attachments">${ticket.attachments.map(file => `<li>${esc(file.filename)}</li>`).join('')}</ul>`
    : '<p class="empty-note">Nenhum anexo.</p>';
  detail.classList.remove('hidden');
  detail.innerHTML = `<div class="ticket-detail-head"><div><p class="ticket-detail-code">${esc(ticket.code)}</p><h3>${esc(ticket.title)}</h3></div><button type="button" class="btn secondary" data-ticket-close-detail>Fechar</button></div>
    <div class="ticket-detail-chips"><span class="chip status-${esc(ticket.status)}">${esc(TICKET_STATUS[ticket.status] || ticket.status)}</span><span class="chip host">${esc(ticket.hostname_snapshot || 'PC não informado')}</span><span class="chip owner">${esc(ticket.owner_name_snapshot || 'Usuário do computador')}</span><span class="chip">${esc(ticket.department || 'TI')}</span></div>
    <p>${esc(ticket.description || 'Sem descrição adicional.')}</p>
    <h4>Andamento</h4><div class="ticket-detail-actions"><select class="kanban-status-select" id="ticket-detail-status">${Object.entries(TICKET_STATUS).map(([key,label]) => `<option value="${key}"${ticket.status === key ? ' selected' : ''}>${label}</option>`).join('')}</select><button type="button" class="btn" data-ticket-save-status="${esc(ticket.id)}">Salvar andamento</button></div>
    ${ticket.resolution ? `<h4>Resolução</h4><p>${esc(ticket.resolution)}</p>` : ''}
    <h4>Conversas</h4><div class="ticket-comments">${comments}</div>
    <h4>Anexos</h4>${attachments}`;
  detail.querySelector('[data-ticket-close-detail]')?.addEventListener('click', () => { selectedTicketId = null; renderTickets(false); });
  detail.querySelector('[data-ticket-save-status]')?.addEventListener('click', async event => {
    const id = event.currentTarget.dataset.ticketSaveStatus;
    const status = detail.querySelector('#ticket-detail-status').value;
    await fetchJson(`/api/tickets/${id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ status }) });
    await renderTickets(true);
  });
}

async function renderTickets(refresh = true) {
  const board = document.getElementById('kanban-board');
  if (!board) return;
  try {
    board.classList.add('is-loading');
    if (refresh) ticketList = await fetchJson('/api/tickets');
    const list = ticketList.filter(ticketMatchesFilters);
    const byStatus = { open: [], in_progress: [], waiting: [], done: [] };
    list.forEach(ticket => (byStatus[ticket.status] || byStatus.open).push(ticket));
    const mini = document.getElementById('tickets-stats-mini');
    if (mini) mini.textContent = `${list.length} chamado(s) exibido(s) · ${byStatus.open.length} aberto(s) · ${byStatus.in_progress.length} em andamento · ${byStatus.waiting.length} aguardando · ${byStatus.done.length} concluído(s)`;
    const sync = document.getElementById('tickets-sync');
    if (sync) sync.textContent = `Atualizado ${new Date().toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })}`;
    board.innerHTML = Object.entries(TICKET_STATUS).map(([status, label]) => `<section class="kanban-col tone-${TICKET_TONE[status]}"><header class="kanban-col-head"><h3>${label}</h3><span class="kanban-count">${byStatus[status].length}</span></header><div class="kanban-col-body">${byStatus[status].length ? byStatus[status].map(renderKanbanCard).join('') : '<p class="kanban-empty">Nenhum chamado.</p>'}</div></section>`).join('');
    board.querySelectorAll('[data-ticket-open]').forEach(button => button.addEventListener('click', () => { selectedTicketId = button.dataset.ticketOpen; renderTicketDetail(ticketList.find(ticket => ticket.id === selectedTicketId)); renderTickets(false); }));
    renderTicketDetail(ticketList.find(ticket => ticket.id === selectedTicketId));
  } catch (error) {
    board.innerHTML = '<p class="empty-note">Não foi possível carregar os chamados.</p>';
    console.error(error);
  } finally { board.classList.remove('is-loading'); }
}

function drawBarChart(canvasId, rows, color = '#5b9cf5') {
  const canvas = document.getElementById(canvasId);
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  const width = canvas.width, height = canvas.height;
  ctx.clearRect(0, 0, width, height);
  const max = Math.max(1, ...rows.map(row => row.value));
  const chartWidth = width - 36, chartHeight = height - 42;
  const step = chartWidth / Math.max(1, rows.length);
  ctx.font = '11px system-ui'; ctx.textAlign = 'center';
  rows.forEach((row, index) => {
    const barHeight = (row.value / max) * (chartHeight - 10);
    const x = 22 + index * step + step * .18;
    const y = height - 26 - barHeight;
    ctx.fillStyle = color; ctx.fillRect(x, y, step * .64, barHeight);
    ctx.fillStyle = '#dbeafe'; ctx.fillText(String(row.value), x + step * .32, Math.max(12, y - 5));
    ctx.fillStyle = '#93a4bf'; ctx.fillText(row.label, x + step * .32, height - 8);
  });
}

async function renderBi() {
  const tickets = await fetchJson('/api/tickets');
  const byStatus = Object.keys(TICKET_STATUS).reduce((result, status) => ({ ...result, [status]: tickets.filter(ticket => ticket.status === status).length }), {});
  const done = tickets.filter(ticket => ticket.status === 'done');
  const durationHours = done.map(ticket => (new Date(ticket.closed_at || ticket.updated_at) - new Date(ticket.created_at)) / 3600000).filter(value => Number.isFinite(value));
  const avgHours = durationHours.length ? durationHours.reduce((sum, value) => sum + value, 0) / durationHours.length : null;
  const rated = tickets.filter(ticket => ticket.rating != null);
  const satisfaction = rated.length ? rated.reduce((sum, ticket) => sum + Number(ticket.rating), 0) / rated.length : null;
  const withinSla = done.length ? Math.round(done.filter(ticket => (new Date(ticket.closed_at || ticket.updated_at) - new Date(ticket.created_at)) <= 48 * 3600000).length / done.length * 100) : null;
  const kpis = [
    ['Total', tickets.length, 'total'], ['Abertos', byStatus.open, 'open'], ['Em andamento', byStatus.in_progress, 'progress'], ['Aguardando', byStatus.waiting, 'wait'], ['Concluídos', byStatus.done, 'done'], ['Tempo médio', avgHours == null ? '—' : `${avgHours.toFixed(1)}h`, 'avg-time'], ['Satisfação', satisfaction == null ? '—' : `${satisfaction.toFixed(1)}/5`, 'satisfaction'], ['SLA 48h', withinSla == null ? '—' : `${withinSla}%`, `sla ${withinSla != null && withinSla < 80 ? 'sla-bad' : ''}`],
  ];
  document.getElementById('bi-kpi-row').innerHTML = kpis.map(([label,value,cls]) => `<div class="bi-kpi kpi-${cls}"><span class="bi-kpi-val">${esc(value)}</span><span class="bi-kpi-lbl">${esc(label)}</span></div>`).join('');
  document.getElementById('bi-resolve-time').textContent = avgHours == null ? '—' : `${avgHours.toFixed(1)}h`;
  document.getElementById('bi-resolve-detail').textContent = `${done.length} chamado(s) concluído(s) na demonstração`;
  document.getElementById('bi-satisfaction').textContent = satisfaction == null ? 'Sem avaliações' : `${satisfaction.toFixed(1)}/5`;
  document.getElementById('bi-satisfaction-detail').textContent = rated.length ? `${rated.length} avaliação(ões)` : 'Avaliação disponível ao concluir';
  document.getElementById('bi-sla').textContent = withinSla == null ? '—' : `${withinSla}%`;
  document.getElementById('bi-sla-detail').textContent = 'Meta de resolução em até 48h';
  drawBarChart('bi-chart-status', Object.entries(TICKET_STATUS).map(([key,label]) => ({ label: label.slice(0, 5), value: byStatus[key] })), '#5b9cf5');
  const priorities = ['high','normal','low'].map(priority => ({ label: priority === 'high' ? 'Alta' : priority === 'low' ? 'Baixa' : 'Normal', value: tickets.filter(ticket => ticket.priority === priority).length }));
  drawBarChart('bi-chart-priority', priorities, '#e8b339');
  const age = [{label:'<1d',value:tickets.filter(ticket => ticket.status !== 'done' && ticketAge(ticket).includes('min')).length},{label:'1–7d',value:tickets.filter(ticket => ticket.status !== 'done' && !ticketAge(ticket).includes('min')).length},{label:'>7d',value:0}];
  drawBarChart('bi-chart-age', age, '#a78bfa');
  const perPc = Object.entries(tickets.reduce((map, ticket) => { map[ticket.hostname_snapshot || 'Sem PC'] = (map[ticket.hostname_snapshot || 'Sem PC'] || 0) + 1; return map; }, {})).map(([label,value]) => ({label,value}));
  drawBarChart('bi-chart-bypc', perPc, '#3ecf8e');
  drawBarChart('bi-chart-volume', [{label:'Abertos',value:tickets.length},{label:'Fechados',value:done.length}], '#5b9cf5');
  document.getElementById('bi-open-tbody').innerHTML = tickets.filter(ticket => ticket.status !== 'done').map(ticket => `<tr><td>${esc(ticket.code)}</td><td>${esc(ticket.title)}</td><td>${esc(ticket.hostname_snapshot || '—')}</td><td>${esc(ticket.priority || 'normal')}</td><td>${esc(TICKET_STATUS[ticket.status] || ticket.status)}</td><td>${esc(ticketAge(ticket))}</td></tr>`).join('') || '<tr><td colspan="6">Nenhum chamado aberto.</td></tr>';
}

let mgmtSearchQuery = '';
let mgmtFontSize = sessionStorage.getItem('belarc-mgmt-font') || 'large';

function applyMgmtFontSize() {
  const report = document.getElementById('management-report');
  if (!report) return;
  report.classList.remove('font-normal', 'font-large', 'font-xlarge');
  report.classList.add(`font-${mgmtFontSize === 'normal' ? 'normal' : mgmtFontSize === 'xlarge' ? 'xlarge' : 'large'}`);
  document.body.classList.remove('mgmt-font-normal', 'mgmt-font-large', 'mgmt-font-xlarge');
  document.body.classList.add(`mgmt-font-${mgmtFontSize}`);
}

function reportEsetText(m) {
  if (!m.eset_installed) return 'Não instalado';
  const parts = [m.eset_info?.product_name || m.eset_product || 'ESET'];
  if (m.eset_info?.version) parts.push(`v${m.eset_info.version}`);
  if (m.eset_info?.real_time_active === false) parts.push('(proteção desativada)');
  return parts.join(' ');
}

function reportOfficeText(m) {
  const keys = m.license_keys?.office_keys || [];
  const fromLic = keys.map(o => shortenOfficeName(o.product)).filter(Boolean);
  const fromList = (m.office_licenses || []).map(shortenOfficeName);
  const merged = [...new Set([...fromLic, ...fromList])];
  return merged.length ? merged.join(', ') : '—';
}

function reportAlertsText(m) {
  const n = alertCountForMachine(m.id);
  return n ? `${n} alerta(s)` : 'Nenhum';
}

function reportHealthText(m) {
  if (m.health_score == null) return '—';
  return `${m.health_score}% (${scoreBand(m.health_score)})`;
}

function reportStandardRatio(m) {
  const total = (m.standard_apps || []).length;
  const ok = m.standard_apps_installed ?? (m.standard_apps || []).filter(a => a.installed).length;
  return total ? `${ok} de ${total} instalados` : '—';
}

function reportAppsList(m) {
  const apps = m.installed_apps?.length
    ? m.installed_apps
    : (m.standard_apps || []).filter(a => a.installed).map(a => APP_DISPLAY_NAMES[a.id] || a.label);
  return apps;
}

function reportUser(m) {
  return m.admin?.owner_name || '—';
}

function reportEmails(m) {
  const emails = [m.admin?.primary_email, m.display_email, ...(m.thunderbird_emails || [])].filter(Boolean);
  return [...new Set(emails)];
}

function reportMissingApps(m) {
  return (m.standard_apps || []).filter(a => !a.installed).map(a => APP_DISPLAY_NAMES[a.id] || a.label);
}

function filterMgmtMachines(list) {
  const q = mgmtSearchQuery.toLowerCase().trim();
  if (!q) return list;
  return list.filter(m => {
    const hay = [
      m.hostname, m.lan_ip, m.anydesk_id, m.logged_user,
      m.admin?.owner_name, m.admin?.ramal, m.admin?.network_cable,
      reportEmails(m).join(' '), reportAppsList(m).join(' '),
      (m.folder_access || []).join(' '),
    ].filter(Boolean).join(' ').toLowerCase();
    return hay.includes(q);
  });
}

function renderMgmtField(label, value, cls = '') {
  const v = value == null || value === '' ? '—' : value;
  return `<div class="mgmt-field ${cls}"><span class="mgmt-label">${esc(label)}</span><span class="mgmt-value">${esc(v)}</span></div>`;
}

function renderMgmtList(label, items, missing = false) {
  if (!items?.length) {
    return `<div class="mgmt-block"><div class="mgmt-block-title">${esc(label)}</div><p class="mgmt-empty">Nenhum item</p></div>`;
  }
  const cls = missing ? 'mgmt-list missing' : 'mgmt-list';
  return `<div class="mgmt-block"><div class="mgmt-block-title">${esc(label)}</div><ul class="${cls}">${items.map(i => `<li>${esc(i)}</li>`).join('')}</ul></div>`;
}

function renderMgmtCard(m) {
  const apps = reportAppsList(m);
  const missing = reportMissingApps(m);
  const emails = reportEmails(m);
  const folders = m.folder_access || [];
  const statusLabel = m.status === 'online' ? 'Online' : 'Offline';
  const statusCls = m.status === 'online' ? 'ok' : 'off';
  const alertN = alertCountForMachine(m.id);
  const pcAlerts = dedupeAlerts(alerts.filter(a => a.machine_id === m.id)).slice(0, 8);
  const healthText = reportHealthText(m);

  return `<article class="mgmt-pc-card" data-hostname="${esc(m.hostname)}">
    <header class="mgmt-card-header">
      <div class="mgmt-card-title-wrap">
        <h3 class="mgmt-pc-name">${esc(m.hostname)}</h3>
        <span class="mgmt-status ${statusCls}">${statusLabel}</span>
        ${m.health_score != null ? `<span class="mgmt-health ${scoreClass(m.health_score)}">${esc(healthText)}</span>` : ''}
        ${alertN ? `<span class="mgmt-alert-count">${alertN} alerta(s)</span>` : ''}
      </div>
      <button type="button" class="btn-link no-print" data-open="${m.id}">Ver detalhes →</button>
    </header>
    <div class="mgmt-card-body">
      <section class="mgmt-section mgmt-layer-health">
        <h4>Conformidade TI</h4>
        <div class="mgmt-fields-grid">
          ${renderMgmtField('Score corporativo', healthText)}
          ${renderMgmtField('BSOD recente', m.has_recent_bsod ? (m.last_bugcheck_code || 'Sim') : 'Não')}
          ${renderMgmtField('Erros de sistema', m.system_errors_count || 0)}
        </div>
      </section>
      <section class="mgmt-section mgmt-layer-alerts">
        <h4>Alertas (${alertN})</h4>
        ${pcAlerts.length
          ? `<ul class="mgmt-alert-list">${pcAlerts.map(a => `<li class="sev-${a.severity}"><strong>${esc(ALERT_CATEGORY_LABELS[a.category] || a.category)}</strong> — ${esc(a.message)}</li>`).join('')}</ul>`
          : '<p class="mgmt-empty">Nenhum alerta ativo.</p>'}
      </section>
      <section class="mgmt-section mgmt-layer-inventory">
        <h4>Inventário — pessoa e contato</h4>
        <div class="mgmt-fields-grid">
          ${renderMgmtField('Responsável', reportUser(m))}
          ${renderMgmtField('Ramal', m.admin?.ramal)}
          ${renderMgmtField('E-mail cadastrado', m.admin?.primary_email)}
          ${renderMgmtField('Usuário Windows', m.logged_user)}
          ${renderMgmtField('E-mails no Thunderbird', emails.join(', '))}
        </div>
      </section>
      <section class="mgmt-section">
        <h4>Equipamento e sistema</h4>
        <div class="mgmt-fields-grid">
          ${renderMgmtField('Modelo do PC', m.model)}
          ${renderMgmtField('Sistema operacional', m.os_version)}
          ${renderMgmtField('Última atualização', formatDate(m.last_seen))}
          ${renderMgmtField('Tempo ligado', formatUptime(m.uptime_seconds))}
        </div>
      </section>
      <section class="mgmt-section">
        <h4>Rede e acesso remoto</h4>
        <div class="mgmt-fields-grid">
          ${renderMgmtField('IP na rede (LAN)', m.lan_ip)}
          ${renderMgmtField('AnyDesk ID', m.anydesk_id)}
          ${renderMgmtField('AnyDesk', m.anydesk_running ? 'Serviço ativo' : (m.anydesk_id ? 'Instalado' : '—'))}
          ${renderMgmtField('Tailscale', m.tailscale_connected ? `Conectado — ${m.tailscale_ip || ''}` : (m.tailscale_installed ? 'Instalado (desligado)' : 'Não instalado'))}
          ${renderMgmtField('Cabo de rede', m.admin?.network_cable)}
        </div>
      </section>
      <section class="mgmt-section">
        <h4>Sistemas, licenças e segurança</h4>
        <div class="mgmt-fields-grid">
          ${renderMgmtField('ESET Antivírus', reportEsetText(m))}
          ${renderMgmtField('Microsoft Office', reportOfficeText(m))}
          ${renderMgmtField('Chave Windows', formatKeyPartial(m.license_keys?.windows_key_partial || m.windows_key_partial))}
          ${renderMgmtField('Software padrão empresa', reportStandardRatio(m))}
          ${(companyProfile?.erp_branches || []).map(b =>
            renderMgmtField(b.label, (m.standard_apps || []).find(a => a.id === b.id)?.installed ? 'Instalado' : 'Não instalado')
          ).join('')}
          ${renderMgmtField(bankingAppLabel(), getBankingApp(m).installed ? `Instalado${getBankingApp(m).version ? ` v${getBankingApp(m).version}` : ''}` : 'Não instalado')}
          ${renderMgmtField(`${companyProfile?.erp_name || 'ERP'} — login`, m.admin?.cybersul_user)}
          ${renderMgmtField('Login NAS Compartilhamento de arquivos', m.admin?.nas_user)}
        </div>
      </section>
      ${renderMgmtList(`Programas instalados (${apps.length})`, apps)}
      ${missing.length ? renderMgmtList(`Programas ausentes (${missing.length})`, missing, true) : ''}
      ${renderMgmtList(`Pastas e drives mapeados (${folders.length})`, folders)}
      ${m.admin?.notes ? `<section class="mgmt-section"><h4>Observações da TI</h4><p class="mgmt-notes">${esc(m.admin.notes)}</p></section>` : ''}
    </div>
  </article>`;
}

function renderMgmtLegend() {
  return `<div class="mgmt-legend">
    <strong>Três camadas — como ler este relatório</strong>
    <ul>
      <li><strong>Inventário:</strong> dados coletados (hardware, software, rede) — não alteram o score.</li>
      <li><strong>Alertas:</strong> avisos da TI (certificados, BitLocker, software) — atenção sem derrubar nota global.</li>
      <li><strong>Conformidade TI:</strong> score corporativo (SMART 30%, segurança 25%, updates 15%, eventos 10%, disco 10%, temp. 5%, cert. corp. 5%).</li>
      <li><strong>Faixas:</strong> 90–100 Excelente · 75–89 Bom · 50–74 Atenção · 25–49 Problema · 0–24 Crítico.</li>
      <li>✓ = programa instalado · ✗ = programa padrão ausente · Alertas = contagem server-side (/api/alerts).</li>
    </ul>
  </div>`;
}

function renderMgmtIndex(list, extraClass = '') {
  return `<nav class="mgmt-index ${extraClass}">
    <h4>Índice — ${list.length} computador(es)</h4>
    <ol>${list.map(m => `<li><strong>${esc(m.hostname)}</strong> — ${esc(reportUser(m))}${m.admin?.ramal ? ` · Ramal ${esc(m.admin.ramal)}` : ''}</li>`).join('')}</ol>
  </nav>`;
}

function renderMgmtSummaryTable(list) {
  return `<table class="mgmt-summary-table">
    <thead><tr>
      <th>Status</th><th>Computador</th><th>Responsável</th><th>Score</th><th>Faixa</th>
      <th>Alertas</th><th>BSOD</th><th>IP</th><th>ESET</th><th>Software</th>
    </tr></thead>
    <tbody>${list.map(m => {
      const st = m.status === 'online' ? 'Online' : 'Offline';
      const stCls = m.status === 'online' ? 'cell-ok' : 'cell-off';
      const alertN = alertCountForMachine(m.id);
      const band = m.health_score != null ? scoreBand(m.health_score) : '—';
      const scoreCls = m.health_score != null ? scoreClass(m.health_score) : '';
      return `<tr>
      <td class="${stCls}">${esc(st)}</td>
      <td><strong>${esc(m.hostname)}</strong></td>
      <td>${esc(reportUser(m))}</td>
      <td class="${scoreCls}">${m.health_score != null ? `${m.health_score}%` : '—'}</td>
      <td class="${scoreCls}">${esc(band)}</td>
      <td class="${alertN ? 'cell-warn' : ''}">${alertN || '0'}</td>
      <td class="${m.has_recent_bsod ? 'cell-warn' : ''}">${m.has_recent_bsod ? 'Sim' : 'Não'}</td>
      <td>${esc(m.lan_ip || '—')}</td>
      <td>${esc(reportEsetText(m))}</td>
      <td>${esc(reportStandardRatio(m))}</td>
    </tr>`;
    }).join('')}</tbody>
  </table>`;
}

function renderManagementReport() {
  const el = document.getElementById('management-report');
  if (!machines.length) {
    el.innerHTML = '<p class="empty-note">Nenhum PC cadastrado.</p>';
    return;
  }
  const now = new Date().toLocaleString('pt-BR');
  const sorted = filterMgmtMachines([...machines].sort((a, b) => (a.hostname || '').localeCompare(b.hostname || '')));
  const online = machines.filter(m => m.status === 'online').length;

  el.innerHTML = `
    <div class="mgmt-doc-header">
      <div class="mgmt-doc-title">Inventário de Computadores — Gestão TI</div>
      <div class="mgmt-doc-sub">Belarc Inventory · Relatório gerado em ${esc(now)} · ${machines.length} PC(s) cadastrado(s)</div>
    </div>
    ${renderMgmtLegend()}
    <div class="mgmt-stats-bar no-print">
      <div class="mgmt-stat"><span class="mgmt-stat-num">${machines.length}</span><span class="mgmt-stat-lbl">Computadores</span></div>
      <div class="mgmt-stat ok"><span class="mgmt-stat-num">${online}</span><span class="mgmt-stat-lbl">Online</span></div>
      <div class="mgmt-stat"><span class="mgmt-stat-num">${machines.length - online}</span><span class="mgmt-stat-lbl">Offline</span></div>
      <div class="mgmt-stat"><span class="mgmt-stat-num">${sorted.length}</span><span class="mgmt-stat-lbl">Exibindo</span></div>
    </div>
    <div class="mgmt-print-summary">
      ${renderMgmtIndex(sorted)}
      ${renderMgmtSummaryTable(sorted)}
    </div>
    ${renderMgmtIndex(sorted, 'mgmt-index-detail print-only')}
    <div class="mgmt-cards">${sorted.map(renderMgmtCard).join('')}</div>
    <footer class="mgmt-doc-footer">Documento interno — TI · Belarc Inventory · Dados coletados automaticamente; cadastro manual em Cadastro TI.</footer>`;

  applyMgmtFontSize();
  el.querySelectorAll('[data-open]').forEach(btn => {
    btn.addEventListener('click', () => selectMachine(btn.dataset.open));
  });
}

function printMgmtReport(mode) {
  applyMgmtFontSize();
  document.body.classList.remove('print-summary', 'print-detail');
  document.body.classList.add(mode === 'summary' ? 'print-summary' : 'print-detail');
  window.print();
  setTimeout(() => document.body.classList.remove('print-summary', 'print-detail'), 500);
}

function setView(view) {
  const requiresTi = ['directory', 'tickets', 'bi'].includes(view);
  if (requiresTi && !tiToken) {
    openLoginModal();
    return;
  }
  currentView = view;
  document.querySelectorAll('.view-btn').forEach(b => b.classList.toggle('active', b.dataset.view === view));
  const isMgmt = view === 'management';
  const isTickets = view === 'tickets';
  const isBi = view === 'bi';
  const isDirectory = view === 'directory';
  document.querySelector('.sidebar').classList.toggle('hidden', isMgmt);
  document.getElementById('detail-content').classList.toggle('hidden', isMgmt || isTickets || isBi || isDirectory || !selectedId);
  document.getElementById('empty-state').classList.toggle('hidden', isMgmt || isTickets || isBi || isDirectory || selectedId);
  document.getElementById('management-panel').classList.toggle('hidden', !isMgmt);
  document.getElementById('tickets-panel').classList.toggle('hidden', !isTickets);
  document.getElementById('bi-panel').classList.toggle('hidden', !isBi);
  document.getElementById('directory-panel').classList.toggle('hidden', !isDirectory);
  if (isMgmt) {
    renderManagementReport();
    applyMgmtFontSize();
  } else if (isTickets) {
    renderTickets(true);
  } else if (isBi) {
    renderBi().catch(console.error);
  } else if (isDirectory) {
    const panel = document.getElementById('directory-panel');
    const main = panel?.querySelector('#directory-main');
    if (main) main.innerHTML = '<p class="empty-note">O Diretório é alimentado pelo inventário e pelo Cadastro TI. Use a busca por pessoa, PC, e-mail, hostname, AnyDesk ou cabo para localizar o contexto operacional.</p>';
  } else if (selectedId && currentDetail) {
    updateDetailView();
  } else if (!selectedId) {
    document.getElementById('empty-state').classList.remove('hidden');
  }
}

document.getElementById('search').addEventListener('input', renderMachineList);
document.getElementById('filter-status').addEventListener('change', renderMachineList);
document.getElementById('tab-panels')?.addEventListener('click', e => {
  const btn = e.target.closest('.btn-copy-cmd');
  if (!btn) return;
  const cmd = btn.dataset.cmd || '';
  navigator.clipboard?.writeText(cmd).then(() => {
    btn.textContent = 'Copiado!';
    setTimeout(() => { btn.textContent = 'Copiar comando'; }, 2000);
  }).catch(() => { prompt('Copie o comando:', cmd); });
});

document.getElementById('tabs').addEventListener('click', e => {
  const tab = e.target.closest('.tab');
  if (tab) renderTab(tab.dataset.tab);
});
document.getElementById('view-toggle').addEventListener('click', e => {
  const btn = e.target.closest('.view-btn');
  if (btn) setView(btn.dataset.view);
});
document.getElementById('btn-login')?.addEventListener('click', openLoginModal);
document.getElementById('login-modal-close')?.addEventListener('click', closeLoginModal);
document.getElementById('login-modal-backdrop')?.addEventListener('click', closeLoginModal);
document.getElementById('login-form')?.addEventListener('submit', async event => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const error = document.getElementById('login-error');
  try {
    const response = await fetch(API + '/api/auth/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: form.get('username'), password: form.get('password') }) });
    if (!response.ok) throw new Error('Usuário ou senha inválidos.');
    const session = await response.json();
    tiToken = session.token;
    sessionStorage.setItem('belarc-ti-token', tiToken);
    setTiUi(true, session.username || 'TI');
    closeLoginModal();
    await refreshDashboard();
  } catch (err) {
    error.textContent = err.message || 'Não foi possível iniciar a sessão.';
    error.classList.remove('hidden');
  }
});
document.getElementById('btn-logout')?.addEventListener('click', async () => {
  try { await fetch(API + '/api/auth/logout', { method: 'POST', headers: authHeaders() }); } catch (_) { /* local cleanup still applies */ }
  tiToken = '';
  sessionStorage.removeItem('belarc-ti-token');
  setTiUi(false);
  setView('inventory');
});
document.getElementById('tickets-status-pills')?.addEventListener('click', event => {
  const pill = event.target.closest('.tf-pill');
  if (!pill) return;
  document.querySelectorAll('#tickets-status-pills .tf-pill').forEach(button => button.classList.toggle('active', button === pill));
  renderTickets(false);
});
document.getElementById('tickets-search')?.addEventListener('input', () => renderTickets(false));
document.getElementById('tickets-filter-priority')?.addEventListener('change', () => renderTickets(false));
document.getElementById('tf-date-from')?.addEventListener('change', () => renderTickets(false));
document.getElementById('tf-date-to')?.addEventListener('change', () => renderTickets(false));
document.getElementById('btn-tickets-refresh')?.addEventListener('click', () => renderTickets(true));
document.getElementById('btn-bi-refresh')?.addEventListener('click', () => renderBi().catch(console.error));
document.getElementById('btn-print-summary').addEventListener('click', () => printMgmtReport('summary'));
document.getElementById('btn-print-detail').addEventListener('click', () => printMgmtReport('detail'));
document.getElementById('mgmt-search').addEventListener('input', e => {
  mgmtSearchQuery = e.target.value;
  renderManagementReport();
});

document.getElementById('stat-alerts-btn')?.addEventListener('click', () => toggleAlertsDrawer(true));
document.getElementById('alerts-drawer-close')?.addEventListener('click', () => toggleAlertsDrawer(false));
document.getElementById('alerts-drawer-backdrop')?.addEventListener('click', () => toggleAlertsDrawer(false));
document.getElementById('alerts-filter-category')?.addEventListener('change', e => {
  drawerCatFilter = e.target.value;
  renderAlertsDrawer();
});
document.getElementById('alerts-filter-search')?.addEventListener('input', e => {
  drawerSearchQuery = e.target.value;
  renderAlertsDrawer();
});
document.getElementById('alerts-sev-pills')?.addEventListener('click', e => {
  const pill = e.target.closest('.alerts-pill');
  if (!pill) return;
  drawerSevFilter = pill.dataset.sev;
  renderAlertsDrawer();
});
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && !document.getElementById('alerts-drawer')?.classList.contains('hidden')) {
    toggleAlertsDrawer(false);
  }
});

const mgmtFontEl = document.getElementById('mgmt-font-size');
if (mgmtFontEl) {
  mgmtFontEl.value = mgmtFontSize;
  mgmtFontEl.addEventListener('change', e => {
    mgmtFontSize = e.target.value;
    sessionStorage.setItem('belarc-mgmt-font', mgmtFontSize);
    applyMgmtFontSize();
  });
}

restoreTiSession().catch(console.error);
refreshDashboard().catch(console.error);
ensureCompanyProfile().catch(console.error);
setInterval(() => refreshDashboard().catch(console.error), 30000);
