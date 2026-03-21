import csv
import io
import json

from devolution.event_reader import EventRow
from devolution.mail_reader import MailRow
from devolution.task_reader import TaskRow

OUTPUT_FORMATS = ("text", "json", "csv")


def render_mail_rows(rows: list[MailRow], output_format: str = "text") -> list[str]:
    if output_format == "text":
        return [f"[{row.date}] {row.sender} | {row.subject}" for row in rows]

    if output_format == "json":
        payload = [
            {"date": row.date, "sender": row.sender, "subject": row.subject}
            for row in rows
        ]
        return [json.dumps(payload, ensure_ascii=False, indent=2)]

    if output_format == "csv":
        buffer = io.StringIO()
        writer = csv.DictWriter(buffer, fieldnames=["date", "sender", "subject"])
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {"date": row.date, "sender": row.sender, "subject": row.subject}
            )
        return [buffer.getvalue().rstrip("\n")]

    raise ValueError(f"Formato de salida no soportado: {output_format}")


def render_events(events: list[EventRow], output_format: str = "text") -> list[str]:
    if output_format == "text":
        return [
            f"[{event.date} {event.time}] {event.relative:<20} {event.summary} {event.description}"
            for event in events
        ]

    if output_format == "json":
        payload = [
            {
                "date": event.date,
                "time": event.time,
                "relative": event.relative,
                "summary": event.summary,
                "description": event.description,
                "meet_url": event.meet_url,
            }
            for event in events
        ]
        return [json.dumps(payload, ensure_ascii=False, indent=2)]

    if output_format == "csv":
        buffer = io.StringIO()
        writer = csv.DictWriter(
            buffer,
            fieldnames=[
                "date",
                "time",
                "relative",
                "summary",
                "description",
                "meet_url",
            ],
        )
        writer.writeheader()
        for event in events:
            writer.writerow(
                {
                    "date": event.date,
                    "time": event.time,
                    "relative": event.relative,
                    "summary": event.summary,
                    "description": event.description,
                    "meet_url": event.meet_url,
                }
            )
        return [buffer.getvalue().rstrip("\n")]

    raise ValueError(f"Formato de salida no soportado: {output_format}")


def render_tasks(rows: list[TaskRow], output_format: str = "text") -> list[str]:
    _FIELDS = ["tasklist", "due", "status", "title", "notes"]

    if output_format == "text":
        lines = []
        for row in rows:
            due = f" [{row.due}]" if row.due else ""
            notes = f" — {row.notes}" if row.notes else ""
            lines.append(f"[{row.tasklist}]{due} {row.title}{notes}")
        return lines

    if output_format == "json":
        payload = [
            {
                "tasklist": row.tasklist,
                "due": row.due,
                "status": row.status,
                "title": row.title,
                "notes": row.notes,
            }
            for row in rows
        ]
        return [json.dumps(payload, ensure_ascii=False, indent=2)]

    if output_format == "csv":
        buffer = io.StringIO()
        writer = csv.DictWriter(buffer, fieldnames=_FIELDS)
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    "tasklist": row.tasklist,
                    "due": row.due,
                    "status": row.status,
                    "title": row.title,
                    "notes": row.notes,
                }
            )
        return [buffer.getvalue().rstrip("\n")]

    raise ValueError(f"Formato de salida no soportado: {output_format}")
