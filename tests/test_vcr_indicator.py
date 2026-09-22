import numpy as np
import pandas as pd

from vcr_indicator import vol


def test_vol_is_zero_for_constant_series():
    series = pd.Series([0.0] * 40)
    result = vol(series, window=30)
    assert result.dropna().eq(0).all()


def test_vol_scales_with_dispersion():
    np.random.seed(0)
    low_vol = pd.Series(np.random.normal(0, 0.001, 400))
    high_vol = pd.Series(np.random.normal(0, 0.05, 400))
    low_result = vol(low_vol, window=30).dropna().mean()
    high_result = vol(high_vol, window=30).dropna().mean()
    assert high_result > low_result
