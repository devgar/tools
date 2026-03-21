# pyright: reportAttributeAccessIssue=false
import json
import os
import urllib.parse
import urllib.request
from dataclasses import dataclass, field

import gi

gi.require_version("EDataServer", "1.2")

from gi.repository import EDataServer

from devolution.sources import find_source, get_oauth2_access_token

DEFAULT_ACCOUNT_FILTER = os.getenv("DEFAULT_ACCOUNT_FILTER", "icareweb.com")
DEFAULT_MAX_RESULTS = 20
TASKS_API_BASE = "https://tasks.googleapis.com/tasks/v1"


@dataclass
class TaskRow:
    tasklist: str
    title: str
    status: str
    due: str
    notes: str


@dataclass
class TaskList:
    id: str
    title: str
    tasks: list[TaskRow] = field(default_factory=list)


def _api_get(token: str, path: str, params: dict | None = None) -> dict:
    query = f"?{urllib.parse.urlencode(params)}" if params else ""
    req = urllib.request.Request(
        f"{TASKS_API_BASE}{path}{query}",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read().decode("utf-8"))


def _parse_due(due: str | None) -> str:
    if not due:
        return ""
    # La API devuelve RFC 3339; nos quedamos solo con la fecha
    return due[:10]


def list_tasks(
    account_filter: str = DEFAULT_ACCOUNT_FILTER,
    max_results: int = DEFAULT_MAX_RESULTS,
    show_completed: bool = False,
    tasklist_filter: str | None = None,
) -> list[TaskRow]:
    source = find_source(account_filter, EDataServer.SOURCE_EXTENSION_MAIL_ACCOUNT)
    token = get_oauth2_access_token(source)

    lists_data = _api_get(token, "/users/@me/lists")
    items = lists_data.get("items", [])

    rows: list[TaskRow] = []
    for lst in items:
        lst_title = lst.get("title", "")
        lst_id = lst.get("id", "")

        if tasklist_filter and tasklist_filter.lower() not in lst_title.lower():
            continue

        params: dict = {
            "maxResults": max_results,
            "showCompleted": str(show_completed).lower(),
            "showDeleted": "false",
            "showHidden": "false",
        }
        tasks_data = _api_get(token, f"/lists/{lst_id}/tasks", params)

        for t in tasks_data.get("items", []):
            rows.append(
                TaskRow(
                    tasklist=lst_title,
                    title=t.get("title", "(sin titulo)").strip(),
                    status=t.get("status", ""),
                    due=_parse_due(t.get("due")),
                    notes=(t.get("notes") or "").strip(),
                )
            )

    return rows


def list_task_lists(account_filter: str = DEFAULT_ACCOUNT_FILTER) -> list[TaskList]:
    """Devuelve todas las listas de tareas con sus tareas pendientes."""
    source = find_source(account_filter, EDataServer.SOURCE_EXTENSION_MAIL_ACCOUNT)
    token = get_oauth2_access_token(source)

    lists_data = _api_get(token, "/users/@me/lists")
    result = []
    for lst in lists_data.get("items", []):
        tl = TaskList(id=lst["id"], title=lst.get("title", ""))
        tasks_data = _api_get(
            token,
            f"/lists/{lst['id']}/tasks",
            {"maxResults": 100, "showCompleted": "false", "showDeleted": "false"},
        )
        for t in tasks_data.get("items", []):
            tl.tasks.append(
                TaskRow(
                    tasklist=tl.title,
                    title=t.get("title", "(sin titulo)").strip(),
                    status=t.get("status", ""),
                    due=_parse_due(t.get("due")),
                    notes=(t.get("notes") or "").strip(),
                )
            )
        result.append(tl)
    return result
