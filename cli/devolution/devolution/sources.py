# pyright: reportAttributeAccessIssue=false
import gi

gi.require_version("EDataServer", "1.2")

from gi.repository import EDataServer


def new_registry() -> EDataServer.SourceRegistry:
    return EDataServer.SourceRegistry.new_sync()


def find_source(account_filter: str, extension: int):
    registry = new_registry()
    token = (account_filter or "").lower()
    sources = registry.list_sources(extension)

    for source in sources:
        display_name = (source.get_display_name() or "").lower()
        source_uid = (source.get_uid() or "").lower()
        if token in display_name or token in source_uid:
            return source

    raise RuntimeError(
        f"No se encontro ninguna cuenta con filtro '{account_filter}' en GNOME Online Accounts/Evolution"
    )


def get_oauth2_access_token(source) -> str:
    ok, access_token, _ = source.get_oauth2_access_token_sync(None)
    if not ok or not access_token:
        raise RuntimeError(
            "No se pudo obtener token OAuth2 desde GNOME (revisa Online Accounts y keyring)"
        )
    return access_token
