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
    peak_price = btc_price_start

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

    for i in range(len(df)):
        current_date = df.index[i]
        current_price = df.iloc[i]['Close']

        if current_mode == 'X1':
            # Update peak
            if current_price > peak_price:
                peak_price = current_price

            # Current equity
            portfolio_value = btc_units * current_price

            # Check for drop from peak
            price_drop = (current_price - peak_price) / peak_price
            if price_drop <= -drop_threshold_pct / 100.0:
                # SWITCH TO XL
                current_mode = 'XL'
                closing_start_date = current_date
                weeks_passed = 0

                # We want total position = portfolio_value * leverage
                # New btc units = (portfolio_value * leverage) / price
                new_btc_units = (portfolio_value * target_leverage) / current_price
                debt = (new_btc_units - btc_units) * current_price
                btc_units = new_btc_units
                initial_units_to_close = btc_units # The total amount to de-leverage

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
                if debt > 0:
                    payment = min(proceeds, debt)
                    debt -= payment
                    proceeds -= payment

                # If there's leftover proceeds (profit from that 1/10th), buy X1 BTC
                if proceeds > 0:
                    added_units = proceeds / current_price
                    btc_units += added_units

                weeks_passed += 1

                # If 10 weeks passed, we are back to full X1 (debt should be 0 or small)
                if weeks_passed >= 10:
                    current_mode = 'X1'
                    # Reset peak to current price to avoid immediate re-trigger
                    peak_price = current_price
                    # Debt should ideally be 0 here if calculations are correct
                    portfolio_value = (btc_units * current_price) - debt
                    btc_units = portfolio_value / current_price
                    debt = 0.0

        history.append({
            'Date': current_date,
            'BTC_Price': current_price,
            'Portfolio_Value': portfolio_value,
            'Mode': current_mode
        })

    history_df = pd.DataFrame(history)
    # Buy & Hold for comparison
    history_df['Buy_Hold'] = initial_investment * (history_df['BTC_Price'] / btc_price_start)

    return history_df

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
