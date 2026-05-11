import pandas as pd
import numpy as np
import yfinance as yf
import plotly.graph_objects as go
from datetime import datetime, timedelta

def run_simulation(start_date, end_date, initial_investment, drop_threshold_pct, target_leverage=2.0):
    # 1. Download Data (with some buffer to detect peak before start if needed, but here we start at x1)
    df = yf.download('BTC-USD', start=start_date, end=end_date, interval='1d', progress=False)
    if df.empty:
        return None

    if isinstance(df.columns, pd.MultiIndex):
        df = pd.DataFrame({'Close': df['Close']['BTC-USD']})
    else:
        df = df[['Close']]

    df = df.sort_index()

    # 2. Simulation variables
    portfolio_value = float(initial_investment)
    btc_price_start = df.iloc[0]['Close']

    # State
    # current_mode: 'X1' or 'XL'
    current_mode = 'X1'
    all_time_high = btc_price_start
    waiting_for_recovery = False

    # Position tracking
    # If X1: btc_units = portfolio_value / price
    # If XL: we have btc_units and debt. Equity = btc_units * price - debt.
    btc_units = portfolio_value / btc_price_start
    debt = 0.0

    # Closing tracking
    closing_start_date = None
    initial_units_to_close = 0
    weeks_passed = 0

    history = []
    trades = []

    for i in range(len(df)):
        current_date = df.index[i]
        current_price = df.iloc[i]['Close']

        if current_mode == 'X1':
            # Update ATH de la séquence
            if current_price > all_time_high:
                all_time_high = current_price
                waiting_for_recovery = False # New ATH means we are no longer in a "drop" state relative to previous ATH

            # Current equity
            portfolio_value = btc_units * current_price

            # Check for drop from ALL TIME HIGH
            price_drop_from_ath = (current_price - all_time_high) / all_time_high

            # Condition de déclenchement : baisse de X% ET on n'est pas en train d'attendre une remontée
            if price_drop_from_ath <= -drop_threshold_pct / 100.0 and not waiting_for_recovery:
                # SWITCH TO XL
                current_mode = 'XL'
                closing_start_date = current_date
                weeks_passed = 0

                old_units = btc_units
                # We want total position = portfolio_value * leverage
                # New btc units = (portfolio_value * leverage) / price
                new_btc_units = (portfolio_value * target_leverage) / current_price
                debt = (new_btc_units - btc_units) * current_price
                btc_units = new_btc_units
                initial_units_to_close = btc_units # The total amount to de-leverage

                trades.append({
                    'Date': current_date,
                    'Action': f'Passage en Levier x{target_leverage:.1f}',
                    'Prix BTC': f'${current_price:,.2f}',
                    'Détails': f'Achat de {new_btc_units - old_units:.4f} BTC via dette (${debt:,.2f})'
                })

        elif current_mode == 'XL':
            # Current equity
            portfolio_value = (btc_units * current_price) - debt

            # Weekly check
            days_since_switch = (current_date - closing_start_date).days
            expected_weeks = days_since_switch // 7

            if expected_weeks > weeks_passed:
                # Sell 1/10th of initial XL units
                units_to_sell = initial_units_to_close / 10.0
                # If we have less than that (rounding), sell all
                units_to_sell = min(units_to_sell, btc_units)

                proceeds = units_to_sell * current_price
                btc_units -= units_to_sell

                # Use proceeds to pay debt first, then buy X1 BTC
                debt_paid = 0
                if debt > 0:
                    debt_paid = min(proceeds, debt)
                    debt -= debt_paid
                    proceeds -= debt_paid

                # If there's leftover proceeds (profit from that 1/10th), buy X1 BTC
                added_units = 0
                if proceeds > 0:
                    added_units = proceeds / current_price
                    btc_units += added_units

                trades.append({
                    'Date': current_date,
                    'Action': f'Sortie progressive (Semaine {expected_weeks}/10)',
                    'Prix BTC': f'${current_price:,.2f}',
                    'Détails': f'Vente {units_to_sell:.4f} BTC. Dette payée: ${debt_paid:,.2f}. Réinvesti: {added_units:.4f} BTC'
                })

                weeks_passed = expected_weeks

                # If 10 weeks passed, we are back to full X1 (debt should be 0 or small)
                if weeks_passed >= 10:
                    current_mode = 'X1'
                    # On revient en X1. On active l'attente de récupération si on est toujours sous le seuil de drop
                    portfolio_value = (btc_units * current_price) - debt
                    btc_units = portfolio_value / current_price
                    debt = 0.0

                    trades.append({
                        'Date': current_date,
                        'Action': 'Fin de phase Levier',
                        'Prix BTC': f'${current_price:,.2f}',
                        'Détails': 'Retour au mode X1 (100% investi)'
                    })

                    # Si on est toujours en baisse de X% par rapport à l'ATH, on attend de repasser au dessus
                    # du seuil avant de pouvoir redéclencher (pour "perdre à nouveau X%")
                    price_drop_from_ath = (current_price - all_time_high) / all_time_high
                    if price_drop_from_ath <= -drop_threshold_pct / 100.0:
                        waiting_for_recovery = True

        history.append({
            'Date': current_date,
            'BTC_Price': current_price,
            'Portfolio_Value': portfolio_value,
            'Mode': current_mode
        })

    history_df = pd.DataFrame(history)
    # Buy & Hold for comparison
    history_df['Buy_Hold'] = initial_investment * (history_df['BTC_Price'] / btc_price_start)

    trades_df = pd.DataFrame(trades)

    return history_df, trades_df

def get_simulator_plot(history_df):
    if history_df is None or history_df.empty:
        return None

    fig = go.Figure()

    # Portfolio Value
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Portfolio_Value'],
        name="Valeur Portefeuille (Stratégie)",
        line=dict(color='#00FFAA', width=2.5)
    ))

    # Buy & Hold
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Buy_Hold'],
        name="Buy & Hold BTC",
        line=dict(color='rgba(255, 165, 0, 0.6)', width=1.5, dash='dash')
    ))

    # BTC Price (Right Axis)
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['BTC_Price'],
        name="Prix BTC",
        line=dict(color='rgba(255, 255, 255, 0.2)', width=1),
        yaxis="y2"
    ))

    # Vertical Lines for Halvings
    halvings = [
        datetime(2012, 11, 28),
        datetime(2016, 7, 9),
        datetime(2020, 5, 11),
        datetime(2024, 4, 20)
    ]

    for h in halvings:
        if history_df['Date'].min() <= h <= history_df['Date'].max():
            fig.add_shape(
                type="line", x0=h, x1=h, y0=0, y1=1,
                yref="paper", line=dict(color="red", width=1, dash="dot")
            )
            fig.add_annotation(
                x=h, y=0.02, yref="paper", text="Halving",
                showarrow=False, font=dict(color="red", size=10),
                yanchor="bottom"
            )

    fig.update_layout(
        title="Simulation d'Investissement BTC - Stratégie de Levier Dynamique",
        xaxis_title="Date",
        xaxis=dict(range=[history_df['Date'].min(), history_df['Date'].max()]),
        yaxis=dict(title="Valeur du Portefeuille (USD)"),
        yaxis2=dict(title="Prix BTC (USD)", overlaying='y', side='right', showgrid=False),
        template="plotly_dark",
        height=700,
        hovermode="x unified",
        legend=dict(x=0.01, y=0.99, bgcolor='rgba(0,0,0,0.5)')
    )

    return fig
