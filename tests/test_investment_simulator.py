import pandas as pd

from investment_simulator import _simulate


def _make_df(prices, freq='D'):
    dates = pd.date_range('2020-01-01', periods=len(prices), freq=freq)
    return pd.DataFrame({'Close': prices}, index=dates)


def test_flat_price_stays_in_x1_mode():
    df = _make_df([100.0] * 30)
    history_df, trades_df = _simulate(
        df, initial_investment=10000, drop_threshold_pct=10.0,
        target_leverage=2.0, exit_frequency='Hebdomadaire', exit_pct=10.0, ticker='BTC-USD'
    )
    assert (history_df['Mode'] == 'X1').all()
    assert history_df['Portfolio_Value'].iloc[-1] == 10000.0


def test_drop_triggers_leverage_switch():
    prices = [100.0] * 5 + [95, 90, 85, 80]  # -20% depuis l'ATH de 100
    df = _make_df(prices)
    history_df, trades_df = _simulate(
        df, initial_investment=10000, drop_threshold_pct=10.0,
        target_leverage=2.0, exit_frequency='Hebdomadaire', exit_pct=10.0, ticker='BTC-USD'
    )
    assert (history_df['Mode'] == 'XL').any()
    assert trades_df['Action'].str.contains('Passage en Levier').any()


def test_no_loss_rule_postpones_exit_below_purchase_price():
    # Drop de 20% pour déclencher le levier (achat à 80), puis le prix reste sous ce niveau
    prices = [100.0] * 3 + [80.0] + [70.0] * 30
    df = _make_df(prices)
    history_df, trades_df = _simulate(
        df, initial_investment=10000, drop_threshold_pct=10.0,
        target_leverage=2.0, exit_frequency='Journalière', exit_pct=10.0, ticker='BTC-USD'
    )
    assert trades_df['Action'].str.contains('reportée').any()
    # Toujours en mode XL car le prix n'est jamais remonté au-dessus du prix d'achat
    assert history_df['Mode'].iloc[-1] == 'XL'


def test_liquidation_zeroes_out_portfolio():
    # Levier x2 (achat à 80) puis crash à 30 -> equity nette négative -> liquidation
    prices = [100.0] * 3 + [80.0] + [30.0] * 5
    df = _make_df(prices)
    history_df, trades_df = _simulate(
        df, initial_investment=10000, drop_threshold_pct=10.0,
        target_leverage=2.0, exit_frequency='Hebdomadaire', exit_pct=10.0, ticker='BTC-USD'
    )
    assert trades_df['Action'].str.contains('LIQUIDATION').any()
    assert history_df['Portfolio_Value'].iloc[-1] == 0
