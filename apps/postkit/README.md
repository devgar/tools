# postkit — spike Bluesky

Validación end-to-end del diseño de tres iteraciones con un único provider (Bluesky).
Cuando la arquitectura esté cómoda, se generaliza añadiendo providers y metiendo SQLite + scheduler.

## Estructura

```
crates/
├── postkit-core/      # traits + tipos compartidos
└── postkit/           # binary CLI + provider Bluesky
```

El trait `Provider` expone los tres verbos del modelo:

- `verify()`  → iter 1 (handshake)
- `compose()` → iter 2 (función pura SourcePost → PreparedPost)
- `execute()` → iter 3 (ejecuta el plan)

`compose()` es deliberadamente síncrona y sin I/O: devuelve un `Vec<Step>` declarativo
que luego `execute()` interpreta. Esto hace que iter 2 sea trivial de testear con
snapshot testing y permite previsualizar planes antes de ejecutar.

## Setup

```bash
# 1. App password de Bluesky
#    → https://bsky.app/settings/app-passwords

# 2. Config
mkdir -p ~/.config/postkit
cp config.example.toml ~/.config/postkit/config.toml
$EDITOR ~/.config/postkit/config.toml

# 3. Build
cargo build --release
```

## Comandos

```bash
# Lista cuentas cargadas desde la config
postkit accounts

# Verifica credenciales (iter 1)
postkit verify
postkit verify personal

# Compone plan sin publicar (iter 2)
#   Emite un array de PreparedPost como JSON → pipea a jq para inspeccionar
postkit compose post.example.toml
postkit compose post.example.toml --targets personal

# Compone + publica inmediatamente (iter 3 sin scheduling)
postkit publish post.example.toml
```

## Lo que NO incluye este spike

- Scheduling / persistencia (SQLite + worker tokio vienen en la iteración 3 completa)
- Refresh de JWT (la app password dura, pero el accessJwt tiene TTL corto;
  para sesiones largas hay que llamar a `com.atproto.server.refreshSession`)
- Threading automático cuando el texto excede 300 grafemas
- Resolución de menciones (`@handle` → DID) para facets
- Retry con backoff
- Logging estructurado (tracing)

## Siguientes pasos sugeridos

1. Publicar un post de prueba end-to-end → confirma que los traits modelan bien el flujo
2. Escribir tests de `compose()` con snapshot testing (`insta` crate)
3. Extraer el provider a su propio crate `postkit-providers-bluesky`
4. Añadir provider de X (pay-per-use ~$0.01/post en 2026)
5. Meter SQLite + scheduler en binary aparte (`postkit-daemon`)
