# pyright: reportAttributeAccessIssue=false
import email
import imaplib
import os
from dataclasses import dataclass
from email.utils import parsedate_to_datetime

import gi

gi.require_version("EDataServer", "1.2")

from gi.repository import EDataServer

from devolution.sources import find_source, get_oauth2_access_token

DEFAULT_ACCOUNT_FILTER = os.getenv("DEFAULT_ACCOUNT_FILTER")
DEFAULT_MAX_RESULTS = 10
DEFAULT_IMAP_HOST = "imap.gmail.com"


@dataclass
class MailRow:
    date: str
    sender: str
    subject: str


def _as_text(value) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _imap_date(date_raw: str) -> str:
    if not date_raw:
        return "sin-fecha"
    try:
        dt = parsedate_to_datetime(date_raw)
        return dt.strftime("%Y-%m-%d %H:%M")
    except Exception:
        return date_raw


def _guess_username(source) -> str:
    for value in (source.get_display_name(), source.get_uid()):
        text = _as_text(value)
        if "@" in text:
            return text
    raise RuntimeError("No se pudo deducir el usuario IMAP desde la fuente de GNOME")


def list_mail(
    account_filter: str,
    max_results: int = DEFAULT_MAX_RESULTS,
    unread_only: bool = True,
    imap_host: str = DEFAULT_IMAP_HOST,
) -> list[MailRow]:
    source = find_source(account_filter, EDataServer.SOURCE_EXTENSION_MAIL_ACCOUNT)
    token = get_oauth2_access_token(source)
    username = _guess_username(source)

    auth_string = f"user={username}\x01auth=Bearer {token}\x01\x01"
    query = "UNSEEN" if unread_only else "ALL"

    mailbox = imaplib.IMAP4_SSL(imap_host)
    mailbox.authenticate("XOAUTH2", lambda _: auth_string.encode("utf-8"))

    try:
        status, _ = mailbox.select("INBOX", readonly=True)
        if status != "OK":
            raise RuntimeError("No se pudo abrir INBOX en modo lectura")

        status, data = mailbox.search(None, query)
        if status != "OK":
            raise RuntimeError("No se pudo listar correos en INBOX")

        ids = data[0].split() if data and data[0] else []
        ids = list(reversed(ids[-max_results:]))

        rows = []
        for msg_id in ids:
            status, payload = mailbox.fetch(
                msg_id,
                "(BODY.PEEK[HEADER.FIELDS (DATE FROM SUBJECT)])",
            )
            if status != "OK" or not payload:
                continue

            raw_headers = None
            for part in payload:
                if isinstance(part, tuple):
                    raw_headers = part[1]
                    break
            if not raw_headers:
                continue

            msg = email.message_from_bytes(raw_headers)
            rows.append(
                MailRow(
                    date=_imap_date(_as_text(msg.get("Date"))),
                    sender=_as_text(msg.get("From")) or "(sin remitente)",
                    subject=_as_text(msg.get("Subject")) or "(sin asunto)",
                )
            )
    finally:
        try:
            mailbox.logout()
        except Exception:
            pass

    return rows
