import pandas as pd

from bmsb_indicator import calculate_bear_market_support_band


def test_bull_market_when_price_above_sma20():
    dates = pd.date_range('2020-01-01', periods=25, freq='W')
    prices = [100 + i for i in range(25)]  # tendance haussière constante
    df = pd.DataFrame({'date': dates, 'close': prices})
    result = calculate_bear_market_support_band(df, sma_length=20, ema_length=21)
    assert result.iloc[-1]['market_regime'] == "Bull Market"


def test_bear_market_when_price_below_sma20():
    dates = pd.date_range('2020-01-01', periods=25, freq='W')
    prices = [200 - i * 5 for i in range(25)]  # tendance baissière marquée
    df = pd.DataFrame({'date': dates, 'close': prices})
    result = calculate_bear_market_support_band(df, sma_length=20, ema_length=21)
    assert result.iloc[-1]['market_regime'] == "Bear Market"


def test_adds_expected_columns():
    dates = pd.date_range('2020-01-01', periods=10, freq='W')
    df = pd.DataFrame({'date': dates, 'close': range(100, 110)})
    result = calculate_bear_market_support_band(df)
    assert {'bmsb_sma20', 'bmsb_ema21', 'market_regime'}.issubset(result.columns)
