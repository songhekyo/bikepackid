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
    const originUrl = new URL(url.pathname + url.search, "https://origin.taktikdansiasat.com");
    return fetch(new Request(originUrl, request));
  },
};
