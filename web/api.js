const ACCESS_KEY = 'cs_web_access';
const REFRESH_KEY = 'cs_web_refresh';
let refreshPromise = null;

export class ApiError extends Error {
  constructor(status) {
    const messages = {
      400: '提交的内容有误，请检查后重试。',
      401: '登录已失效，请重新登录。',
      403: '当前账号没有执行此操作的权限。',
      404: '没有找到这项内容，它可能已被删除。',
      409: '内容刚刚发生变化，请刷新后重试。',
      429: '操作太频繁，请稍等片刻再试。',
    };
    super(messages[status] || `暂时无法连接服务（HTTP ${status}），请稍后重试。`);
    this.status = status;
  }
}

export function saveSession(data) {
  sessionStorage.setItem(ACCESS_KEY, data.access_token);
  sessionStorage.setItem(REFRESH_KEY, data.refresh_token);
}

export function clearSession() {
  sessionStorage.removeItem(ACCESS_KEY);
  sessionStorage.removeItem(REFRESH_KEY);
}

export function accessToken() { return sessionStorage.getItem(ACCESS_KEY) || ''; }

async function refreshSession() {
  const refreshToken = sessionStorage.getItem(REFRESH_KEY);
  if (!refreshToken) throw new ApiError(401);
  const response = await fetch('/api/refresh-token', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  });
  if (!response.ok) { clearSession(); throw new ApiError(response.status); }
  const data = await response.json();
  saveSession(data);
  return data.access_token;
}

export async function request(path, options = {}, retry = true) {
  const headers = new Headers(options.headers || {});
  const token = accessToken();
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (options.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  let response;
  try { response = await fetch(path, { cache: 'no-store', ...options, headers }); }
  catch { throw new Error('无法连接 Server，请检查网络或服务是否运行。'); }
  if (response.status === 401 && retry && sessionStorage.getItem(REFRESH_KEY)) {
    refreshPromise ||= refreshSession().finally(() => { refreshPromise = null; });
    await refreshPromise;
    return request(path, options, false);
  }
  if (!response.ok) throw new ApiError(response.status);
  if (response.status === 204) return null;
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

export async function login(username, password) {
  const response = await fetch('/api/login', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!response.ok) throw new ApiError(response.status);
  const data = await response.json();
  saveSession(data);
  return data;
}

export function connectEvents({ onMessage, onStatus }) {
  let socket;
  let closed = false;
  let retryTimer;
  const connect = () => {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const token = accessToken();
    socket = new WebSocket(`${protocol}//${location.host}/ws${token ? `?token=${encodeURIComponent(token)}` : ''}`);
    socket.onopen = () => { onStatus(true); socket.send(JSON.stringify({ node_id: '' })); };
    socket.onmessage = event => {
      try { onMessage(JSON.parse(event.data)); } catch { /* Ignore malformed push messages. */ }
    };
    socket.onclose = () => {
      onStatus(false);
      if (!closed) retryTimer = setTimeout(connect, 3000);
    };
    socket.onerror = () => socket.close();
  };
  connect();
  return () => { closed = true; clearTimeout(retryTimer); socket?.close(); };
}
