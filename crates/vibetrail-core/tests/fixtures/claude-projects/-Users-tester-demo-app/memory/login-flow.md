---
name: login-flow
description: Token refresh happens in middleware, not the client
metadata:
  type: project
---

The login module refreshes tokens inside `src/auth/middleware.ts`; the
client never calls the refresh endpoint directly.
