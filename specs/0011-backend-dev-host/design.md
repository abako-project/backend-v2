# Design

Reuse the existing adapter on loopback 4080 and the existing TLS virtual host
for `dev.abako.xyz`. Add only the bounded API routing block. Remove the known
Digesta block from the web site while retaining its frontend and exact RPC
route. Keep one safe API log format definition. Validate both site candidates
together before replacing either live file.

The dev site's legacy server-level OPTIONS response excludes `/api` so the
adapter handles its preflight. The API location retains existing security
response headers and uses the adapter's CORS headers; other locations retain
the site's existing legacy CORS behavior.

Back up settings and both sites privately. Recreate only the adapter with the
correct origin/RP settings. Validate the public API; restore configuration on
failure without touching stores or secrets.
