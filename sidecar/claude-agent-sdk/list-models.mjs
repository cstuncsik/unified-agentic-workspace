// Dependency-free model-list helper for the SDK model picker. Key via env (injected
// by the backend, never argv). On success prints the Anthropic /v1/models JSON to
// stdout; on ANY failure prints nothing to stdout or stderr and exits 1 — the backend
// forwards a fixed opaque error, so the API status/body/endpoint must never surface.
try {
  const res = await fetch("https://api.anthropic.com/v1/models?limit=1000", {
    headers: {
      "x-api-key": process.env.ANTHROPIC_API_KEY ?? "",
      "anthropic-version": "2023-06-01",
    },
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) process.exit(1);
  process.stdout.write(await res.text());
} catch {
  process.exit(1);
}
