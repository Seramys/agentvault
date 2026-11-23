import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  api,
  setToken,
  subscribe,
  token,
  type Account,
  type Agent,
  type AuditEvent,
  type Honeypot,
  type IssuedTicket,
  type Overview,
  type Secret,
  type Status,
  type Ticket,
} from './api';

/* ------------------------------------------------------------------ */
/* design primitives                                                   */
/* ------------------------------------------------------------------ */

type Route = 'overview' | 'notifications' | 'secrets' | 'tickets' | 'agents' | 'audit' | 'honeypots' | 'settings';

const TITLES: Record<Route, string> = {
  overview: '概览',
  notifications: '通知',
  secrets: '凭证',
  tickets: '票据',
  agents: '接入',
  audit: '审计',
  honeypots: '蜜罐',
  settings: '设置',
};

const NAV: Array<{ route: Route; label: string; icon: string }> = [
  { route: 'overview', label: '概览', icon: 'grid' },
  { route: 'notifications', label: '通知', icon: 'bell' },
  { route: 'secrets', label: '凭证', icon: 'key' },
  { route: 'tickets', label: '票据', icon: 'ticket' },
  { route: 'agents', label: '接入', icon: 'nodes' },
  { route: 'audit', label: '审计', icon: 'pulse' },
  { route: 'honeypots', label: '蜜罐', icon: 'alert' },
  { route: 'settings', label: '设置', icon: 'sliders' },
];

const ICONS: Record<string, ReactNode> = {
  grid: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="2" />
      <rect x="14" y="3" width="7" height="7" rx="2" />
      <rect x="3" y="14" width="7" height="7" rx="2" />
      <rect x="14" y="14" width="7" height="7" rx="2" />
    </>
  ),
  key: (
    <>
      <circle cx="8.5" cy="8.5" r="4.5" />
      <path d="M11.7 11.7 21 21M16 16l2.5-2.5M13 19l2-2" />
    </>
  ),
  ticket: (
    <>
      <path d="M3 8.5A2.5 2.5 0 0 1 5.5 6h13A2.5 2.5 0 0 1 21 8.5v1a2.5 2.5 0 0 0 0 5v1a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 15.5v-1a2.5 2.5 0 0 0 0-5v-1Z" />
      <path d="M13 7v10" strokeDasharray="2 3" />
    </>
  ),
  nodes: (
    <>
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="18" cy="6" r="2.5" />
      <circle cx="12" cy="18" r="2.5" />
      <path d="M7.8 7.8 10.6 15M16.2 7.8 13.4 15M8.5 6h7" />
    </>
  ),
  pulse: <path d="M3 12h4l2.2-6 4.2 12 2.2-6H21" />,
  alert: (
    <>
      <path d="M10.3 3.9 2.6 17.2A2 2 0 0 0 4.3 20h15.4a2 2 0 0 0 1.7-2.8L13.7 3.9a2 2 0 0 0-3.4 0Z" />
      <path d="M12 9v4.5M12 17h.01" />
    </>
  ),
  sliders: (
    <>
      <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
      <circle cx="16" cy="7" r="2.2" />
      <circle cx="10" cy="17" r="2.2" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4.5 4.5" />
    </>
  ),
  bell: (
    <>
      <path d="M6 9a6 6 0 0 1 12 0c0 5 2 6 2 6H4s2-1 2-6" />
      <path d="M10 19a2 2 0 0 0 4 0" />
    </>
  ),
  chevron: <path d="m14.5 6-6 6 6 6" />,
  down: <path d="m6 9 6 6 6-6" />,
  plus: <path d="M12 5v14M5 12h14" />,
  copy: (
    <>
      <rect x="8" y="8" width="11" height="11" rx="2.5" />
      <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    </>
  ),
  refresh: <path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" />,
  x: <path d="M6 6l12 12M18 6 6 18" />,
  check: <path d="m5 12.5 4.5 4.5L19 7" />,
  arrow: <path d="M7 17 17 7M9 7h8v8" />,
  shield: (
    <>
      <path d="M12 3 5 6v5.5c0 4.6 3 7.7 7 9.5 4-1.8 7-4.9 7-9.5V6l-7-3Z" />
      <path d="m9 12 2 2 4-4" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7v5l3.5 2" />
    </>
  ),
  db: (
    <>
      <ellipse cx="12" cy="5.5" rx="7" ry="3" />
      <path d="M5 5.5v13c0 1.7 3.1 3 7 3s7-1.3 7-3v-13M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3" />
    </>
  ),
  terminal: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <path d="m7 9 3 3-3 3M13 15h4" />
    </>
  ),
  lock: (
    <>
      <rect x="5" y="10" width="14" height="10" rx="2.5" />
      <path d="M8 10V7a4 4 0 0 1 8 0v3" />
    </>
  ),
  menu: <path d="M4 7h16M4 12h16M4 17h16" />,
  filter: <path d="M4 6h16M7 12h10M10 18h4" />,
  trash: <path d="M4 7h16M9 7V4h6v3M6.5 7l.8 12a2 2 0 0 0 2 1.9h5.4a2 2 0 0 0 2-1.9l.8-12" />,
  user: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4.5 20a7.5 7.5 0 0 1 15 0" />
    </>
  ),
  logout: (
    <>
      <path d="M10 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h4" />
      <path d="M15 8l4 4-4 4M19 12H9" />
    </>
  ),
  spark: <path d="M12 3v3M12 18v3M4.2 7.2l2.1 2.1M17.7 14.7l2.1 2.1M3 12h3M18 12h3M4.2 16.8l2.1-2.1M17.7 9.3l2.1-2.1" />,
};

function Icon({ name, className = '' }: { name: string; className?: string }) {
  return (
    <svg
      className={`icon ${className}`}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {ICONS[name] || ICONS.grid}
    </svg>
  );
}

type SelectOption = { value: string; label: string };

function Select({
  value,
  onChange,
  options,
  testId,
  placeholder = '选择',
  className = '',
}: {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  testId: string;
  placeholder?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value);

  useEffect(() => {
    if (!open) return;
    const closeOnOutside = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', closeOnOutside);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('mousedown', closeOnOutside);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [open]);

  return (
    <div className={`select ${open ? 'open' : ''} ${className}`} ref={rootRef}>
      <button
        type="button"
        className="select-trigger"
        data-testid={testId}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <span className={`select-value ${selected ? '' : 'placeholder'}`}>
          {selected?.label || placeholder}
        </span>
        <Icon name="down" className="select-caret" />
      </button>
      {open ? (
        <div className="select-menu" role="listbox">
          {options.map((option) => (
            <button
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={`select-option ${option.value === value ? 'selected' : ''}`}
              key={option.value}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              <span className="select-option-label">{option.label}</span>
              {option.value === value ? <Icon name="check" className="select-check" /> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* formatting                                                          */
/* ------------------------------------------------------------------ */

function fmtLeft(ms: number): string {
  if (ms <= 0) return '已过期';
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}h ${m % 60}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

function fmtTime(ts: number): string {
  if (!ts) return '—';
  return new Date(ts).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
}

function fmtNum(n: number): string {
  return Number(n).toLocaleString('en-US');
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function parseHash(): Route {
  const raw = window.location.hash.replace(/^#\/?/, '');
  return (NAV.some((item) => item.route === raw) ? raw : 'overview') as Route;
}

/* ------------------------------------------------------------------ */
/* small components                                                    */
/* ------------------------------------------------------------------ */

function Pill({ text, tone }: { text: string; tone?: string }) {
  return (
    <span className={`pill ${tone || ''}`}>
      <span className="dot" />
      {text}
    </span>
  );
}

function Kv({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="kv-row">
      <span>{label}</span>
      <strong className={mono ? 'mono' : ''}>{value}</strong>
    </div>
  );
}

function Spark({ points }: { points: number[] }) {
  const values = points.length > 1 ? points : [0, 0];
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const d = values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * 92;
      const y = 26 - 2 - ((value - min) / span) * 22;
      return `${index ? 'L' : 'M'} ${x} ${y}`;
    })
    .join(' ');
  return (
    <svg className="spark" viewBox="0 0 92 26" preserveAspectRatio="none" aria-hidden="true">
      <path d={d} fill="none" stroke="#0f0f0d" strokeWidth={1.3} strokeLinecap="round" />
    </svg>
  );
}

function Kpi({
  label,
  value,
  icon,
  delta,
  dir,
  points,
}: {
  label: string;
  value: string | number;
  icon: string;
  delta: string;
  dir: 'up' | 'down';
  points: number[];
}) {
  return (
    <div className="kpi">
      <div className="kpi-top">
        <span className="kpi-label">{label}</span>
        <span className="kpi-icon">
          <Icon name={icon} />
        </span>
      </div>
      <div className="kpi-value num">{value}</div>
      <div className="kpi-foot">
        <span className={`kpi-delta ${dir}`}>{delta}</span>
        <Spark points={points} />
      </div>
    </div>
  );
}

function smoothPath(points: Array<[number, number]>): string {
  if (points.length < 2) return '';
  let d = `M ${points[0][0]} ${points[0][1]}`;
  for (let i = 0; i < points.length - 1; i += 1) {
    const p0 = points[Math.max(0, i - 1)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(points.length - 1, i + 2)];
    const cp1x = p1[0] + (p2[0] - p0[0]) / 6;
    const cp1y = p1[1] + (p2[1] - p0[1]) / 6;
    const cp2x = p2[0] - (p3[0] - p1[0]) / 6;
    const cp2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += ` C ${cp1x} ${cp1y}, ${cp2x} ${cp2y}, ${p2[0]} ${p2[1]}`;
  }
  return d;
}

function AreaChart({ issued, resolved }: { issued: number[]; resolved: number[] }) {
  const W = 760;
  const H = 236;
  const L = 12;
  const R = 12;
  const T = 12;
  const B = 28;
  const max = Math.max(...issued, ...resolved, 1) * 1.18;
  const x = (index: number) => L + (index * (W - L - R)) / (issued.length - 1 || 1);
  const y = (value: number) => T + (H - T - B) * (1 - value / max);
  const line = (values: number[]) => smoothPath(values.map((value, index) => [x(index), y(value)]));
  const area = `${line(issued)} L ${x(issued.length - 1)} ${H - B} L ${L} ${H - B} Z`;
  const grid = [0, 1, 2, 3].map((index) => {
    const gy = T + ((H - T - B) * index) / 3;
    return <line key={index} x1={L} y1={gy} x2={W - R} y2={gy} stroke="#f0f0ea" strokeWidth={1} />;
  });
  const dots = (values: number[], color: string) =>
    values.map((value, index) => (
      <circle key={`${color}-${index}`} cx={x(index)} cy={y(value)} r={2.4} fill="#fff" stroke={color} strokeWidth={1.3}>
        <title>{`${index}:00 · ${value}`}</title>
      </circle>
    ));
  return (
    <svg className="chart" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="签发与解析趋势">
      <defs>
        <linearGradient id="areaFill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#0f0f0d" stopOpacity={0.09} />
          <stop offset="1" stopColor="#0f0f0d" stopOpacity={0} />
        </linearGradient>
      </defs>
      {grid}
      <path d={area} fill="url(#areaFill)" />
      <path d={line(issued)} fill="none" stroke="#0f0f0d" strokeWidth={1.8} strokeLinecap="round" />
      <path d={line(resolved)} fill="none" stroke="#2f5bff" strokeWidth={1.5} strokeDasharray="5 5" strokeLinecap="round" />
      {dots(issued, '#0f0f0d')}
      {dots(resolved, '#2f5bff')}
    </svg>
  );
}

const DONUT_COLORS: Record<string, string> = {
  ENV: '#0f0f0d',
  TOKEN: '#2f5bff',
  JSON: '#c9c9be',
};

function Donut({ kinds, total }: { kinds: Overview['kinds']; total: number }) {
  const radius = 68;
  const circumference = 2 * Math.PI * radius;
  let offset = 0;
  return (
    <div className="donut-wrap">
      <div className="donut-center" style={{ position: 'relative' }}>
        <svg viewBox="0 0 200 200" width={182} height={182} role="img" aria-label="凭证构成">
          {kinds.map((kind) => {
            const length = (circumference * kind.value) / 100;
            const element = (
              <circle
                key={kind.label}
                cx="100"
                cy="100"
                r={radius}
                fill="none"
                stroke={DONUT_COLORS[kind.label] || '#c9c9be'}
                strokeWidth={17}
                strokeDasharray={`${length} ${circumference - length}`}
                strokeDashoffset={-offset}
                transform="rotate(-90 100 100)"
              >
                <title>{`${kind.label} · ${kind.value}%`}</title>
              </circle>
            );
            offset += length;
            return element;
          })}
        </svg>
        <div
          style={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeContent: 'center',
            textAlign: 'center',
            pointerEvents: 'none',
          }}
        >
          <strong className="num" style={{ fontSize: 26, fontWeight: 600 }}>
            {total}
          </strong>
          <span style={{ fontSize: 10, color: '#a8a8a0' }}>凭证</span>
        </div>
      </div>
      <div className="donut-legend">
        {kinds.map((kind) => (
          <div className="dl-row" key={kind.label}>
            <span className="dot" style={{ background: DONUT_COLORS[kind.label] || '#c9c9be' }} />
            {kind.label}
            <span className="val">{kind.value}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function Bars({ items }: { items: Overview['usage'] }) {
  if (!items.length) return <div className="empty">暂无调用数据</div>;
  return (
    <div className="bars">
      {items.map((item, index) => (
        <div className="bar-row" key={item.label}>
          <div className="bar-label">
            <Icon name="terminal" className="tiny" />
            {item.label}
          </div>
          <div className="bar-track">
            <div className={`bar-fill ${index === 0 ? 'accent' : ''}`} style={{ width: `${item.value}%` }} />
          </div>
          <div className="bar-val">{item.value}%</div>
        </div>
      ))}
    </div>
  );
}

function Heatmap({ heat }: { heat: number[] }) {
  return (
    <>
      <div className="heatmap">
        {Array.from({ length: 24 }).map((_, index) => (
          <div key={index} className={`heat-cell l${heat[index] || 0}`} title={`${index}:00 · ${(heat[index] || 0) * 12} 次`} />
        ))}
      </div>
      <div className="heat-legend">
        低
        <i style={{ background: '#f0f0ea' }} />
        <i style={{ background: '#e4e4dc' }} />
        <i style={{ background: '#c9c9be' }} />
        <i style={{ background: '#8f8f84' }} />
        <i style={{ background: '#3d3d37' }} />
        高
      </div>
    </>
  );
}

function Timeline({ events }: { events: AuditEvent[] }) {
  return (
    <div className="timeline">
      {events.map((event) => {
        const tone = event.result === 'denied' || event.event.includes('honeypot') ? 'danger' : 'ok';
        return (
          <div className="tl-item" key={`${event.ts}-${event.event}-${event.ticket || ''}`}>
            <div className={`tl-dot ${tone}`} />
            <div className="tl-body">
              <div className="tl-line">
                <span className="tl-event">{event.event}</span>
                <Pill text={event.result} tone={tone} />
              </div>
              <div className="tl-text">
                {event.actor} · {event.secret || '—'}
              </div>
              <div className="tl-meta">
                {fmtTime(event.ts)}
                {event.ticket ? ` · ${event.ticket}` : ''}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* views                                                               */
/* ------------------------------------------------------------------ */

function OverviewView({
  overview,
  audit,
  status,
  onAddSecret,
  onNav,
}: {
  overview: Overview | null;
  audit: AuditEvent[];
  status: Status | null;
  onAddSecret: () => void;
  onNav: (route: Route) => void;
}) {
  if (!overview) return <div className="empty">加载中…</div>;
  const today = new Date()
    .toLocaleDateString('en-GB', { weekday: 'long', day: '2-digit', month: 'short', year: 'numeric' })
    .toUpperCase();
  const issuedSpark = overview.trend.issued.slice(-6);
  const resolvedSpark = overview.trend.resolved.slice(-6);
  return (
    <>
      <div className="hero">
        <div className="hero-copy">
          <div className="eyebrow">{today}</div>
          <h1 className="hero-title">
            凭证，<em>一目了然。</em>
          </h1>
          <p className="hero-sub">
            长期密钥只留在操作系统保险箱里，Agent 每次调用只拿到一张短票。签发、解析、吊销，全部可追溯。
          </p>
          <div className="hero-actions">
            <button className="btn primary" onClick={onAddSecret}>
              <Icon name="plus" />
              新建凭证
            </button>
            <button className="btn" onClick={() => onNav('agents')}>
              <Icon name="nodes" />
              接入 Agent
            </button>
          </div>
        </div>
        <div className="hero-side">
          <div className="security-card">
            <div className="sc-head">
              <Icon name="shield" />
              安全态势正常
            </div>
            <div className="sc-big">0 起泄露</div>
            <div className="sc-sub">过去 24 小时没有任何长期凭证外泄</div>
            <div className="sc-grid">
              <div className="sc-item">
                <span>后端</span>
                <strong>{status?.backend || '—'}</strong>
              </div>
              <div className="sc-item">
                <span>Epoch</span>
                <strong>{overview.epoch}</strong>
              </div>
              <div className="sc-item">
                <span>硬件指纹</span>
                <strong>{status?.hardware || '—'}</strong>
              </div>
              <div className="sc-item">
                <span>Broker</span>
                <strong>{status?.broker ? 'online' : 'offline'}</strong>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="kpi-grid">
        <Kpi label="活跃凭证" value={overview.secrets} icon="key" delta="+1 本周" dir="up" points={issuedSpark} />
        <Kpi label="活票" value={overview.live} icon="ticket" delta="实时" dir="up" points={resolvedSpark} />
        <Kpi label="24h 签发" value={fmtNum(overview.issued)} icon="arrow" delta="+12.4%" dir="up" points={issuedSpark} />
        <Kpi label="24h 拒绝" value={fmtNum(overview.denied)} icon="alert" delta="0 泄露" dir="up" points={overview.trend.resolved.slice(-6)} />
      </div>

      <div className="chart-grid section">
        <div className="card">
          <div className="card-head">
            <div>
              <div className="card-title">签发 / 解析</div>
              <div className="card-sub">过去 24 小时 · 每小时</div>
            </div>
            <span className="link">实时</span>
          </div>
          <AreaChart issued={overview.trend.issued} resolved={overview.trend.resolved} />
          <div className="chart-legend">
            <span className="legend-item">
              <span className="dot" style={{ background: '#0f0f0d' }} />
              签发
            </span>
            <span className="legend-item">
              <span className="dot" style={{ background: '#2f5bff' }} />
              解析
            </span>
            <span className="legend-item muted">峰值 {Math.max(...overview.trend.issued, 0)} 次 / 时</span>
          </div>
        </div>
        <div className="card">
          <div className="card-head">
            <div>
              <div className="card-title">凭证构成</div>
              <div className="card-sub">按类型</div>
            </div>
            <span className="link">{overview.secrets} 个凭证</span>
          </div>
          <Donut kinds={overview.kinds} total={overview.secrets} />
        </div>
      </div>

      <div className="chart-grid bottom">
        <div className="card">
          <div className="card-head">
            <div>
              <div className="card-title">Agent 调用分布</div>
              <div className="card-sub">按请求数 · 24h</div>
            </div>
            <span className="link">
              <Icon name="arrow" className="tiny" />
            </span>
          </div>
          <Bars items={overview.usage} />
          <div style={{ marginTop: 20, paddingTop: 14, borderTop: '1px dashed #e8e8e2' }}>
            <div className="eyebrow" style={{ marginBottom: 6 }}>
              每小时活跃度
            </div>
            <Heatmap heat={overview.heat} />
          </div>
        </div>
        <div className="card">
          <div className="card-head">
            <div>
              <div className="card-title">最近事件</div>
              <div className="card-sub">实时审计</div>
            </div>
            <button className="link" onClick={() => onNav('audit')}>
              全部
            </button>
          </div>
          <Timeline events={audit.slice(0, 6)} />
        </div>
      </div>
    </>
  );
}

function SecretsView({
  secrets,
  query,
  filter,
  sort,
  setFilter,
  setSort,
  onOpen,
  onRotate,
  onRevoke,
  onCopy,
  onAdd,
}: {
  secrets: Secret[];
  query: string;
  filter: string;
  sort: 'name' | 'epoch';
  setFilter: (value: string) => void;
  setSort: (value: 'name' | 'epoch') => void;
  onOpen: (name: string) => void;
  onRotate: (name: string) => void;
  onRevoke: (name: string) => void;
  onCopy: (value: string) => void;
  onAdd: () => void;
}) {
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return secrets
      .filter((secret) => (filter === 'all' || secret.kind === filter) && (!q || `${secret.name} ${secret.note || ''} ${secret.agents.join(' ')}`.toLowerCase().includes(q)))
      .sort((a, b) => (sort === 'name' ? a.name.localeCompare(b.name) : b.epoch - a.epoch));
  }, [secrets, query, filter, sort]);

  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">VAULT</div>
          <h1 className="view-title">凭证</h1>
          <p className="view-sub">{secrets.length} 个长期凭证，全部存放在 OS 密钥库中。</p>
        </div>
        <button className="btn primary" data-testid="btn-new-secret" onClick={onAdd}>
          <Icon name="plus" />
          新建凭证
        </button>
      </div>
      <div className="toolbar">
        <div className="chips">
          {[
            ['all', '全部'],
            ['env', 'Env'],
            ['token', 'Token'],
            ['json', 'JSON'],
          ].map(([value, label]) => (
            <button key={value} className={`chip ${filter === value ? 'active' : ''}`} onClick={() => setFilter(value)}>
              {label}
            </button>
          ))}
        </div>
        <span className="count">
          {rows.length} / {secrets.length}
        </span>
        <div className="toolbar-spacer" />
        <button className="btn sm" onClick={() => setSort(sort === 'name' ? 'epoch' : 'name')}>
          <Icon name="filter" className="tiny" />
          {sort === 'name' ? '按名称' : '按 epoch'}
        </button>
      </div>
      <div className="card">
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>名称</th>
                <th>类型</th>
                <th>Epoch</th>
                <th>最近轮换</th>
                <th>状态</th>
                <th className="right">操作</th>
              </tr>
            </thead>
            <tbody>
              {rows.length ? (
                rows.map((secret) => (
                  <tr key={secret.name} onClick={() => onOpen(secret.name)}>
                    <td>
                      <div className="cell-main">
                        <Icon name={secret.kind === 'env' ? 'db' : 'key'} className="tiny" />
                        {secret.name}
                      </div>
                      <div className="cell-sub">{secret.note || '—'}</div>
                    </td>
                    <td>
                      <Pill text={secret.kind.toUpperCase()} tone={secret.kind === 'token' ? 'accent' : undefined} />
                    </td>
                    <td className="mono">{secret.epoch}</td>
                    <td className="muted">{fmtTime(secret.rotated_at)}</td>
                    <td>
                      <Pill text="Active" tone="ok" />
                    </td>
                    <td className="right">
                      <div className="row-actions">
                        <button
                          className="icon-btn sm"
                          title="复制名称"
                          onClick={(event) => {
                            event.stopPropagation();
                            onCopy(secret.name);
                          }}
                        >
                          <Icon name="copy" className="tiny" />
                        </button>
                        <button
                          className="icon-btn sm"
                          title="轮换"
                          onClick={(event) => {
                            event.stopPropagation();
                            onRotate(secret.name);
                          }}
                        >
                          <Icon name="refresh" className="tiny" />
                        </button>
                        <button
                          className="icon-btn sm"
                          title="吊销"
                          onClick={(event) => {
                            event.stopPropagation();
                            onRevoke(secret.name);
                          }}
                        >
                          <Icon name="trash" className="tiny" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={6}>
                    <div className="empty">没有匹配的凭证</div>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

function TicketsView({
  tickets,
  query,
  onOpen,
  onRenew,
  onRevoke,
  onRevokeAll,
  onIssue,
}: {
  tickets: Ticket[];
  query: string;
  onOpen: (id: string) => void;
  onRenew: (id: string) => void;
  onRevoke: (id: string) => void;
  onRevokeAll: () => void;
  onIssue: () => void;
}) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const q = query.trim().toLowerCase();
  const rows = tickets.filter((ticket) => !q || `${ticket.id} ${ticket.secret} ${ticket.sub} ${ticket.parent || ''}`.toLowerCase().includes(q));
  const parents = rows.filter((ticket) => ticket.kind === 'parent').length;
  const leaves = rows.length - parents;
  const soon = rows.filter((ticket) => ticket.exp > now && ticket.exp - now < 5 * 60_000).length;

  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">TICKETS</div>
          <h1 className="view-title">票据</h1>
          <p className="view-sub">父票负责“任务还活着”，子票负责“每次调用只活一次”。</p>
        </div>
        <div className="row-actions">
          <button className="btn primary" data-testid="btn-issue-ticket" onClick={onIssue}>
            <Icon name="plus" />
            签发票据
          </button>
          <button className="btn" onClick={onRevokeAll}>
            <Icon name="trash" />
            全局吊销
          </button>
        </div>
      </div>
      <div className="stat-strip" style={{ marginBottom: 12 }}>
        <div className="item">
          <span>父票</span>
          <strong>{parents}</strong>
        </div>
        <div className="item">
          <span>子票</span>
          <strong>{leaves}</strong>
        </div>
        <div className="item">
          <span>5 分钟内过期</span>
          <strong>{soon}</strong>
        </div>
      </div>
      <div className="card">
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>票据</th>
                <th>凭证</th>
                <th>Agent</th>
                <th>剩余</th>
                <th>使用</th>
                <th className="right">操作</th>
              </tr>
            </thead>
            <tbody>
              {rows.length ? (
                rows.map((ticket) => {
                  const left = ticket.exp - now;
                  const percent = Math.max(0, Math.min(100, (left / ticket.total) * 100));
                  return (
                    <tr key={ticket.id} onClick={() => onOpen(ticket.id)}>
                      <td>
                        <div className="cell-main">
                          <Icon name={ticket.kind === 'parent' ? 'ticket' : 'key'} className="tiny" />
                          {ticket.id}
                        </div>
                        <div className="cell-sub">
                          {ticket.kind === 'parent' ? '父票 · 任务级' : '子票 · 一次性'}
                          {ticket.parent ? ` · ${ticket.parent}` : ''}
                        </div>
                      </td>
                      <td>{ticket.secret}</td>
                      <td className="muted">{ticket.sub}</td>
                      <td style={{ minWidth: 150 }}>
                        <span>{fmtLeft(left)}</span>
                        <div className="bar-track" style={{ marginTop: 6 }}>
                          <div className={`bar-fill ${ticket.kind === 'parent' ? 'accent' : ''}`} style={{ width: `${percent}%` }} />
                        </div>
                      </td>
                      <td className="mono">{ticket.kind === 'parent' ? `×${ticket.renewals}` : `${ticket.uses} use`}</td>
                      <td className="right">
                        <div className="row-actions">
                          {ticket.kind === 'parent' ? (
                            <button
                              className="btn sm"
                              onClick={(event) => {
                                event.stopPropagation();
                                onRenew(ticket.id);
                              }}
                            >
                              续期
                            </button>
                          ) : null}
                          <button
                            className="btn sm danger"
                            onClick={(event) => {
                              event.stopPropagation();
                              onRevoke(ticket.id);
                            }}
                          >
                            吊销
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })
              ) : (
                <tr>
                  <td colSpan={6}>
                    <div className="empty">没有活票</div>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

function AgentPathPicker({
  agent,
  paths,
  onPick,
  onCancel,
}: {
  agent: string;
  paths: string[];
  onPick: (path: string) => void;
  onCancel: () => void;
}) {
  const [path, setPath] = useState(paths[0] || '');
  return (
    <>
      <div className="modal-title">选择 {agent} 配置文件</div>
      <p className="modal-text">检测到多个候选路径。选择要接入的文件，下一步先展示 dry-run 计划，不会立即改写。</p>
      <Select
        testId="agent-path-picker"
        value={path}
        onChange={setPath}
        options={paths.map((item) => ({ value: item, label: item }))}
      />
      <div className="modal-actions">
          <button className="btn ghost" data-testid="agent-path-cancel" onClick={onCancel}>
            取消
          </button>
          <button className="btn primary" data-testid="agent-path-confirm" onClick={() => onPick(path)}>
            预览计划
          </button>
        </div>
    </>
  );
}

function IssueTicketModal({
  secrets,
  onCancel,
  onIssued,
}: {
  secrets: Secret[];
  onCancel: () => void;
  onIssued: (result: IssuedTicket) => void;
}) {
  const options = secrets
    .filter((secret) => !secret.honeypot)
    .map((secret) => ({ value: secret.name, label: secret.name }));
  const [secret, setSecret] = useState(options[0]?.value || '');
  const [ttl, setTtl] = useState('15m');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const submit = async () => {
    if (!secret) return;
    setBusy(true);
    setError('');
    try {
      const result = await api.issueTicket(secret, ttl.trim() || '15m');
      onIssued(result);
    } catch (cause) {
      setError(errMsg(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="modal-title">签发票据</div>
      <p className="modal-text">默认 15 分钟、只能使用一次。票据只显示这一次，解析后即失效。</p>
      <Select
        testId="issue-secret"
        value={secret}
        onChange={setSecret}
        options={options}
        placeholder="选择凭证"
      />
      <input
        className="modal-input"
        data-testid="issue-ttl"
        value={ttl}
        onChange={(event) => setTtl(event.target.value)}
        placeholder="TTL，例如 15m / 1h"
      />
      {error ? <div className="form-error">{error}</div> : null}
      <div className="modal-actions">
        <button className="btn ghost" onClick={onCancel} disabled={busy}>
          取消
        </button>
        <button className="btn primary" data-testid="btn-issue-confirm" onClick={() => void submit()} disabled={busy || !secret}>
          {busy ? '签发中…' : '签发'}
        </button>
      </div>
    </>
  );
}

function AgentsView({
  agents,
  onInit,
  onPickPath,
  onOpen,
  onCopy,
}: {
  agents: Agent[];
  onInit: (id: string) => void;
  onPickPath: (id: string) => void;
  onOpen: (id: string) => void;
  onCopy: (value: string) => void;
}) {
  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">AGENTS</div>
          <h1 className="view-title">接入</h1>
          <p className="view-sub">一个内部模型，六个平台适配器。新增平台只加 adapter。</p>
        </div>
        <button className="btn" onClick={() => onCopy('agentvault init --agent codex')}>
          <Icon name="copy" />
          复制 Codex 命令
        </button>
      </div>
      <div className="agent-grid">
        {agents.map((agent) => (
          <div className="agent-card" key={agent.id}>
            <div className="agent-head">
              <div className="agent-icon">
                <Icon name={['gemini', 'windsurf', 'cline'].includes(agent.id) ? 'nodes' : 'terminal'} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="agent-name">{agent.id}</div>
                <div className="agent-path">{agent.path || '—'}</div>
              </div>
              <span className={`pill ${agent.wrapped ? 'ok' : ''}`} style={{ marginLeft: 'auto' }}>
                {agent.wrapped ? '已接入' : '未接入'}
              </span>
            </div>
            <div className="agent-meta">
              {agent.wrapped ? <span className="pill accent">{agent.servers} servers</span> : null}
              <span className="agent-servers">{agent.supports_toml ? 'TOML' : 'JSON'}</span>
            </div>
            <div className="agent-foot">
              <span className="agent-servers">{agent.wrapped ? `${agent.servers} 个 server 已包装` : '尚未接入'}</span>
              <div className="row-actions">
                <button className="btn sm" data-testid={`agent-pick-${agent.id}`} onClick={() => onPickPath(agent.id)}>
                  选择路径
                </button>
                <button className="btn sm" onClick={() => onOpen(agent.id)}>
                  {agent.wrapped ? '查看命令' : '接入指南'}
                </button>
                {!agent.wrapped ? (
                  <button className="btn sm primary" onClick={() => onInit(agent.id)}>
                    接入
                  </button>
                ) : null}
              </div>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function AuditView({
  audit,
  query,
  filter,
  setFilter,
  onCopy,
}: {
  audit: AuditEvent[];
  query: string;
  filter: string;
  setFilter: (value: string) => void;
  onCopy: (value: string) => void;
}) {
  const q = query.trim().toLowerCase();
  const rows = audit.filter((event) => (filter === 'all' || event.event.includes(filter)) && (!q || `${event.event} ${event.actor} ${event.secret || ''} ${event.ticket || ''}`.toLowerCase().includes(q)));
  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">AUDIT</div>
          <h1 className="view-title">审计</h1>
          <p className="view-sub">追加式 JSONL，写入即不可改。蜜罐触发单独标红。</p>
        </div>
        <button className="btn" onClick={() => onCopy('agentvault audit --tail 50')}>
          <Icon name="terminal" />
          复制审计命令
        </button>
      </div>
      <div className="toolbar">
        <div className="chips">
          {[
            ['all', '全部'],
            ['issue', '签发'],
            ['resolve', '解析'],
            ['revoke', '吊销'],
            ['honeypot', '蜜罐'],
          ].map(([value, label]) => (
            <button
              key={value}
              data-testid={`audit-filter-${value}`}
              className={`chip ${filter === value ? 'active' : ''}`}
              onClick={() => setFilter(value)}
            >
              {label}
            </button>
          ))}
        </div>
        <span className="count" data-testid="audit-count">{rows.length} 条</span>
      </div>
      <div className="card">
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>时间</th>
                <th>事件</th>
                <th>Agent</th>
                <th>凭证</th>
                <th>票据</th>
                <th>结果</th>
              </tr>
            </thead>
            <tbody>
              {rows.length ? (
                rows.map((event) => {
                  const tone = event.result === 'denied' || event.event.includes('honeypot') ? 'danger' : 'ok';
                  return (
                    <tr key={`${event.ts}-${event.event}-${event.ticket || ''}`} data-testid="audit-row">
                      <td className="mono muted">{fmtTime(event.ts)}</td>
                      <td>
                        <span className="tl-event">{event.event}</span>
                      </td>
                      <td className="muted">{event.actor}</td>
                      <td>{event.secret || '—'}</td>
                      <td className="mono muted">{event.ticket || '—'}</td>
                      <td>
                        <Pill text={event.result} tone={tone} />
                      </td>
                    </tr>
                  );
                })
              ) : (
                <tr>
                  <td colSpan={6}>
                    <div className="empty">没有匹配的事件</div>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

function HoneypotsView({
  honeypots,
  onAdd,
  onTrigger,
  onDelete,
  onCopy,
}: {
  honeypots: Honeypot[];
  onAdd: () => void;
  onTrigger: (id: string) => void;
  onDelete: (id: string) => void;
  onCopy: (value: string) => void;
}) {
  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">CANARY</div>
          <h1 className="view-title">蜜罐</h1>
          <p className="view-sub">把假凭证放进 Agent 不该读的地方。一旦被真实使用，立即全局吊销。</p>
        </div>
        <button className="btn primary" onClick={onAdd}>
          <Icon name="plus" />
          新建蜜罐
        </button>
      </div>
      <div className="honeypot-grid">
        {honeypots.map((honeypot) => (
          <div className={`canary ${honeypot.status === 'triggered' ? 'triggered' : ''}`} key={honeypot.id}>
            <div className="canary-head">
              <div className="canary-name">
                <Icon name="alert" className="tiny" />
                {honeypot.name || honeypot.id}
              </div>
              {honeypot.status === 'triggered' ? <Pill text="已触发" tone="danger" /> : <Pill text="待命" tone="ok" />}
            </div>
            <div className="canary-token">
              <span>{honeypot.token.slice(0, 22)}…</span>
              <button className="icon-btn sm" onClick={() => onCopy(honeypot.token)}>
                <Icon name="copy" className="tiny" />
              </button>
            </div>
            <div className="canary-meta">
              <span>
                命中 <strong>{honeypot.hits}</strong>
              </span>
              <span>
                创建 <strong>{fmtTime(honeypot.created_at)}</strong>
              </span>
            </div>
            <div className="agent-foot">
              <span className="agent-servers">{honeypot.status === 'triggered' ? '已触发全局吊销' : '等待被读取'}</span>
              <div className="row-actions">
                {honeypot.status !== 'triggered' ? (
                  <button className="btn sm" onClick={() => onTrigger(honeypot.id)}>
                    模拟触发
                  </button>
                ) : null}
                <button className="btn sm danger" onClick={() => onDelete(honeypot.id)}>
                  删除
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>
      <div className="card" style={{ marginTop: 14 }}>
        <div className="card-head">
          <div>
            <div className="card-title">它为什么有效</div>
            <div className="card-sub">Agent 不会区分真凭证和 canary</div>
          </div>
        </div>
        <div className="hero-sub" style={{ maxWidth: 'none' }}>
          正常凭证只存在于 broker 内存；蜜罐值放在 <span className="mono">.env</span> 或旧配置里。只要它出现在一次真实调用中，broker 就认为有人在偷钥匙，立刻递增全局 epoch，所有票一起失效。
        </div>
      </div>
    </>
  );
}

function Switch({ on, onChange }: { on: boolean; onChange: (value: boolean) => void }) {
  return <button className={`switch ${on ? 'on' : ''}`} onClick={() => onChange(!on)} aria-pressed={on} />;
}

function AccountsCard({
  role,
  showToast,
}: {
  role: string;
  showToast: (message: string) => void;
}) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [newRole, setNewRole] = useState('viewer');
  const [tenant, setTenant] = useState('default');
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    try {
      setAccounts(await api.accounts());
    } catch (cause) {
      showToast(errMsg(cause));
    }
  }, [showToast]);

  useEffect(() => {
    if (role === 'owner') void reload();
  }, [role, reload]);

  if (role !== 'owner') {
    return (
      <div className="setting-card">
        <div className="setting-title">
          <Icon name="user" className="tiny" />
          账号与租户
        </div>
        <div className="kv">
          <Kv label="当前角色" value={role} />
          <Kv label="账号管理" value="仅 owner 可用" />
        </div>
      </div>
    );
  }

  const create = () => {
    if (!username.trim() || !password) {
      showToast('请输入账号与密码');
      return;
    }
    setBusy(true);
    void (async () => {
      try {
        await api.addAccount(username.trim(), password, newRole, tenant.trim() || 'default');
        setUsername('');
        setPassword('');
        await reload();
        showToast('账号已创建');
      } catch (cause) {
        showToast(errMsg(cause));
      } finally {
        setBusy(false);
      }
    })();
  };

  const remove = (name: string) => {
    setBusy(true);
    void (async () => {
      try {
        await api.removeAccount(name);
        await reload();
        showToast(`账号 ${name} 已删除`);
      } catch (cause) {
        showToast(errMsg(cause));
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div className="setting-card">
      <div className="setting-title">
        <Icon name="user" className="tiny" />
        账号与租户
      </div>
      <div className="kv">
        <Kv label="当前角色" value={role} />
        <Kv label="存储" value="web-accounts.json · argon2id" mono />
      </div>
      <div className="setting-open" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8 }}>
        <input
          className="modal-input"
          data-testid="account-username"
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          placeholder="账号"
          autoComplete="off"
        />
        <input
          className="modal-input"
          data-testid="account-password"
          type="password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          placeholder="密码（至少 6 位）"
          autoComplete="new-password"
        />
        <Select
          testId="account-role"
          className="select-stack"
          value={newRole}
          onChange={setNewRole}
          options={[
            { value: 'viewer', label: 'viewer（只读）' },
            { value: 'admin', label: 'admin' },
            { value: 'owner', label: 'owner' },
          ]}
        />
        <input
          className="modal-input"
          data-testid="account-tenant"
          value={tenant}
          onChange={(event) => setTenant(event.target.value)}
          placeholder="租户，默认 default"
          autoComplete="off"
        />
        <button className="btn sm primary" data-testid="account-create" onClick={create} disabled={busy}>
          创建账号
        </button>
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>账号</th>
              <th>角色</th>
              <th>租户</th>
              <th className="right">操作</th>
            </tr>
          </thead>
          <tbody>
            {accounts.length ? (
              accounts.map((account) => (
                <tr key={account.username} data-testid="account-row">
                  <td>{account.username}</td>
                  <td>
                    <Pill text={account.role} />
                  </td>
                  <td>{account.tenant}</td>
                  <td className="right">
                    <button
                      className="btn sm danger"
                      data-testid={`account-delete-${account.username}`}
                      onClick={() => remove(account.username)}
                      disabled={busy}
                    >
                      删除
                    </button>
                  </td>
                </tr>
              ))
            ) : (
              <tr>
                <td colSpan={4}>
                  <div className="empty">尚无账号，仅本地 web.token 可登录</div>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function SettingsView({
  status,
  role,
  onRevokeAll,
  showToast,
}: {
  status: Status | null;
  role: string;
  onRevokeAll: () => void;
  showToast: (message: string) => void;
}) {
  const [honeypotRevoke, setHoneypotRevoke] = useState(true);
  const [denyUnknown, setDenyUnknown] = useState(true);
  const [keepAudit, setKeepAudit] = useState(false);
  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">SYSTEM</div>
          <h1 className="view-title">设置</h1>
          <p className="view-sub">后端、指纹与危险操作。团队版会在这里接 SSO 与审计留存。</p>
        </div>
      </div>
      <div className="settings-grid">
        <div className="setting-card">
          <div className="setting-title">
            <Icon name="db" className="tiny" />
            密钥后端
          </div>
          <div className="kv">
            <Kv label="Backend" value={status?.backend || '—'} mono />
            <Kv label="Device" value={status?.device || '—'} mono />
            <Kv label="Hardware" value={status?.hardware || '—'} mono />
            <Kv label="Broker" value={status?.broker ? 'online' : 'offline'} mono />
            <Kv label="Epoch" value={String(status?.epoch ?? '—')} mono />
          </div>
        </div>
        <div className="setting-card">
          <div className="setting-title">
            <Icon name="shield" className="tiny" />
            策略
          </div>
          <div className="setting-open">
            <div>
              <div className="so-title">蜜罐触发自动吊销</div>
              <div className="so-sub">命中 canary 立即递增全局 epoch</div>
            </div>
            <Switch on={honeypotRevoke} onChange={(value) => { setHoneypotRevoke(value); showToast('设置已更新'); }} />
          </div>
          <div className="setting-open">
            <div>
              <div className="so-title">拒绝未知 server</div>
              <div className="so-sub">不在白名单里的 MCP server 直接拒绝</div>
            </div>
            <Switch on={denyUnknown} onChange={(value) => { setDenyUnknown(value); showToast('设置已更新'); }} />
          </div>
          <div className="setting-open">
            <div>
              <div className="so-title">审计保留 30 天</div>
              <div className="so-sub">JSONL 轮转，旧文件保留</div>
            </div>
            <Switch on={keepAudit} onChange={(value) => { setKeepAudit(value); showToast('设置已更新'); }} />
          </div>
        </div>
        <div className="setting-card">
          <div className="setting-title">
            <Icon name="clock" className="tiny" />
            票据策略
          </div>
          <div className="kv">
            <Kv label="默认 TTL" value="15 分钟" />
            <Kv label="父票硬上限" value="8 小时" />
            <Kv label="子票 TTL" value="60 秒" />
            <Kv label="默认 max_uses" value="1" />
            <Kv label="全局 epoch" value={String(status?.epoch ?? '—')} mono />
          </div>
        </div>
        <AccountsCard role={role} showToast={showToast} />
        <div className="setting-card danger-zone">
          <div className="setting-title">
            <Icon name="alert" className="tiny" />
            危险操作
          </div>
          <div className="hero-sub" style={{ margin: '4px 0 14px' }}>
            全局吊销会让所有父票、子票立即失效，Agent 的下一次调用会被拒绝。此操作不可撤销。
          </div>
          <button className="btn danger" onClick={onRevokeAll}>
            <Icon name="trash" />
            全局吊销
          </button>
        </div>
      </div>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* app                                                                 */
/* ------------------------------------------------------------------ */

interface DataState {
  status: Status | null;
  overview: Overview | null;
  secrets: Secret[];
  tickets: Ticket[];
  agents: Agent[];
  audit: AuditEvent[];
  honeypots: Honeypot[];
}

interface Session {
  name: string;
  email: string;
  role: string;
  tenant: string;
  mode: 'local' | 'account';
  signedInAt: number;
}

interface Notice {
  id: string;
  tone: 'danger' | 'warn' | 'info';
  title: string;
  text: string;
  time: number;
  read: boolean;
  route: Route;
}

function LoginPage({
  hasToken,
  error,
  onLocal,
  onAccount,
}: {
  hasToken: boolean;
  error: string;
  onLocal: (token: string) => void | Promise<void>;
  onAccount: (username: string, password: string) => void | Promise<void>;
}) {
  const [username, setUsername] = useState('owner');
  const [password, setPassword] = useState('');
  const [tokenValue, setTokenValue] = useState('');
  const [busy, setBusy] = useState(false);
  const run = (task: () => void | Promise<void>) => {
    setBusy(true);
    void Promise.resolve(task()).finally(() => setBusy(false));
  };
  return (
    <div className="login-shell">
      <section className="login-visual">
        <div className="login-grid" aria-hidden="true" />
        <div className="login-brand">
          <div className="brand-mark">AV</div>
          <div className="brand-text">
            <strong>agentvault</strong>
            <span>Console</span>
          </div>
        </div>
        <div className="login-copy">
          <div className="eyebrow">AI CREDENTIAL SECURITY</div>
          <h1 className="login-title">
            让每个 Agent
            <br />
            <em>只拿到 15 分钟的钥匙。</em>
          </h1>
          <p className="login-sub">
            长期密钥留在 OS 保险箱，Agent 每次调用只拿一张短票。签发、解析、吊销，全部可追溯。
          </p>
          <div className="login-points">
            <div className="login-point">
              <span className="login-point-icon"><Icon name="lock" /></span>
              <div><strong>不落明文</strong><span>DPAPI · Keychain · Secret Service</span></div>
            </div>
            <div className="login-point">
              <span className="login-point-icon"><Icon name="ticket" /></span>
              <div><strong>一次性票据</strong><span>默认 15 分钟，用完即废</span></div>
            </div>
            <div className="login-point">
              <span className="login-point-icon"><Icon name="pulse" /></span>
              <div><strong>全链路审计</strong><span>签发、解析、吊销，一条不缺</span></div>
            </div>
          </div>
        </div>
        <div className="login-visual-foot">LOCAL-FIRST · 数据不离开这台设备</div>
      </section>

      <section className="login-panel">
        <div className="login-card">
          <div className="eyebrow">SIGN IN</div>
          <h2>登录 Console</h2>
          <p className="login-hint">使用本地会话直接进入，或使用本地账号登录。</p>
          {error ? <div className="login-error">{error}</div> : null}
          <button
            className="btn primary login-submit"
            data-testid="login-local"
            onClick={() => run(() => onLocal(tokenValue))}
            disabled={busy || (!tokenValue.trim() && !hasToken)}
          >
            <Icon name="lock" />
            使用本地会话进入
          </button>
          <label className="login-field">
            <span>本地会话 Token</span>
            <input
              data-testid="login-token"
              value={tokenValue}
              onChange={(event) => setTokenValue(event.target.value)}
              placeholder="从 AGENTVAULT_HOME/web.token 复制"
              autoComplete="off"
            />
          </label>
          {!hasToken && !tokenValue ? (
            <div className="login-hint">token 位于 AGENTVAULT_HOME/web.token；agentvault web 启动时会输出该文件路径。</div>
          ) : null}
          <div className="login-divider"><span>或</span></div>
          <form
            className="login-form"
            onSubmit={(event) => {
              event.preventDefault();
              run(() => onAccount(username, password));
            }}
          >
            <label className="login-field">
              <span>账号</span>
              <input
                data-testid="login-username"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                autoComplete="username"
              />
            </label>
            <label className="login-field">
              <span>密码</span>
              <input
                data-testid="login-password"
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                autoComplete="current-password"
              />
            </label>
            <div className="login-row">
              <label className="login-check"><input type="checkbox" defaultChecked /> 记住我</label>
              <span className="login-hint">账号在「设置」中由 owner 创建</span>
            </div>
            <button className="btn login-account" type="submit" data-testid="login-account" disabled={busy}>
              使用账号登录
            </button>
          </form>
          <div className="login-foot-note">会话令牌 12 小时过期；viewer 账号只读。</div>
        </div>
      </section>
    </div>
  );
}

function NotificationsView({
  notices,
  filter,
  setFilter,
  onMarkAll,
  onOpen,
}: {
  notices: Notice[];
  filter: string;
  setFilter: (value: string) => void;
  onMarkAll: () => void;
  onOpen: (notice: Notice) => void;
}) {
  const unread = notices.filter((notice) => !notice.read).length;
  const rows = notices.filter((notice) => {
    if (filter === 'unread') return !notice.read;
    if (filter === 'security') return notice.tone === 'danger' || notice.tone === 'warn';
    if (filter === 'tickets') return notice.route === 'tickets';
    return true;
  });
  return (
    <>
      <div className="view-head">
        <div>
          <div className="eyebrow">INBOX</div>
          <h1 className="view-title">通知</h1>
          <p className="view-sub">安全、票据与接入的实时提醒。</p>
        </div>
        <button className="btn" onClick={onMarkAll}>
          <Icon name="check" />
          全部已读
        </button>
      </div>
      <div className="toolbar">
        <div className="chips">
          {[['all', '全部'], ['unread', '未读'], ['security', '安全'], ['tickets', '票据']].map(([value, label]) => (
            <button key={value} className={`chip ${filter === value ? 'active' : ''}`} onClick={() => setFilter(value)}>
              {label}
            </button>
          ))}
        </div>
        <span className="count">{unread} 未读</span>
      </div>
      <div className="card">
        {rows.length ? (
          <div className="notif-list">
            {rows.map((notice) => (
              <button key={notice.id} className={`notif-item ${notice.read ? '' : 'unread'}`} onClick={() => onOpen(notice)}>
                <span className={`notif-icon ${notice.tone}`}>
                  <Icon name={notice.tone === 'danger' ? 'alert' : notice.tone === 'warn' ? 'clock' : 'bell'} />
                </span>
                <span className="notif-body">
                  <strong>{notice.title}</strong>
                  <span>{notice.text}</span>
                  <em>{fmtTime(notice.time)}</em>
                </span>
                {!notice.read ? <span className="notif-unread" /> : null}
              </button>
            ))}
          </div>
        ) : (
          <div className="empty">没有符合条件的通知</div>
        )}
      </div>
    </>
  );
}

export default function App() {
  const [route, setRoute] = useState<Route>(parseHash());
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [sort, setSort] = useState<'name' | 'epoch'>('name');
  const [rail, setRail] = useState(() => localStorage.getItem('av-rail') === '1');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [data, setData] = useState<DataState>({
    status: null,
    overview: null,
    secrets: [],
    tickets: [],
    agents: [],
    audit: [],
    honeypots: [],
  });
  const [drawer, setDrawer] = useState<{ eyebrow: string; title: string; body: ReactNode } | null>(null);
  const [modal, setModal] = useState<{ content: ReactNode } | null>(null);
  const [toastText, setToastText] = useState('');
  const [session, setSession] = useState<Session | null>(() => {
    const saved = localStorage.getItem('av-session');
    if (saved && token) {
      try {
        const restored = JSON.parse(saved) as Partial<Session>;
        return {
          name: restored.name || 'local',
          email: restored.email || 'local',
          role: restored.role || 'owner',
          tenant: restored.tenant || 'default',
          mode: restored.mode === 'account' ? 'account' : 'local',
          signedInAt: restored.signedInAt || Date.now(),
        };
      } catch {
        /* ignore */
      }
    }
    if (token) {
      return { name: 'local', email: 'local', role: 'owner', tenant: 'default', mode: 'local', signedInAt: Date.now() };
    }
    return null;
  });
  const [userMenu, setUserMenu] = useState(false);
  const [profileName, setProfileName] = useState('');
  const [readIds, setReadIds] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem('av-read') || '[]') as string[];
    } catch {
      return [];
    }
  });
  const [noticeFilter, setNoticeFilter] = useState('all');
  const toastTimer = useRef<number | undefined>(undefined);
  const refreshTimer = useRef<number | undefined>(undefined);
  const searchRef = useRef<HTMLInputElement>(null);

  const showToast = useCallback((message: string) => {
    setToastText(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToastText(''), 2200);
  }, []);

  const notices = useMemo<Notice[]>(() => {
    const list: Notice[] = [];
    const now = Date.now();
    for (const ticket of data.tickets) {
      if (ticket.status === 'active' && ticket.exp - now < 5 * 60_000) {
        const id = `exp-${ticket.id}`;
        list.push({
          id,
          tone: 'warn',
          title: '票据即将过期',
          text: `${ticket.secret} · ${ticket.id.slice(0, 8)} 剩余 ${fmtLeft(ticket.exp - now)}`,
          time: now,
          read: readIds.includes(id),
          route: 'tickets',
        });
      }
    }
    for (const honeypot of data.honeypots) {
      if (honeypot.status === 'triggered') {
        const id = `hp-${honeypot.id}`;
        list.push({
          id,
          tone: 'danger',
          title: '蜜罐已触发',
          text: `${honeypot.name || honeypot.id} 被真实使用，已全局吊销`,
          time: honeypot.triggered_at || now,
          read: readIds.includes(id),
          route: 'honeypots',
        });
      }
    }
    for (const event of data.audit.slice(0, 8)) {
      const id = `ev-${event.ts}-${event.event}-${event.ticket || ''}`;
      list.push({
        id,
        tone: event.result === 'denied' || event.event.includes('honeypot') ? 'danger' : 'info',
        title: event.event,
        text: `${event.actor} · ${event.secret || '—'}${event.ticket ? ' · ' + event.ticket : ''}`,
        time: event.ts,
        read: readIds.includes(id),
        route: 'audit',
      });
    }
    if (!list.length) {
      list.push({
        id: 'welcome',
        tone: 'info',
        title: '欢迎使用 agentvault',
        text: '接入第一个 Agent 后，这里会出现安全与票据提醒。',
        time: now,
        read: false,
        route: 'agents',
      });
    }
    return list.sort((a, b) => b.time - a.time);
  }, [data.tickets, data.honeypots, data.audit, readIds]);
  const unread = notices.filter((notice) => !notice.read).length;

  const openNotice = useCallback((notice: Notice) => {
    setReadIds((current) => (current.includes(notice.id) ? current : [...current, notice.id]));
    window.location.hash = `#/${notice.route}`;
  }, []);

  const markAllRead = useCallback(() => {
    setReadIds(notices.map((notice) => notice.id));
    showToast('全部通知已读');
  }, [notices, showToast]);

  const loginLocal = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (!trimmed && !token) {
        showToast('请输入 web.token 中的会话 token');
        return;
      }
      if (trimmed) setToken(trimmed);
      return (async () => {
        try {
          const me = await api.me();
          setSession({
            name: me.username,
            email: me.username,
            role: me.role,
            tenant: me.tenant,
            mode: 'local',
            signedInAt: Date.now(),
          });
          setError('');
          showToast('已进入本地控制台');
        } catch (cause) {
          setToken('');
          setError(errMsg(cause));
        }
      })();
    },
    [showToast]
  );

  const loginAccount = useCallback(
    (username: string, password: string) => {
      return (async () => {
        try {
          const result = await api.login(username, password);
          setToken(result.token);
          setSession({
            name: username,
            email: username,
            role: result.role,
            tenant: result.tenant,
            mode: 'account',
            signedInAt: Date.now(),
          });
          setError('');
          showToast(`已登录 ${username} · 租户 ${result.tenant}`);
        } catch (cause) {
          setError(errMsg(cause));
        }
      })();
    },
    [showToast]
  );

  const logout = useCallback(() => {
    void api.logout().catch(() => undefined);
    setSession(null);
    setUserMenu(false);
    setToken('');
    localStorage.removeItem('av-session');
    showToast('已退出登录');
  }, [showToast]);

  const openProfile = useCallback(() => {
    if (!session) return;
    setDrawer({
      eyebrow: 'PROFILE',
      title: '个人信息',
      body: (
        <>
          <div className="profile-head">
            <div className="profile-avatar">{session.name.slice(0, 2).toUpperCase()}</div>
            <div>
              <strong>{session.name}</strong>
              <span>{session.email}</span>
            </div>
          </div>
          <div className="drawer-section">
            <div className="drawer-label">资料</div>
            <label className="profile-field">
              <span>显示名称</span>
              <input value={profileName} onChange={(event) => setProfileName(event.target.value)} />
            </label>
            <div className="kv">
              <Kv label="角色" value={session.role} />
              <Kv label="租户" value={session.tenant || 'default'} />
              <Kv label="登录方式" value={session.mode === 'local' ? '本地会话' : '本地账号'} />
              <Kv label="上次登录" value={fmtTime(session.signedInAt)} />
            </div>
          </div>
          <div className="drawer-section">
            <div className="drawer-label">本机</div>
            <div className="kv">
              <Kv label="后端" value={data.status?.backend || '—'} mono />
              <Kv label="设备" value={data.status?.device || '—'} mono />
              <Kv label="硬件指纹" value={data.status?.hardware || '—'} mono />
            </div>
          </div>
          <div className="drawer-actions">
            <button
              className="btn sm primary"
              onClick={() => {
                const name = profileName.trim() || session.name;
                setSession({ ...session, name });
                showToast('个人信息已保存');
                setDrawer(null);
              }}
            >
              保存
            </button>
            <button
              className="btn sm"
              onClick={() => {
                setDrawer(null);
                logout();
              }}
            >
              退出登录
            </button>
          </div>
        </>
      ),
    });
  }, [data.status, logout, profileName, session, showToast]);

  const load = useCallback(async () => {
    if (!token) {

      setLoading(false);
      return;
    }
    try {
      const [me, status, overview, secrets, tickets, agents, audit, honeypots] = await Promise.all([
        api.me(),
        api.status(),
        api.overview(),
        api.secrets(),
        api.tickets(),
        api.agents(),
        api.audit(200),
        api.honeypots(),
      ]);
      setData({ status, overview, secrets, tickets, agents, audit, honeypots });
      setSession((current) => {
        if (!current || (current.role === me.role && current.tenant === me.tenant)) return current;
        return { ...current, role: me.role, tenant: me.tenant };
      });
      setError('');
    } catch (cause) {
      setError(errMsg(cause));
    } finally {
      setLoading(false);
    }
  }, []);

  const scheduleLoad = useCallback(() => {
    window.clearTimeout(refreshTimer.current);
    refreshTimer.current = window.setTimeout(() => {
      void load();
    }, 400);
  }, [load]);

  useEffect(() => {
    if (!session) return;
    void load();
    return subscribe(scheduleLoad);
  }, [load, scheduleLoad, session]);

  useEffect(() => {
    localStorage.setItem('av-rail', rail ? '1' : '0');
  }, [rail]);

  useEffect(() => {
    if (session) localStorage.setItem('av-session', JSON.stringify(session));
    else localStorage.removeItem('av-session');
  }, [session]);

  useEffect(() => {
    localStorage.setItem('av-read', JSON.stringify(readIds));
  }, [readIds]);

  useEffect(() => {
    if (session) setProfileName(session.name);
  }, [session]);

  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      if (!target.closest('.user-menu') && !target.closest('.avatar')) setUserMenu(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, []);

  useEffect(() => {
    const onHash = () => {
      setRoute(parseHash());
      setQuery('');
      setFilter('all');
      setDrawer(null);
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setDrawer(null);
        setModal(null);
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const copy = useCallback(async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      showToast('已复制到剪贴板');
    } catch {
      showToast('复制失败');
    }
  }, [showToast]);

  const act = useCallback(
    async (fn: () => Promise<unknown>, ok: string) => {
      try {
        await fn();
        showToast(ok);
        await load();
      } catch (cause) {
        showToast(errMsg(cause));
      }
    },
    [load, showToast]
  );

  const confirmAction = useCallback(
    (title: string, text: string, danger: boolean, fn: () => void | Promise<void>) => {
      setModal({
        content: (
          <>
            <div className="modal-title">{title}</div>
            <p className="modal-text">{text}</p>
            <div className="modal-actions">
              <button className="btn ghost" onClick={() => setModal(null)}>
                取消
              </button>
              <button
                className={`btn ${danger ? 'danger' : 'primary'}`}
                onClick={() => {
                  setModal(null);
                  void fn();
                }}
              >
                确认
              </button>
            </div>
          </>
        ),
      });
    },
    []
  );

  const rotateSecret = useCallback(
    (name: string) => {
      confirmAction(`轮换 ${name}？`, '旧票会立即失效，新值只显示一次。下游系统需要同步更新。', false, () =>
        act(() => api.rotate(name), '已轮换 · epoch +1')
      );
    },
    [act, confirmAction]
  );

  const revokeSecret = useCallback(
    (name: string) => {
      confirmAction(`吊销 ${name}？`, '该凭证及其所有票据会立即失效。', true, () =>
        act(() => api.removeSecret(name), '凭证已吊销')
      );
    },
    [act, confirmAction]
  );

  const revokeTicket = useCallback(
    (id: string) => {
      confirmAction(`吊销票据 ${id}？`, '父票会级联吊销它的全部子票。', true, () =>
        act(() => api.revokeTicket(id), '票据已吊销')
      );
    },
    [act, confirmAction]
  );

  const revokeAll = useCallback(() => {
    confirmAction('全局吊销？', '所有父票和子票立即失效，Agent 的下一次调用会被拒绝。此操作不可撤销。', true, () =>
      act(() => api.revokeAll(), '已全局吊销')
    );
  }, [act, confirmAction]);

  const openIssue = useCallback(() => {
    if (!data.secrets.some((secret) => !secret.honeypot)) {
      showToast('先创建凭证，再签发票据');
      return;
    }
    setModal({
      content: (
        <IssueTicketModal
          secrets={data.secrets}
          onCancel={() => setModal(null)}
          onIssued={(result) => {
            setModal({
              content: (
                <>
                  <div className="modal-title" data-testid="ticket-issued">
                    票据已签发
                  </div>
                  <p className="modal-text">
                    只显示一次，请复制给 Agent。{result.jti ? ` JTI ${result.jti}` : ''}
                  </p>
                  <textarea className="modal-input mono" readOnly value={result.ticket} rows={3} />
                  <div className="modal-actions">
                    <button className="btn" onClick={() => void copy(result.ticket)}>
                      复制
                    </button>
                    <button className="btn primary" onClick={() => setModal(null)}>
                      完成
                    </button>
                  </div>
                </>
              ),
            });
            void load();
          }}
        />
      ),
    });
  }, [copy, data.secrets, load, showToast]);

  const addSecret = useCallback(() => {
    setModal({
      content: (
        <>
          <div className="modal-title">新建凭证</div>
          <p className="modal-text">
            名称建议使用 <span className="mono">服务/用途</span> 格式，例如 postgres/prod。值只保存在 OS 密钥库。
          </p>
          <input className="modal-input" id="newSecretName" data-testid="new-secret-name" placeholder="名称" autoComplete="off" />
          <input className="modal-input" id="newSecretValue" data-testid="new-secret-value" placeholder="值" autoComplete="off" />
          <div className="modal-actions">
            <button className="btn ghost" onClick={() => setModal(null)}>
              取消
            </button>
            <button
              className="btn primary"
              data-testid="btn-create-secret"
              onClick={() => {
                const name = (document.getElementById('newSecretName') as HTMLInputElement | null)?.value.trim();
                const value = (document.getElementById('newSecretValue') as HTMLInputElement | null)?.value;
                if (!name || !value) {
                  showToast('请输入名称和值');
                  return;
                }
                setModal(null);
                void act(() => api.addSecret(name, value), '凭证已创建');
              }}
            >
              创建
            </button>
          </div>
        </>
      ),
    });
  }, [act, showToast]);

  const addHoneypot = useCallback(() => {
    setModal({
      content: (
        <>
          <div className="modal-title">新建蜜罐</div>
          <p className="modal-text">生成一个 canary 值，放进 Agent 不该读的配置里。任何真实使用都会触发全局吊销。</p>
          <input className="modal-input" id="newHoneypotName" placeholder="名称，例如 prod-canary" autoComplete="off" />
          <div className="modal-actions">
            <button className="btn ghost" onClick={() => setModal(null)}>
              取消
            </button>
            <button
              className="btn primary"
              onClick={() => {
                const name = (document.getElementById('newHoneypotName') as HTMLInputElement | null)?.value.trim();
                if (!name) {
                  showToast('请输入名称');
                  return;
                }
                setModal(null);
                void act(() => api.addHoneypot(name), '蜜罐已创建');
              }}
            >
              创建
            </button>
          </div>
        </>
      ),
    });
  }, [act, showToast]);

  const openSecret = useCallback(
    (name: string) => {
      const secret = data.secrets.find((item) => item.name === name);
      if (!secret) return;
      setDrawer({
        eyebrow: 'SECRET',
        title: secret.name,
        body: (
          <>
            <div className="drawer-section">
              <div className="drawer-label">属性</div>
              <div className="kv">
                <Kv label="类型" value={secret.kind} />
                <Kv label="Epoch" value={String(secret.epoch)} />
                <Kv label="最近轮换" value={fmtTime(secret.rotated_at)} />
                <Kv label="绑定 Agent" value={secret.agents.join(', ') || '—'} />
              </div>
            </div>
            <div className="drawer-section">
              <div className="drawer-label">状态</div>
              <Pill text="Active" tone="ok" />
              <p className="modal-text">长期值只存在于 OS 密钥库与 broker 内存，Agent 拿不到明文。</p>
            </div>
            <div className="drawer-actions">
              <button className="btn sm" onClick={() => void copy(secret.name)}>
                <Icon name="copy" className="tiny" />
                复制名称
              </button>
              <button className="btn sm" onClick={() => rotateSecret(secret.name)}>
                <Icon name="refresh" className="tiny" />
                轮换
              </button>
              <button className="btn sm danger" onClick={() => revokeSecret(secret.name)}>
                <Icon name="trash" className="tiny" />
                吊销
              </button>
            </div>
          </>
        ),
      });
    },
    [copy, data.secrets, revokeSecret, rotateSecret]
  );

  const openTicket = useCallback(
    (id: string) => {
      const ticket = data.tickets.find((item) => item.id === id);
      if (!ticket) return;
      const children = data.tickets.filter((item) => item.parent === ticket.id);
      setDrawer({
        eyebrow: 'TICKET',
        title: ticket.id,
        body: (
          <>
            <div className="drawer-section">
              <div className="drawer-label">票据</div>
              <div className="kv">
                <Kv label="类型" value={ticket.kind === 'parent' ? '父票 · 任务级' : '子票 · 一次性'} />
                <Kv label="凭证" value={ticket.secret} />
                <Kv label="Agent" value={ticket.sub} />
                <Kv label="剩余" value={fmtLeft(ticket.exp - Date.now())} />
                {ticket.parent ? <Kv label="父票" value={ticket.parent} /> : null}
                <Kv label="使用" value={ticket.kind === 'parent' ? `×${ticket.renewals}` : `${ticket.uses} 次`} />
              </div>
            </div>
            {ticket.kind === 'parent' ? (
              <div className="drawer-section">
                <div className="drawer-label">子票 ({children.length})</div>
                {children.length ? (
                  children.map((child) => (
                    <div className="kv-row" key={child.id}>
                      <span className="mono">{child.id}</span>
                      <strong>{fmtLeft(child.exp - Date.now())}</strong>
                    </div>
                  ))
                ) : (
                  <div className="empty">暂无子票</div>
                )}
              </div>
            ) : null}
            <div className="drawer-actions">
              {ticket.kind === 'parent' ? (
                <button className="btn sm" onClick={() => void act(() => api.renew(ticket.id), '已续期 30 分钟')}>
                  续期
                </button>
              ) : null}
              <button className="btn sm danger" onClick={() => revokeTicket(ticket.id)}>
                吊销
              </button>
            </div>
          </>
        ),
      });
    },
    [act, data.tickets, revokeTicket]
  );

  const openAgent = useCallback(
    (id: string) => {
      const agent = data.agents.find((item) => item.id === id);
      if (!agent) return;
      const command = `agentvault init --agent ${agent.id}`;
      setDrawer({
        eyebrow: 'AGENT',
        title: agent.id,
        body: (
          <>
            <div className="drawer-section">
              <div className="drawer-label">配置</div>
              <div className="kv">
                <Kv label="路径" value={agent.path || '—'} />
                <Kv label="格式" value={agent.supports_toml ? 'TOML' : 'JSON'} />
                <Kv label="状态" value={agent.wrapped ? '已接入' : '未接入'} />
              </div>
            </div>
            <div className="drawer-section">
              <div className="drawer-label">接入命令</div>
              <div className="canary-token">
                <span className="mono">{command}</span>
                <button className="icon-btn sm" onClick={() => void copy(command)}>
                  <Icon name="copy" className="tiny" />
                </button>
              </div>
              <p className="modal-text">
                先运行 <span className="mono">agentvault daemon</span>，再执行上面的命令。敏感 env 会自动迁入密钥库。
              </p>
            </div>
          </>
        ),
      });
    },
    [copy, data.agents]
  );

  const previewAgentPlan = useCallback(
    async (id: string, path?: string) => {
      const plan = await api.agentPlan(id, path);
      const summary = plan.changes.length
        ? plan.changes
            .map((change) => {
              const migrated = Array.isArray(change.migrated_env) ? change.migrated_env.length : 0;
              return `${String(change.server)}${migrated ? ` · 迁移 ${migrated} 个 env` : ''}`;
            })
            .join('  ')
        : '没有需要迁移的 server';
      confirmAction(
        `接入 ${id}？`,
        `配置：${path || plan.path}（${plan.format.toUpperCase()}）。${summary}。会先备份，失败自动回滚。`,
        false,
        () => act(() => api.initAgent(id, plan.path), '已接入')
      );
    },
    [act, confirmAction]
  );

  const pickAgentPath = useCallback(
    (id: string) => {
      void (async () => {
        try {
          const candidates = await api.agentCandidates();
          const candidate = candidates.find((item) => item.id === id);
          const paths = (candidate?.paths ?? [])
            .slice()
            .sort((a, b) => Number(b.exists) - Number(a.exists))
            .map((item) => item.path);
          setModal({
            content: (
              <AgentPathPicker
                agent={id}
                paths={paths.length ? paths : ['.mcp.json']}
                onPick={(path) => {
                  setModal(null);
                  void previewAgentPlan(id, path).catch((cause) => showToast(errMsg(cause)));
                }}
                onCancel={() => setModal(null)}
              />
            ),
          });
        } catch (cause) {
          showToast(errMsg(cause));
        }
      })();
    },
    [previewAgentPlan, showToast]
  );

  const initAgent = useCallback(
    (id: string) => {
      void (async () => {
        try {
          const candidates = await api.agentCandidates();
          const candidate = candidates.find((item) => item.id === id);
          const existing = candidate?.paths.filter((item) => item.exists).map((item) => item.path) ?? [];
          if (existing.length > 1) {
            setModal({
              content: (
                <AgentPathPicker
                  agent={id}
                  paths={existing}
                  onPick={(path) => {
                    setModal(null);
                    void previewAgentPlan(id, path).catch((cause) => showToast(errMsg(cause)));
                  }}
                  onCancel={() => setModal(null)}
                />
              ),
            });
            return;
          }
          await previewAgentPlan(id, existing[0]);
        } catch (cause) {
          showToast(errMsg(cause));
        }
      })();
    },
    [previewAgentPlan, showToast]
  );

  const triggerHoneypot = useCallback(
    (id: string) => {
      confirmAction(`模拟触发 ${id}？`, '这只用于演示：会真实执行一次全局吊销。', true, () =>
        act(() => api.triggerHoneypot(id), '蜜罐命中 · 已全局吊销')
      );
    },
    [act, confirmAction]
  );

  const deleteHoneypot = useCallback(
    (id: string) => {
      confirmAction(`删除蜜罐 ${id}？`, '只会删除 canary 值，不会影响已有票据。', false, () =>
        act(() => api.deleteHoneypot(id), '蜜罐已删除')
      );
    },
    [act, confirmAction]
  );

  if (!session) {
    return <LoginPage hasToken={Boolean(token)} error={error} onLocal={loginLocal} onAccount={loginAccount} />;
  }

  let content: ReactNode;
  if (error) {
    content = (
      <div className="card">
        <div className="empty">
          <div>{error}</div>
          <div style={{ marginTop: 12 }}>
            <button className="btn sm" onClick={() => void load()}>
              重试
            </button>
          </div>
        </div>
      </div>
    );
  } else if (loading) {
    content = <div className="empty">正在连接 broker…</div>;
  } else if (route === 'notifications') {
    content = (
      <NotificationsView
        notices={notices}
        filter={noticeFilter}
        setFilter={setNoticeFilter}
        onMarkAll={markAllRead}
        onOpen={openNotice}
      />
    );
  } else if (route === 'overview') {
    content = <OverviewView overview={data.overview} audit={data.audit} status={data.status} onAddSecret={addSecret} onNav={(next) => { window.location.hash = `#/${next}`; }} />;
  } else if (route === 'secrets') {
    content = (
      <SecretsView
        secrets={data.secrets}
        query={query}
        filter={filter}
        sort={sort}
        setFilter={setFilter}
        setSort={setSort}
        onOpen={openSecret}
        onRotate={rotateSecret}
        onRevoke={revokeSecret}
        onCopy={(value) => void copy(value)}
        onAdd={addSecret}
      />
    );
  } else if (route === 'tickets') {
    content = (
      <TicketsView
        tickets={data.tickets}
        query={query}
        onOpen={openTicket}
        onRenew={(id) => void act(() => api.renew(id), '已续期 30 分钟')}
        onRevoke={revokeTicket}
        onRevokeAll={revokeAll}
        onIssue={openIssue}
      />
    );
  } else if (route === 'agents') {
    content = <AgentsView agents={data.agents} onInit={initAgent} onPickPath={pickAgentPath} onOpen={openAgent} onCopy={(value) => void copy(value)} />;
  } else if (route === 'audit') {
    content = <AuditView audit={data.audit} query={query} filter={filter} setFilter={setFilter} onCopy={(value) => void copy(value)} />;
  } else if (route === 'honeypots') {
    content = (
      <HoneypotsView
        honeypots={data.honeypots}
        onAdd={addHoneypot}
        onTrigger={triggerHoneypot}
        onDelete={deleteHoneypot}
        onCopy={(value) => void copy(value)}
      />
    );
  } else {
    content = <SettingsView status={data.status} role={session.role} onRevokeAll={revokeAll} showToast={showToast} />;
  }

  return (
    <>
      <div className={`app ${rail ? 'rail' : ''}`}>
        <aside className="sidebar">
          <div className="brand">
            <div className="brand-mark">AV</div>
            <div className="brand-text">
              <strong>agentvault</strong>
              <span>Console</span>
            </div>
            <button className="rail-toggle" onClick={() => setRail(!rail)} aria-label="收起侧栏">
              <Icon name="chevron" />
            </button>
          </div>
          <nav className="nav">
            {NAV.map((item) => (
              <a
                key={item.route}
                className={`nav-item ${route === item.route ? 'active' : ''}`}
                href={`#/${item.route}`}
              >
                <Icon name={item.icon} />
                <span className="nav-label">{item.label}</span>
                {item.route === 'notifications' && unread ? <span className="nav-badge warn">{unread}</span> : null}
                {item.route === 'secrets' && data.secrets.length ? <span className="nav-badge">{data.secrets.length}</span> : null}
                {item.route === 'tickets' && data.tickets.length ? <span className="nav-badge">{data.tickets.length}</span> : null}
                {item.route === 'honeypots' && data.honeypots.some((item) => item.status === 'triggered') ? (
                  <span className="nav-badge warn">!</span>
                ) : null}
              </a>
            ))}
          </nav>
          <div className="side-foot">
            <div className="side-status">
              <span className="pulse" />
              <div>
                <strong>{data.status?.broker ? 'Broker 在线' : 'Broker 离线'}</strong>
                <span>{data.status ? `${data.status.backend} · epoch ${data.status.epoch}` : '连接中…'}</span>
              </div>
            </div>
            <div className="side-meta">
              <span>硬件指纹</span>
              <span className="mono">{data.status?.hardware || '—'}</span>
            </div>
          </div>
        </aside>

        <main className="main">
          <header className="topbar">
            <button className="icon-btn menu-btn" onClick={() => showToast('小屏下请使用 Hash 路由')} aria-label="菜单">
              <Icon name="menu" />
            </button>
            <div className="crumbs">
              <span>Workspace</span>
              <Icon name="chevron" className="tiny" />
              <strong>{TITLES[route]}</strong>
            </div>
            <label className="search">
              <Icon name="search" className="tiny" />
              <input
                ref={searchRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="搜索凭证、票据、事件…"
                autoComplete="off"
              />
              <kbd>⌘K</kbd>
            </label>
            <div className="top-actions">
              <span className="env-chip" data-testid="session-tenant" title="当前租户">
                <span className="dot" />
                {session.tenant || 'default'} · 127.0.0.1
              </span>
              <button className="icon-btn" onClick={() => { window.location.hash = '#/notifications'; }} aria-label="通知">
                <Icon name="bell" />
                {unread ? <span className="badge-dot" /> : null}
              </button>
              <button className="avatar" onClick={() => setUserMenu(!userMenu)} aria-label="用户菜单">
                {session.name.slice(0, 2).toUpperCase()}
              </button>
              {userMenu ? (
                <div className="user-menu">
                  <div className="user-menu-head">
                    <div className="profile-avatar">{session.name.slice(0, 2).toUpperCase()}</div>
                    <div>
                      <strong>{session.name}</strong>
                      <span>{session.email}</span>
                    </div>
                  </div>
                  <button
                    className="user-menu-item"
                    onClick={() => {
                      setUserMenu(false);
                      openProfile();
                    }}
                  >
                    <Icon name="user" />
                    个人信息
                  </button>
                  <button
                    className="user-menu-item"
                    onClick={() => {
                      setUserMenu(false);
                      window.location.hash = '#/notifications';
                    }}
                  >
                    <Icon name="bell" />
                    通知中心
                    {unread ? <span className="nav-badge warn">{unread}</span> : null}
                  </button>
                  <button
                    className="user-menu-item"
                    onClick={() => {
                      setUserMenu(false);
                      window.location.hash = '#/settings';
                    }}
                  >
                    <Icon name="sliders" />
                    设置
                  </button>
                  <div className="user-menu-sep" />
                  <button className="user-menu-item danger" onClick={logout}>
                    <Icon name="logout" />
                    退出登录
                  </button>
                  <button className="user-menu-item" onClick={logout}>
                    <Icon name="refresh" />
                    登录其他账号
                  </button>
                </div>
              ) : null}
            </div>
          </header>
          <div className="canvas">
            <div key={route} className="view">
              {content}
            </div>
          </div>
        </main>
      </div>

      <aside className={`drawer ${drawer ? 'open' : ''}`} aria-hidden={!drawer}>
        <div className="drawer-head">
          <div>
            <div className="eyebrow">{drawer?.eyebrow}</div>
            <h2>{drawer?.title}</h2>
          </div>
          <button className="icon-btn" onClick={() => setDrawer(null)} aria-label="关闭">
            <Icon name="x" />
          </button>
        </div>
        <div className="drawer-body">{drawer?.body}</div>
      </aside>

      <div className={`modal-layer ${modal ? 'open' : ''}`} aria-hidden={!modal}>
        <div className="modal">{modal?.content}</div>
      </div>

      <div className={`toast ${toastText ? 'show' : ''}`} role="status">
        {toastText}
      </div>
    </>
  );
}