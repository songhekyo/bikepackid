export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);

    // GET/HEAD: try the static landing page first. Anything that isn't
    // a known static file (every API route) falls through to the EC2
    // origin — no hardcoded list of API path prefixes to keep in sync
    // with auth-service/journey-service as routes are added.
    if (request.method === "GET" || request.method === "HEAD") {
      const assetResponse = await env.ASSETS.fetch(request);
      if (assetResponse.status !== 404) {
        return assetResponse;
      }
    }

    // Everything else (POST/PATCH/... and any GET/HEAD that missed a
    // static asset) goes to the EC2 origin, reachable only at this
    // DNS-only (non-proxied) hostname.
    //
    // redirect: "manual" is load-bearing — fetch()'s default is "follow",
    // which means *this Worker* would transparently chase a 3xx from the
    // origin (e.g. the waitlist form's 303 back to taktikdansiasat.com)
    // instead of the real client. Since that redirect target is this same
    // zone, the Worker would end up fetching itself — Cloudflare's
    // same-zone loop protection kicks in and the client gets back a
    // broken/empty response instead of the page. Passing the raw 3xx +
    // Location straight through lets the browser do the following, same
    // as any ordinary reverse proxy should.
    const originUrl = new URL(url.pathname + url.search, "https://origin.taktikdansiasat.com");
    return fetch(new Request(originUrl, request), { redirect: "manual" });
  },
};
