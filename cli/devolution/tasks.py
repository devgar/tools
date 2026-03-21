#!/usr/bin/env python3

import argparse

from devolution.task_reader import DEFAULT_ACCOUNT_FILTER, DEFAULT_MAX_RESULTS, list_tasks
from devolution.renderers import OUTPUT_FORMATS, render_tasks


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Lista Google Tasks desde GNOME/Evolution en modo solo lectura"
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
        help="Numero maximo de tareas a mostrar por lista",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Incluye tambien tareas completadas (por defecto solo pendientes)",
    )
    parser.add_argument(
        "--list",
        dest="tasklist",
        default=None,
        help="Filtro por nombre de lista de tareas",
    )
    parser.add_argument(
        "--format",
        choices=OUTPUT_FORMATS,
        default="text",
        help="Formato de salida: text, json o csv",
    )
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    try:
        rows = list_tasks(
            account_filter=args.account,
            max_results=max(1, args.max_results),
            show_completed=args.all,
            tasklist_filter=args.tasklist,
        )
    except Exception as exc:
        print(f"Error: {exc}")
        return 1

    if not rows:
        print("No se encontraron tareas para los filtros indicados.")
        return 0

    for line in render_tasks(rows, output_format=args.format):
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
