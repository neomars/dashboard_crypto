import pandas as pd

from bmi_indicator import find_corrections


def test_no_correction_when_price_only_rises():
    df = pd.DataFrame({'close': [100, 110, 120, 130]})
    assert find_corrections(df, min_drop=-15) == []


def test_detects_single_correction_below_threshold():
    df = pd.DataFrame({'close': [100, 120, 100]})  # -16.7% depuis le pic de 120
    corrections = find_corrections(df, min_drop=-15)
    assert len(corrections) == 1
    assert corrections[0] < -15


def test_ignores_correction_above_threshold():
    df = pd.DataFrame({'close': [100, 120, 110]})  # -8.3% depuis le pic, sous le seuil de -15%
    assert find_corrections(df, min_drop=-15) == []


def test_empty_dataframe_returns_empty_list():
    df = pd.DataFrame({'close': []})
    assert find_corrections(df, min_drop=-15) == []
