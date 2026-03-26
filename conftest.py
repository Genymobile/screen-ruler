"""
conftest.py — make the repository root available as a Python import path.

This allows ``pytest`` to find ``screen_ruler`` without needing a manual
``PYTHONPATH=.`` prefix.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))
