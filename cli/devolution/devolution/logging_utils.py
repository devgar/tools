import logging
import os


def configure_logging() -> logging.Logger:
    logging.basicConfig(level=os.environ.get("LOGLEVEL", "ERROR").upper())
    return logging.getLogger(__name__)
