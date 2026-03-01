#!/usr/bin/env python3

import argparse

from devolution.event_reader import DEFAULT_ACCOUNT_FILTER, get_event_rows
from devolution.renderers import OUTPUT_FORMATS, render_events


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Lista eventos desde GNOME/Evolution en modo solo lectura"
    )
    parser.add_argument(
        "--format",
        choices=OUTPUT_FORMATS,
        default="text",
        help="Formato de salida: text, json o csv",
    )
    parser.add_argument(
        "--account",
        default=DEFAULT_ACCOUNT_FILTER,
        help="Filtro para localizar el calendario (display name o uid)",
    )
    return parser.parse_args()


if __name__ == "__main__":
    args = _parse_args()
    myevents = get_event_rows(account_filter=args.account)
    for line in render_events(myevents, output_format=args.format):
        print(line)
