import numpy as np
import pandas as pd
import pytest

from net_realized_pnl import (
    build_net_realized_figure, bubble_sizes, compute_weekly_net_realized,
)


def test_weekly_aggregation_from_profit_and_loss_columns():
    df = pd.DataFrame({
        'day': ['2024-01-01', '2024-01-03', '2024-01-08'],  # deux lundis distincts
        'realized_profit_usd': [100.0, 50.0, 10.0],
        'realized_loss_usd': [-20.0, -30.0, 40.0],  # signe indifférent
    })
    weekly = compute_weekly_net_realized(df)
    assert list(weekly['week']) == [pd.Timestamp('2024-01-01'), pd.Timestamp('2024-01-08')]
    assert list(weekly['profit']) == [150.0, 10.0]
    assert list(weekly['loss']) == [50.0, 40.0]
    assert list(weekly['net']) == [100.0, -30.0]


def test_single_net_column_is_split_into_profit_and_loss():
    df = pd.DataFrame({'week': ['2024-01-01', '2024-01-08'], 'net_realized_pnl': [25.0, -5.0]})
    weekly = compute_weekly_net_realized(df)
    assert list(weekly['profit']) == [25.0, 0.0]
    assert list(weekly['loss']) == [0.0, 5.0]
    assert list(weekly['net']) == [25.0, -5.0]


def test_missing_columns_raise_explicit_error():
    with pytest.raises(ValueError, match="temporelle"):
        compute_weekly_net_realized(pd.DataFrame({'profit': [1]}))
    with pytest.raises(ValueError, match="profit/perte"):
        compute_weekly_net_realized(pd.DataFrame({'week': ['2024-01-01'], 'foo': [1]}))


def test_bubble_sizes_scale_with_magnitude_regardless_of_sign():
    sizes = bubble_sizes([0, 100, -400], min_size=5, max_size=25)
    assert sizes[0] == 5
    assert sizes[2] == 25
    assert sizes[1] == pytest.approx(15)  # sqrt(100)/sqrt(400) = 0.5


def test_bubble_sizes_all_zero():
    assert np.all(bubble_sizes([0, 0]) == bubble_sizes([0])[0])


def test_figure_colours_bubbles_by_sign_of_net():
    weekly = pd.DataFrame({
        'week': pd.to_datetime(['2024-01-01', '2024-01-08']),
        'profit': [10.0, 0.0], 'loss': [0.0, 5.0], 'net': [10.0, -5.0],
    })
    price = pd.Series(range(1, 15), index=pd.date_range('2024-01-01', periods=14, freq='D'), dtype=float)
    fig = build_net_realized_figure(weekly, price)
    names = [t.name for t in fig.data]
    profit_trace = fig.data[names.index('Net realized profit (semaine)')]
    loss_trace = fig.data[names.index('Net realized loss (semaine)')]
    assert list(profit_trace.x) == [pd.Timestamp('2024-01-01')]
    assert list(profit_trace.y) == [7.0]  # clôture du dimanche de la 1re semaine
    assert list(loss_trace.y) == [14.0]
