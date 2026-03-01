# devolution

Scripts CLI en modo solo lectura para consultar datos corporativos (eventos y correo) usando integracion con GNOME/Evolution.

## Requisitos del sistema (Linux)

Estas dependencias no las instala `uv` porque son del sistema y de GObject Introspection:

- `evolution-data-server`
- `gobject-introspection`
- `libgirepository`
- Typelibs de `EDataServer`, `ECal` y, para correo, backend de mail compatible

## Entorno Python con uv

```bash
uv sync
```

## Configuracion por entorno

Puedes sobreescribir el filtro por defecto de cuenta/calendario con:

```bash
export DEFAULT_ACCOUNT_FILTER="you@example.com"
```

## Ejecucion

```bash
uv run python events.py
uv run python events.py --account another.guy@example.com
uv run python events.py --format json
uv run python events.py --format csv
uv run python mails.py -n 5
uv run python mails.py -n 5 --all
uv run python mails.py -n 5 --format json
uv run python mails.py -n 5 --format csv
```

En eventos, los formatos `json` y `csv` incluyen columnas estructuradas:

- `date`
- `time`
- `relative`
- `summary`
- `description`
- `meet_url`
