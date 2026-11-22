export type SecretKind = 'env' | 'token' | 'json';
export type TicketKind = 'leaf' | 'parent';

export interface Status {
  backend: string;
  device: string;
  hardware: string;
  epoch: number;
  broker: boolean;
  secrets: number;
  agents: number;
}

export interface Overview {
  secrets: number;
  live: number;
  issued: number;
  resolved: number;
  denied: number;
  trend: { issued: number[]; resolved: number[] };
  kinds: Array<{ label: string; value: number; count: number }>;
  usage: Array<{ label: string; value: number }>;
  heat: number[];
  epoch: number;
}

export interface Secret {
  name: string;
  kind: SecretKind;
  epoch: number;
  rotated_at: number;
  created_at: number;
  honeypot: boolean;
  agents: string[];
  note: string | null;
}

export interface Ticket {
  id: string;
  kind: TicketKind;
  secret: string;
  sub: string;
  parent: string | null;
  exp: number;
  iat: number;
  total: number;
  uses: number;
  renewals: number;
  scope: string;
  status: 'active' | 'expired';
}

export interface Agent {
  id: string;
  path: string;
  wrapped: boolean;
  servers: number;
  supports_toml: boolean;
}

export interface AgentCandidate {
  id: string;
  paths: Array<{ path: string; exists: boolean }>;
  supports_toml: boolean;
}

export interface AgentPlan {
  path: string;
  format: 'json' | 'toml';
  changes: Array<Record<string, unknown>>;
  skipped?: string[];
}

export interface IssuedTicket {
  ticket: string;
  jti: string;
  exp?: number;
}

export interface AuditEvent {
  ts: number;
  event: string;
  actor: string;
  secret?: string | null;
  ticket?: string | null;
  result: string;
  meta?: unknown;
}

export interface Honeypot {
  id: string;
  name: string;
  token: string;
  hits: number;
  status: 'armed' | 'triggered';
  created_at: number;
  triggered_at: number | null;
}

export type Role = 'owner' | 'admin' | 'viewer';

export interface AuthSession {
  token: string;
  role: Role | string;
  tenant: string;
  expires_at: number;
}

export interface Me {
  username: string;
  role: Role | string;
  tenant: string;
}

export interface Account {
  username: string;
  role: Role | string;
  tenant: string;
  created_at: number;
}

const TOKEN_KEY = 'av-token';
// The desktop shell hands off its loopback token once, then removes it from the address bar.
const bootURL = new URL(window.location.href);
const bootToken = bootURL.searchParams.get('token')?.trim();
if (bootToken) {
  sessionStorage.setItem(TOKEN_KEY, bootToken);
  bootURL.searchParams.delete('token');
  window.history.replaceState(null, '', `${bootURL.pathname}${bootURL.search}${bootURL.hash}`);
}
export let token = sessionStorage.getItem(TOKEN_KEY) || '';

export function setToken(value: string): void {
  token = value.trim();
  if (token) sessionStorage.setItem(TOKEN_KEY, token);
  else sessionStorage.removeItem(TOKEN_KEY);
}

export function hasToken(): boolean {
  return Boolean(token);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${token}`,
      ...(init?.headers || {}),
    },
  });
  const body = await response.json().catch(() => ({ ok: false, error: `HTTP ${response.status}` }));
  if (!body.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body.data as T;
}

export const api = {
  login: (username: string, password: string) =>
    request<AuthSession>('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),
  logout: () => request<{ ok: boolean }>('/api/auth/logout', { method: 'POST' }),
  me: () => request<Me>('/api/auth/me'),
  accounts: () => request<Account[]>('/api/auth/accounts'),
  addAccount: (username: string, password: string, role: string, tenant: string) =>
    request<Account>('/api/auth/accounts', {
      method: 'POST',
      body: JSON.stringify({ username, password, role, tenant }),
    }),
  removeAccount: (username: string) =>
    request<{ removed: string }>(`/api/auth/accounts/${encodeURIComponent(username)}`, { method: 'DELETE' }),
  status: () => request<Status>('/api/status'),
  overview: () => request<Overview>('/api/overview'),
  secrets: () => request<Secret[]>('/api/secrets'),
  addSecret: (name: string, value: string, kind: SecretKind = 'token') =>
    request('/api/secrets', { method: 'POST', body: JSON.stringify({ name, value, kind }) }),
  rotate: (name: string) =>
    request<{ secret: string; new_value: string }>(`/api/secrets/${encodeURIComponent(name)}/rotate`, {
      method: 'POST',
    }),
  removeSecret: (name: string) =>
    request(`/api/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' }),
  tickets: () => request<Ticket[]>('/api/tickets'),
  issueTicket: (secret: string, ttl = '15m', scope = 'read', maxUses = 1) =>
    request<IssuedTicket>('/api/tickets', {
      method: 'POST',
      body: JSON.stringify({ secret, ttl, scope, max_uses: maxUses }),
    }),
  renew: (id: string) =>
    request(`/api/tickets/${encodeURIComponent(id)}/renew`, { method: 'POST' }),
  revokeTicket: (id: string) =>
    request(`/api/tickets/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  revokeAll: () =>
    request('/api/revoke-all', { method: 'POST', body: JSON.stringify({ confirm: true }) }),
  agents: () => request<Agent[]>('/api/agents'),
  agentCandidates: () => request<AgentCandidate[]>('/api/agents/candidates'),
  agentPlan: (agent: string, path?: string) =>
    request<AgentPlan>('/api/agents/plan', {
      method: 'POST',
      body: JSON.stringify({ agent, path }),
    }),
  initAgent: (agent: string, path?: string) =>
    request(`/api/agents/init`, {
      method: 'POST',
      body: JSON.stringify({ agent, path, confirm: true }),
    }),
  audit: (limit = 100) => request<AuditEvent[]>(`/api/audit?limit=${limit}`),
  honeypots: () => request<Honeypot[]>('/api/honeypots'),
  addHoneypot: (name: string) =>
    request('/api/honeypots', { method: 'POST', body: JSON.stringify({ name }) }),
  triggerHoneypot: (id: string) =>
    request(`/api/honeypots/${encodeURIComponent(id)}/trigger`, {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    }),
  deleteHoneypot: (id: string) =>
    request(`/api/honeypots/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};

/**
 * SSE without putting the token in a URL: EventSource cannot send headers, so
 * we read the event stream with fetch and an Authorization header.
 */
export function subscribe(onRefresh: () => void): () => void {
  if (!token) return () => undefined;
  const controller = new AbortController();
  let closed = false;

  void (async () => {
    try {
      const response = await fetch('/api/events', {
        headers: { authorization: `Bearer ${token}` },
        signal: controller.signal,
      });
      if (!response.ok || !response.body) return;
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      while (!closed) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';
        for (const line of lines) {
          if (line.startsWith('event: refresh')) onRefresh();
        }
      }
    } catch {
      /* network errors are reported by the next poll */
    }
  })();

  return () => {
    closed = true;
    controller.abort();
  };
}