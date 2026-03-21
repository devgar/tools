# pyright: reportAttributeAccessIssue=false
import os
import re
from dataclasses import dataclass
from datetime import datetime, timedelta

import gi

gi.require_version("ECal", "2.0")

from gi.repository import ECal, EDataServer

from devolution.sources import new_registry

DEFAULT_ACCOUNT_FILTER = os.getenv(
    "DEFAULT_ACCOUNT_FILTER", "edgar.albalate@icareweb.com"
)


@dataclass
class EventRow:
    date: str
    time: str
    relative: str
    summary: str
    description: str
    meet_url: str


def _parse_ical(str_date: str) -> datetime:
    if len(str_date) == 8:
        return datetime.strptime(str_date, "%Y%m%d")
    return datetime.strptime(str_date, "%Y%m%dT%H%M%S")


def time_to(str_date: str, now: datetime | None = None) -> str:
    event_date = _parse_ical(str_date)
    now = now or datetime.now()
    delta = event_date - now
    if delta.total_seconds() < 0:
        return "Evento pasado"
    if delta.total_seconds() < 60:
        return "En unos segundos"
    if delta.total_seconds() < 3600:
        minutes = int(delta.total_seconds() // 60)
        return f"En {minutes} minuto{'s' if minutes > 1 else ''}"
    if delta.total_seconds() < 86400:
        hours = int(delta.total_seconds() // 3600)
        return f"En {hours} hora{'s' if hours > 1 else ''}"
    days = int(delta.total_seconds() // 86400)
    return f"En {days} dia{'s' if days > 1 else ''}"


def is_before_next_saturday(str_date: str, today: datetime | None = None) -> bool:
    event_date = _parse_ical(str_date)
    today = today or datetime.now()
    today = today.replace(hour=0, minute=0, second=0, microsecond=0)
    days_to_sat = (5 - today.weekday()) % 7
    if days_to_sat == 0:
        days_to_sat = 7
    next_sat = today + timedelta(days=days_to_sat)
    return event_date < next_sat and event_date >= today


def get_events(account_filter: str) -> list[str]:
    rows = get_event_rows(account_filter)
    return [
        f"[{row.date} {row.time}] {row.relative:<20} {row.summary} {row.description}"
        for row in rows
    ]


def get_event_rows(account_filter: str) -> list[EventRow]:
    registry = new_registry()
    sources = registry.list_sources(EDataServer.SOURCE_EXTENSION_CALENDAR)

    now = datetime.now()
    rows = []

    for source in sources:
        uid = (source.get_uid() or "").lower()
        display_name = (source.get_display_name() or "").lower()
        if account_filter and account_filter.lower() not in [uid, display_name]:
            continue

        client = ECal.Client.connect_sync(
            source, ECal.ClientSourceType.EVENTS, 30, None
        )
        _, components = client.get_object_list_as_comps_sync('(contains? "" "")', None)

        for comp in components:
            dtstart = comp.get_dtstart()
            if not dtstart:
                continue

            ical_val = dtstart.get_value().as_ical_string()
            if not is_before_next_saturday(ical_val, now):
                continue

            summary = comp.get_summary()
            summary_val = summary.get_value() if summary else "Sin titulo"

            description = ""
            descriptions = comp.get_descriptions() or []
            for desc in descriptions:
                description = desc.get_value() or ""

            match = re.search(r"https://meet\.google\.com/[a-z0-9-]+", description)
            meet_url = match.group(0) if match else ""

            event_dt = _parse_ical(ical_val)
            date = event_dt.strftime("%Y-%m-%d")
            time = event_dt.strftime("%H:%M") if len(ical_val) > 8 else "00:00"
            description_val = meet_url if meet_url else description

            rows.append(
                EventRow(
                    date=date,
                    time=time,
                    relative=time_to(ical_val, now),
                    summary=summary_val,
                    description=description_val,
                    meet_url=meet_url,
                )
            )

    rows.sort(key=lambda row: (row.date, row.time, row.summary))
    return rows
