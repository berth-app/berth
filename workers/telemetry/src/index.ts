export interface Env {
  DB: D1Database;
}

interface TelemetryEvent {
  id: string;
  event_type: string;
  app_version: string;
  os_version: string;
  context: Record<string, unknown>;
  occurred_at: string;
}

interface IngestPayload {
  device_id: string;
  events: TelemetryEvent[];
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, {
        headers: corsHeaders(),
      });
    }

    const url = new URL(request.url);

    if (request.method === "POST" && url.pathname === "/ingest") {
      return handleIngest(request, env);
    }

    return new Response("Not found", { status: 404 });
  },
};

async function handleIngest(request: Request, env: Env): Promise<Response> {
  let payload: IngestPayload;
  try {
    payload = await request.json();
  } catch {
    return jsonResponse({ error: "Invalid JSON" }, 400);
  }

  if (!payload.device_id || !Array.isArray(payload.events) || payload.events.length === 0) {
    return jsonResponse({ error: "Missing device_id or events" }, 400);
  }

  if (payload.events.length > 100) {
    return jsonResponse({ error: "Too many events (max 100)" }, 400);
  }

  // Rate limit: 100 events per device per hour
  const hourAgo = new Date(Date.now() - 3600_000).toISOString();
  const countResult = await env.DB.prepare(
    "SELECT COUNT(*) as cnt FROM telemetry_events WHERE device_id = ? AND ingested_at > ?"
  )
    .bind(payload.device_id, hourAgo)
    .first<{ cnt: number }>();

  if (countResult && countResult.cnt >= 100) {
    return jsonResponse({ error: "Rate limited" }, 429);
  }

  const stmt = env.DB.prepare(
    `INSERT OR IGNORE INTO telemetry_events (id, device_id, event_type, app_version, os_version, context, occurred_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`
  );

  const batch = payload.events.map((e) =>
    stmt.bind(
      e.id,
      payload.device_id,
      e.event_type,
      e.app_version || "",
      e.os_version || "",
      JSON.stringify(e.context || {}),
      e.occurred_at
    )
  );

  await env.DB.batch(batch);

  return jsonResponse({ accepted: payload.events.length }, 200);
}

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": "application/json",
      ...corsHeaders(),
    },
  });
}

function corsHeaders(): Record<string, string> {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  };
}
