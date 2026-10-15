# Security and compatibility

Retain cookie attributes, CSRF checks, allowed origins, service authentication
and authorization. Do not read or move private secrets. Keep wallet format-1
associated data and mock signing domain bytes unchanged. Derive the registry
owner through a step environment variable and lowercase it before constructing
image names; do not interpolate it into executable shell syntax.
