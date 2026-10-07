// WebSocket check for ClusterScope /ws (QA fixture).
// Node >= 22 ships a global WebSocket client, so no package install is needed
// (this host is offline). Node lives at /public/tianyuyang/.nvm/.../node.
//
// usage: node ws-check.mjs [url] [node_id] [seconds]
// prints one line per received message and a final RESULT line:
//   RESULT connected=<0|1> subscribed=<0|1> metrics=<0|1> alerts=<0|1>
const url = process.argv[2] ?? "ws://127.0.0.1:8080/ws";
const nodeId = process.argv[3] ?? "qa-node-01";
const seconds = Number(process.argv[4] ?? 20);

// Server message types are `metrics_update` / `alert_update` / `job_update`;
// the counters keep the short names used on the RESULT line.
const counters = { connected: 0, subscribed: 0, metrics: 0, alerts: 0, jobs: 0 };
const typeToCounter = {
  connected: "connected",
  subscribed: "subscribed",
  metrics_update: "metrics",
  alert_update: "alerts",
  job_update: "jobs",
};
const ws = new WebSocket(url);
const report = () =>
  `RESULT connected=${counters.connected} subscribed=${counters.subscribed} metrics=${counters.metrics} alerts=${counters.alerts} jobs=${counters.jobs}`;
const timer = setTimeout(() => {
  console.log(report());
  ws.close();
  process.exit(0);
}, seconds * 1000);

ws.addEventListener("open", () => {
  console.log("OPEN " + url);
  ws.send(JSON.stringify({ node_id: nodeId }));
});

ws.addEventListener("message", (event) => {
  const text = String(event.data);
  let type = "unknown";
  try {
    type = JSON.parse(text).type ?? "unknown";
  } catch {
    /* not JSON */
  }
  const key = typeToCounter[type];
  if (key) counters[key] += 1;
  console.log("MSG " + type + " " + text.slice(0, 160));
});

ws.addEventListener("error", (event) => {
  console.log("ERROR " + (event.message ?? "websocket error"));
});

ws.addEventListener("close", (event) => {
  console.log(`CLOSE code=${event.code} reason=${event.reason}`);
  clearTimeout(timer);
  console.log(report());
  process.exit(0);
});
