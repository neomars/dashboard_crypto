import os
import sys

import pytest
import streamlit as st

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


@pytest.fixture(autouse=True)
def _clear_streamlit_cache():
    """Évite les faux positifs/négatifs dus au cache st.cache_data partagé entre tests."""
    st.cache_data.clear()
    yield
