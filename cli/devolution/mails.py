#!/usr/bin/env python3

import argparse

from devolution.mail_reader import (
    DEFAULT_ACCOUNT_FILTER,
    DEFAULT_IMAP_HOST,
    DEFAULT_MAX_RESULTS,
    list_mail,
)
from devolution.renderers import OUTPUT_FORMATS, render_mail_rows


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Lista correos desde GNOME/Evolution en modo solo lectura"
    )
    parser.add_argument(
        "--account",
        default=DEFAULT_ACCOUNT_FILTER,
        help="Filtro para localizar la cuenta (display name o uid)",
    )
    parser.add_argument(
        "-n",
        "--max-results",
        default=DEFAULT_MAX_RESULTS,
        type=int,
        help="Numero maximo de correos a mostrar",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Incluye tambien correos leidos (por defecto solo no leidos)",
    )
    parser.add_argument(
        "--imap-host",
        default=DEFAULT_IMAP_HOST,
        help="Servidor IMAP (por defecto imap.gmail.com)",
    )
    parser.add_argument(
        "--format",
        choices=OUTPUT_FORMATS,
        default="text",
        help="Formato de salida: text, json o csv",
    )
    args = parser.parse_args()
    if args.account is None:
        parser.error("El filtro de cuenta es obligatorio")
    return args


def main() -> int:
    args = _parse_args()
    try:
        rows = list_mail(
            account_filter=args.account,
            max_results=max(1, args.max_results),
            unread_only=not args.all,
            imap_host=args.imap_host,
        )
    except Exception as exc:
        print(f"Error: {exc}")
        return 1

    if not rows:
        print("No se encontraron correos para los filtros indicados.")
        return 0

    for line in render_mail_rows(rows, output_format=args.format):
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
